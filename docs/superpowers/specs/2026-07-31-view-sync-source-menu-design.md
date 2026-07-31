# 查看 Sync 的源文件 — 菜单项设计

日期:2026-07-31

## 问题

打开 Sync(vault)目录下的镜像 `.md` 时,用户无法知道它是从哪里 sync 过来的、源文件在磁盘上的哪个位置。

## 目标

在 **文件 / 同步到Vault** 菜单下新增「查看 Sync 的源文件」菜单项:点击后在 Finder 中打开并选中对应的源文件。非 Sync 镜像文件时该项 disable。

## 行为

- 菜单位置:File 菜单里,紧跟现有 `sync-to-vault` 项之后,新增 `view-sync-source`。
- 点击动作:调用现有 `revealVaultSource(deviceSourceForVaultPath(当前文件路径))`,即已有的 `@tauri-apps/plugin-opener` 的 `revealItemInDir` —— 在 Finder 中打开源文件所在目录并选中该文件。
- 源文件已被移动/删除时:`revealItemInDir` 失败,走已有的 `sotvault.revealFailed` toast,不新增错误分支。

## 启用/禁用条件

启用当且仅当:当前活动文件是 vault 镜像(`isMirrorPath` 为真)**且本机记录了源文件绝对路径**(`deviceSourceForVaultPath` 返回非空)。

其余一律 disable:
- 普通文件、纯 vault 文件(非镜像);
- **从另一台设备 sync 过来、本机无源路径记录的镜像** —— Finder 无法定位另一台机器上的路径,与现有 `SyncOriginBanner`「本机有源才显示 reveal 按钮」的行为保持一致。

## 复用的现有能力(无需新增后端命令 / 新逻辑函数)

- `isMirrorPath(path)` — `src/lib/sotvault.svelte.ts`
- `deviceSourceForVaultPath(path)` — `src/lib/sotvault.svelte.ts`(返回本机源绝对路径或 null)
- `revealVaultSource(sourcePath)` — `src/lib/sotvault.svelte.ts`
- 该菜单项本质是现有 `src/components/SyncOriginBanner.svelte` reveal 按钮的菜单栏等价物。

## 改动点(6 处,均沿用现有约定)

1. `src-tauri/src/lib.rs`
   - File 子菜单(~1795 行,`sync-to-vault` 之后)新增
     `MenuItemBuilder::with_id("view-sync-source", menu_label(locale, "file.viewSyncSource"))`。
   - `menu_label` 匹配(~1405 行)新增四语标签元组
     `"file.viewSyncSource" => ("View Sync Source", "查看 Sync 的源文件", "Sync 元ファイルを表示", "Sync-Quelldatei anzeigen")`(顺序 en, zh, ja, de;文案定稿以实现时为准)。
   - 点击经由已有的全局 `menu-event` 转发到前端,无需新增 Rust 分发分支。

2. `src/lib/plugins/types.ts`
   - `EnabledWhenContext` 新增 `hasSyncSource?: boolean`(与 `canSyncToVault` 并列)。

3. `src/App.svelte`
   - 构造 `ewTab` 时(~711–723 行)计算
     `hasSyncSource: isMirrorPath(fp) && !!deviceSourceForVaultPath(fp)`。

4. `src/lib/plugins/menu-registry.ts`
   - `CORE_MENU_ENABLED_ITEMS` 新增一条
     `{ id: 'view-sync-source', pluginId: 'sotvault', command: 'view-sync-source', label: '', enabledWhen: 'currentTab.hasSyncSource' }`。

5. `src/lib/commands.ts`
   - `CommandId` 联合类型新增 `'view-sync-source'`。
   - `handlers` 新增 `'view-sync-source': viewSyncSource`,handler:
     取当前活动 tab 的 `filePath`,`const src = deviceSourceForVaultPath(fp); if (src) await revealVaultSource(src)`。

6. 无需新增 Rust 命令、无需改 updater/发布脚本。

## 验证

- `pnpm check` + 相关测试通过。
- dev 构建实机验证(GUI 改动须实机):
  - 打开一个 vault 里的镜像 `.md` → 菜单项 enable,点击 → Finder 选中源文件。
  - 打开普通文件 / 纯 vault 文件 → 菜单项 disable。
  - (若可复现)打开另一台设备 sync 来的镜像 → disable。

## 非目标

- 不为跨设备镜像展示远端路径或提示。
- 不新增 reveal 相关的后端命令。
- 不改动 `SyncOriginBanner` 现有行为。
