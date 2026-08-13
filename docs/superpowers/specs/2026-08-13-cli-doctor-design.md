# CLI doctor 自检 —— 设计

> 类型:设计规格 · 日期:2026-08-13
> 涉及:`src-tauri/src/cli/`(新增 `doctor.rs`,改 `router.rs` / `builtin.rs`)

## 0 · 一句话

`notemd doctor` 一条命令跑完全部本地健康检查(环境 / 配置与 vault / 搜索索引 / 插件系统)加两项网络探测,**只诊断、只给建议、绝不改动任何东西**;全部检查复用各子系统已有的判断原语,不另写第二份「什么算健康」的逻辑。

## 1 · 为什么

notemd 的能力分散在多个子系统里:shared config、vault、搜索索引、插件市场、git 同步、CLI 软链。任何一个坏了,症状都出现在别处 —— 索引 stamp 不一致表现为「搜不到」,软链 dangling 表现为「命令不存在」,插件架构二进制缺失表现为「装了却没反应」(这三种都在真实事故里出现过,见 `dev-install 抢占版本号`、`插件索引合并式发布` 等记录)。用户和 agent 排查时需要逐个子系统翻状态文件;`doctor` 把这些判断收拢成一条命令,是 `brew doctor` / `flutter doctor` 的通行做法。

**判断逻辑必须复用,不能重写。** 每个子系统「什么算健康」已经有唯一权威实现:`install::status` 知道 dangling 软链、`discovery::scan_root` 的校验链知道插件何时该被跳过、`SearchIndex::open` 的 globs stamp 知道索引何时失效。doctor 若自带一份判断,两份必然漂移 —— 这是本项目反复踩过的「第二真相来源」坑(见重命名检测 spec §1.1 第 4 条)。

## 2 · 范围

**做:**
- 核心环境:CLI 软链、git 可用性、git proxy 合法性
- 配置与 vault:`shared.json`、sotvault 目录、vault git 仓库、`.notemd/settings.json`
- 搜索索引:可打开、统计摘要、被跳过的大文件
- 插件系统:根目录、`state.json`、逐插件 manifest / 架构二进制校验
- 网络(默认做,`--offline` 跳过):插件市场 registry、updater endpoint
- `--json` 输出、退出码约定、`help doctor` 长帮助

**不做:**
- 不做 `--fix`。首版只报告 + 每条失败附一行修复建议;自动修复以后按需加。
- 不查 agent 约定(AGENTS.md / CLAUDE.md 软链 / OKF human id)。首版明确排除,以后可作为独立检查组加入。
- 不查 vault_sync 运行态(`SyncState`)。它是 AppHandle/manager 绑定的进程内状态,CLI 新进程看不到;只查它的静态前提(git 可用、是 git 仓库、proxy 合法)。
- 不查通知/日志总线。均为进程内内存态,新进程恒空,查了也是假数据。
- 不为 doctor 引入任何新的状态文件或缓存。

## 3 · 命令与路由(规范性)

- **纯 builtin,无 webview。** 与 `search` 同一执行平面;`src-tauri/tests/cli_startup_timing.rs` 的启动时序断言必须继续通过。
- 路由注册在 `router.rs` 的插件 manifest 匹配**之前**(与 `search` 的 `router.rs:91` 精确同款):`doctor` 不可被插件遮蔽、不可被 disable。
- 参数:接受全局 `--json` / `-q`;自有 `--offline`(跳过网络组)、`--vault <path>`(显式指定 vault,复用 `search::resolve_vault_root` 的解析次序)。
- 帮助:`CORE COMMANDS` 列表加一行;`help doctor` 长帮助含自己的 `EXIT CODES:` 段(理由见 §5)。
- 子进程一律走 `platform::command()`,不得直接 `std::process::Command::new`(Windows 无控制台闪烁规则)。

## 4 · 检查项(规范性)

每项检查产出一条记录:`{ id, group, status, detail, hint }`。`status ∈ { pass, warn, fail, skip }`;`hint` 仅在非 pass 时给出,内容是**一条可执行的下一步**(命令或指向哪个设置界面),不是解释。

### 4.1 环境(env)

| id | 判断 | 复用 | 失败语义 |
| --- | --- | --- | --- |
| `env.cli_link` | 软链已装且指向有效 | `install::status()`(已区分未装 / dangling) | 未装 = warn(GUI 用户不必装);dangling = fail |
| `env.git` | `git` 在 PATH 上可执行 | `git_ops::version()` | fail(vault 同步的硬前提;git_ops.rs:11 的注释即此义) |
| `env.git_proxy` | `shared.json` 的 `git_proxy` 若非空则合法 | `git_ops::validate_proxy_url()` | 非法 = fail;未配置 = pass |

### 4.2 配置与 vault(vault)

| id | 判断 | 复用 | 失败语义 |
| --- | --- | --- | --- |
| `vault.shared_config` | `shared.json` 存在且是可解析 JSON | 自读文件 + `serde_json` 解析(**不走** `shared_config::read()` —— 它 fail-soft,缺失与损坏都吞成默认值,doctor 恰恰要区分这两态) | 缺失 = warn(全新安装属正常);损坏 = fail |
| `vault.sotvault` | `sotvault` 已配置且目录存在 | `search::resolve_vault_root` 同款判断(search.rs:135-141) | 未配置 = warn;配置了但目录不存在 = fail |
| `vault.git_repo` | vault 下 `.git` 存在 | `Path::exists` | warn(「文件高于应用」下 vault 可以不是 git 仓库,但同步能力不可用) |
| `vault.settings` | `.notemd/settings.json` 可解析、search weights 在界内 | `vault_settings` 的解析 + `Weights::sanitized()` 前后比对 | 损坏 = fail;weights 越界被夹 = warn;文件不存在 = pass(全默认合法) |

sotvault 未配置或目录不存在时,`vault.git_repo` / `vault.settings` 及整个搜索索引组记 `skip`,不连坐报 fail。

### 4.3 搜索索引(search)

| id | 判断 | 复用 | 失败语义 |
| --- | --- | --- | --- |
| `search.index_open` | 索引 DB 可打开 | `SearchIndex::open(&root, &stamp)`,stamp **必须**来自 `scan_options_for(root).source_globs.stamp()`(cli/search.rs:241)—— 换任何别的算法都会把好索引误判为失效 | 打不开 = warn(search 有直接扫描兜底,不是硬故障) |
| `search.stats` | 统计摘要:文件数 / 块数 / DB 大小 / 构建时间 | `idx.stats()`,渲染对齐 `report_stats`(search.rs:462)的字段命名 | 恒 pass,纯信息;`files == 0` 且 vault 非空 = warn |
| `search.skipped_large` | 有无因超限被跳过、对搜索不可见的大文件 | 增量 sweep 结果的 `files_skipped_large`,sweep 与 search 命令同款、同 2s 预算(`SWEEP_DEADLINE`) | 非空 = warn,detail 列文件,hint 指向 `.notemd/settings.json` 的阈值项;sweep 超时 = detail 注明数据可能不全 |

doctor **不做全量首建**:索引不存在时不触发 `ensure_built`(可能是长任务,违背「diagnose-only」),记 warn 并提示跑一次 `notemd search --stats`;仅当索引已存在时才跑上述 2s 增量 sweep —— 那是 search 每次调用都会做的派生数据维护,不算改动用户数据。

### 4.4 插件系统(plugin)

| id | 判断 | 复用 | 失败语义 |
| --- | --- | --- | --- |
| `plugin.root` | plugins 根目录存在 | `market::plugins_root()`(与 `runner::v2_plugins_root` 的一致性已有合同测试钉住) | 不存在 = pass + detail 注明「未安装任何插件」 |
| `plugin.state` | `state.json` 可解析 | 自读 + 解析(同 §4.2 理由,不走 fail-soft 的 `state::load()`) | 损坏 = fail |
| `plugin.<id>` | 每个已启用插件:manifest 可读可解析、通过 `plugin_protocol::validate_manifest`、`m.id == 目录名`、当前架构(`current_arch_triple()`)有二进制 | `discovery::scan_root` 的校验链 —— **必须调用同一实现**(现状是私有逐步校验则提为可复用函数),不得在 doctor 里复刻这串判断 | 任一步失败 = fail,detail 即 discovery 会打的那条 skip 原因 |

### 4.5 网络(net)—— `--offline` 时整组 skip

| id | 判断 | 复用 | 失败语义 |
| --- | --- | --- | --- |
| `net.registry` | 插件市场 index 可取 | `market::fetch_index(registry_base())`,含已有 10s 超时;runtime 复用 builtin.rs:552 的 current-thread tokio 惯例 | 失败 = **warn**(网络问题不是安装损坏) |
| `net.updater` | updater endpoint 的 `latest.json` 可 GET | 同一 HTTP client 惯例 + 10s 超时;URL 取自 `tauri.conf.json` 的 endpoints(编译期 `include_str!` 或常量,不引入运行时配置解析) | 失败 = **warn** |

网络组两项**并发发起**,doctor 总耗时上界 ≈ 单项超时(10s),而非累加。

## 5 · 输出与退出码(规范性)

**流纪律**沿用仓库约定(stdout = 结果,stderr = 进度):检查过程不打进度(本地检查是毫秒级,网络组静默等待),**报告整体是结果,走 stdout**。

**人类可读**:按组分节,每行 `✓ / ⚠ / ✗ / -`(pass/warn/fail/skip)+ id + detail;非 pass 项下一行缩进两格给 hint(对齐 `shareVaultDiagnostics` 的缩进惯例)。末尾一行 summary:`N passed, N warnings, N failures, N skipped`。

**`--json`**:标准信封 `{"ok": <无 fail>, "data": { "checks": [...], "summary": {...} }}`,字段 snake_case。不学 `search --json` 的裸对象例外 —— 那是 grep 形状命令的特例,doctor 是标准命令。

**退出码**:

| 码 | 含义 |
| --- | --- |
| 0 | 无 fail(允许 warn / skip) |
| 1 | 至少一项 fail |
| 2 | 参数错误(未知 flag 等) |

`search` 的「1 = 无命中非错误」是它自己的例外;doctor 用通用约定「1 = 发现问题」,`help doctor` 的 EXIT CODES 段写明,消费方(CI / agent)据此可直接 `notemd doctor && ...`。

**warn 不影响退出码** —— 这是刻意的:未装软链、vault 非 git 仓库、网络不可达都是合法运行态,doctor 报 0 才能安全进脚本;要严格模式以后再加 `--strict`,首版不做。

## 6 · 结构

`doctor.rs` 内部:每项检查是一个签名统一的函数,吃显式参数(`&Path` / 配置值),**不摸全局状态**,返回上述记录结构;`run()` 负责编排(含网络组并发)、渲染、算退出码。检查函数与渲染分离 —— 前者可用 tempdir 直接单测,后者对着固定记录集验格式。

## 7 · 测试

**TDD,先测后码。**

单元(doctor.rs 内,tempdir 构造三态:缺失 / 损坏 / 正常):

1. `shared.json` 缺失 → warn;放一个非 JSON 文件 → fail —— **钉住「不走 fail-soft read()」这条**:若实现偷懒改用 `shared_config::read()`,两态都变 pass,此测必红
2. sotvault 配置了但目录不存在 → fail;未配置 → warn 且 vault/search 组整体 skip(不连坐)
3. `state.json` 损坏 → fail;插件目录含 manifest id 与目录名不符的插件 → 该 `plugin.<id>` fail 且 detail 含原因
4. 退出码:全 pass = 0;仅 warn = 0;任一 fail = 1
5. JSON 渲染:信封结构、`ok` 与 fail 数一致、snake_case

集成(`cli_builtin_integration.rs`,真二进制 + 隔离 `$HOME`):

6. `notemd doctor --offline --json` 在干净 HOME 下:退出 0,`ok: true`,net 组两项 `skip`
7. `help` 输出含 doctor;`help doctor` 含 EXIT CODES 段
8. `doctor --offline` 人类可读输出含 summary 行

网络组不进任何自动测试(默认 `--offline`);实现后手工跑一次 `notemd doctor` 验证真实联网路径即可。

**mutation 验证(必需):** 把退出码计算里的 fail 判断改成恒 0,第 4 条必须变红;把 `vault.shared_config` 的实现换成 `shared_config::read()`,第 1 条必须变红。

## 8 · 残余风险

- **updater endpoint 探测只验证可达,不验证签名链。** 「能 GET latest.json」不等于「升级一定成功」(v6.720.1 的架构错包、v5.0.2 的签名失配都不是可达性问题)。doctor 不试图覆盖发布侧质量门,那是 release.sh 的职责。
- **discovery 校验链若日后加步骤,doctor 自动跟随**(因为调的是同一实现)—— 但若有人在 discovery 里加了 AppHandle 依赖,doctor 的复用会被迫断开;届时该把纯校验部分保持为无句柄函数,而不是在 doctor 里复刻。
- **网络组 10s 超时在慢网下会被用户感知为「卡住」。** 接受:有 `--offline` 逃生门,且两项并发上界即单项超时。
