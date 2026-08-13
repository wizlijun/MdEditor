//! 注意力加权的全部算术与摄取。
//!
//! 唯一事实源是 vault 里 git 同步的 `.notemd/analytics/*.json`(Reading
//! Insights 采集,见 `src/lib/insights/model.ts`);索引里的 `doc_attention`
//! 表只是它的可弃派生缓存。本模块**只读** vault。
//!
//! 与 `query.rs` 里既有的 `doc_date` 新鲜度加成不是一回事:那个衰减的是
//! 「文档写于何时」,这个衰减的是「你何时在它身上花过时间」。两者独立叠加。

/// 30 天减半。设计规格 §4.2:匹配「手头活儿」的节奏 —— 不至于一周就忘,
/// 也不至于把去年读的书永久顶在前面。
pub const HALF_LIFE_DAYS: f64 = 30.0;

/// 只摄取最近这么多天的 analytics 文件。**这不是优化,是正确性边界**:
/// 30 天半衰期下 365 天前的贡献是 2^-12.2 ≈ 0.0002,低于任何有意义的精度,
/// 而没有这条截断,摄取成本会随 vault 的年龄线性膨胀。
pub const MAX_AGE_DAYS: i64 = 365;

/// 加成曲线的参考尺度:衰减后累计到 2 小时即封顶。
pub const REF_MINUTES: f64 = 120.0;

/// 编辑时间的权重。与看板 `value.ts` 的 `DEFAULT_WEIGHTS.edit` 一致:
/// 写过比读过重。
pub const EDIT_WEIGHT: f64 = 1.5;

/// `age_days` 天前的一分钟,今天还值多少。
///
/// 负 `age_days` 钳到 0:设备本地日的日期桶跨时区可能「来自未来」,时钟
/// 回拨也会造出同样的输入。让未来的投入拿到 >1 的乘数是纯粹的错误。
pub fn decay(age_days: i64) -> f64 {
    let age = age_days.max(0) as f64;
    0.5f64.powf(age / HALF_LIFE_DAYS)
}

/// 一天一文档的计数器折成注意力分钟数。负值(损坏文件)钳到 0。
pub fn minutes_of(read_ms: i64, edit_ms: i64) -> f64 {
    let read = read_ms.max(0) as f64;
    let edit = edit_ms.max(0) as f64;
    (read + EDIT_WEIGHT * edit) / 60_000.0
}

/// 排序加成。`stored_minutes` 是 `doc_attention.minutes`(已衰减到该表的
/// `as_of` 当天),`age_days` 是 `as_of` 到今天的天数 —— 表每天重算,但
/// app 开着不动时存量会冻住,这一项让陈旧的表优雅退化而不是发出过期高分。
///
/// **零注意力严格返回 1.0**(规格 §4.2):AI 刚生成、你还没读的文档注意力
/// 恒为 0,而搜索的场合恰恰常常是去找它们。惩罚未读 = 把新产出永久埋掉。
pub fn boost(stored_minutes: f64, age_days: i64, k: f64) -> f64 {
    if k <= 0.0 || !stored_minutes.is_finite() || stored_minutes <= 0.0 {
        return 1.0;
    }
    let m = stored_minutes * decay(age_days);
    let frac = (m.ln_1p() / REF_MINUTES.ln_1p()).min(1.0);
    1.0 + k * frac
}

use std::collections::BTreeMap;

/// 一份 `YYYY-MM-DD.<deviceId>.json` 里我们关心的部分。
#[derive(Debug, Clone)]
pub struct DayFile {
    /// 文件名里的日期桶(设备本地日,见 `insights/model.ts` 的 `dayKey`)。
    pub day: String,
    /// 文件名里的 deviceId。`abs:` 归因**必须**用它配对。
    pub device_id: String,
    pub docs: Vec<DocDay>,
}

/// 一天里一个文档的两个计时器。其余计数器(`open_count`、`mark_ops`…)
/// 刻意不读:批注已经由 `is_annotation ×1.2` 与 `origin=human ×1.25`
/// 加过两道了,再摆进来是同一件事数三遍(规格 §4.1)。
#[derive(Debug, Clone)]
pub struct DocDay {
    /// 原样的 docKey,含 `rel:` / `abs:` 前缀。
    pub key: String,
    pub read_ms: i64,
    pub edit_ms: i64,
}

/// 一条「某设备上的某绝对路径 ↔ vault 里的某镜像」记录,由调用方从
/// `.notemd/mirrors/` 读出后传入。
///
/// 由调用方供给而不是本 crate 自己读盘:`MirrorMeta` 的格式归
/// `src-tauri/src/sotvault/mirror_meta.rs` 所有,两个 crate 各解析一遍
/// 意味着格式一改就有一边静默错掉。
#[derive(Debug, Clone)]
pub struct MirrorLink {
    pub device_id: String,
    pub source: String,
    /// vault 相对路径,与 `files.path` 同域。
    pub mirror: String,
}

/// 把所有日文件折成 `vault 相对路径 → 衰减到 as_of 当天的注意力分钟数`。
///
/// 纯函数:同样的输入永远给同样的输出,不碰文件系统、不看时钟。摄取之所以
/// 是**全量重算**而不是增量累加,原因在规格 §3.1:当天的 analytics 文件整天
/// 都在被重写(存的是当日累计计数器,不是增量事件),任何水位方案算错时都是
/// **静默**的 —— 分数偏高,没有任何症状。
pub fn fold(files: &[DayFile], links: &[MirrorLink], as_of: &str) -> BTreeMap<String, f64> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    for f in files {
        let Some(age) = crate::query::days_between(&f.day, as_of) else { continue };
        if age > MAX_AGE_DAYS {
            continue;
        }
        let factor = decay(age);
        for d in &f.docs {
            let Some(path) = resolve(&d.key, &f.device_id, links) else { continue };
            let m = minutes_of(d.read_ms, d.edit_ms) * factor;
            if m > 0.0 {
                *out.entry(path).or_insert(0.0) += m;
            }
        }
    }
    out.retain(|_, v| *v > 0.0);
    out
}

/// docKey → vault 相对路径。`rel:` 去前缀;`abs:` 查同设备的镜像记录;
/// 都不匹配就是 `None`(丢弃,不猜)。
fn resolve(key: &str, device_id: &str, links: &[MirrorLink]) -> Option<String> {
    if let Some(rel) = key.strip_prefix("rel:") {
        return (!rel.is_empty()).then(|| rel.to_string());
    }
    let abs = key.strip_prefix("abs:")?;
    links
        .iter()
        .find(|l| l.device_id == device_id && l.source == abs)
        .map(|l| l.mirror.clone())
}

use std::path::Path;

/// analytics 子目录,与 `src/lib/insights/store.svelte.ts` 的 `SUBDIR` 一致。
const ANALYTICS_SUBDIR: &str = ".notemd/analytics";

/// 扫 `<vault>/.notemd/analytics/`,读出未超龄的日文件。
///
/// 全程尽力而为:目录不存在、单个文件损坏、文件名不认识 —— 都是跳过,
/// 不是错误。从没开过 Reading Insights 的用户的正常状态就是「没有目录」,
/// 那必须退化成「这一档恒等于 ×1.0」,而不是让索引报错。
///
/// **超龄判断在读盘之前**,只看文件名:这是 `MAX_AGE_DAYS` 真正省下 IO 的
/// 地方,十年老 vault 的摄取成本因此是常数而不是线性。
pub fn collect(vault_root: &Path, as_of: &str) -> Vec<DayFile> {
    let dir = vault_root.join(ANALYTICS_SUBDIR);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some((day, device_id)) = split_name(&name) else { continue };
        match crate::query::days_between(day, as_of) {
            Some(age) if age <= MAX_AGE_DAYS => {}
            _ => continue,
        }
        let Ok(txt) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(parsed) = serde_json::from_str::<DeviceAnalyticsJson>(&txt) else { continue };
        let docs: Vec<DocDay> = parsed
            .docs
            .into_iter()
            .flat_map(|(key, days)| {
                days.into_values().map(move |c| DocDay {
                    key: key.clone(),
                    read_ms: c.read_ms,
                    edit_ms: c.edit_ms,
                })
            })
            .collect();
        if docs.is_empty() {
            continue;
        }
        out.push(DayFile { day: day.to_string(), device_id: device_id.to_string(), docs });
    }
    out
}

/// `2026-08-13.<deviceId>.json` → `("2026-08-13", "<deviceId>")`。
/// deviceId 是 UUID(不含点),日期不含点,所以「第一个点」和「最后一个点」
/// 就是全部的分隔信息 —— 与 `store.svelte.ts` 的 `FILE_RE` 同一条约定。
fn split_name(name: &str) -> Option<(&str, &str)> {
    let stem = name.strip_suffix(".json")?;
    let (day, device) = stem.split_once('.')?;
    if day.len() != 10 || device.is_empty() {
        return None;
    }
    Some((day, device))
}

/// 只声明我们要读的字段。`deviceName`、`sessions`、以及 `DayCounters` 里
/// 其余的计数器都被 serde 忽略 —— 采集侧加字段不该让摄取失败。
#[derive(serde::Deserialize)]
struct DeviceAnalyticsJson {
    #[serde(default)]
    docs: std::collections::BTreeMap<String, std::collections::BTreeMap<String, CountersJson>>,
}

#[derive(serde::Deserialize)]
struct CountersJson {
    #[serde(default)]
    read_ms: i64,
    #[serde(default)]
    edit_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 半衰期 30 天:今天 ×1、30 天前 ×0.5、90 天前 ×0.125。
    #[test]
    fn decay_halves_every_thirty_days() {
        assert!((decay(0) - 1.0).abs() < 1e-9);
        assert!((decay(30) - 0.5).abs() < 1e-9);
        assert!((decay(90) - 0.125).abs() < 1e-9);
    }

    /// 负 age(时钟回拨、跨时区日期桶错位)绝不能放大分数。
    #[test]
    fn decay_never_amplifies_for_a_negative_age() {
        assert!((decay(-5) - 1.0).abs() < 1e-9, "未来的日期桶按今天算,不给额外奖励");
    }

    /// 编辑按 1.5 倍计:写过比读过重(与看板 DEFAULT_WEIGHTS.edit 一致)。
    #[test]
    fn edit_time_counts_one_and_a_half_times() {
        // 60_000 ms = 1 min
        assert!((minutes_of(60_000, 0) - 1.0).abs() < 1e-9);
        assert!((minutes_of(0, 60_000) - 1.5).abs() < 1e-9);
        assert!((minutes_of(60_000, 60_000) - 2.5).abs() < 1e-9);
    }

    /// 负计数器(损坏文件)不得产生负分钟数,否则会污染整份合计。
    #[test]
    fn negative_counters_clamp_to_zero() {
        assert_eq!(minutes_of(-1_000_000, -1_000_000), 0.0);
    }

    /// 设计规格 §4.2 的硬约束:零注意力**严格**等于 1.0,不是近似。
    /// 未读的文档(AI 刚生成、你还没看)必须原地不动,否则新产出会被永久埋掉。
    #[test]
    fn zero_attention_is_exactly_one() {
        assert_eq!(boost(0.0, 0, 0.4), 1.0);
        assert_eq!(boost(0.0, 999, 0.4), 1.0);
    }

    /// 规格 §4.2 的系数表。容差 5e-3 —— 钉的是曲线形状,不是浮点位。
    #[test]
    fn the_boost_table_matches_the_spec() {
        let k = 0.4;
        for (m, want) in [(5.0, 1.15), (30.0, 1.29), (60.0, 1.34), (120.0, 1.40)] {
            let got = boost(m, 0, k);
            assert!((got - want).abs() < 5e-3, "m={m} 期望 ≈{want},实得 {got}");
        }
    }

    /// 封顶:超过参考尺度后不再增长,挡住「一份文档读了 100 小时就霸榜」。
    #[test]
    fn the_boost_is_capped(){
        let k = 0.4;
        assert_eq!(boost(REF_MINUTES, 0, k), boost(10_000.0, 0, k));
        assert!((boost(REF_MINUTES, 0, k) - (1.0 + k)).abs() < 1e-9);
    }

    /// k=0 是「关掉这个功能」的正确表达,必须恒等于 1.0。
    #[test]
    fn k_zero_disables_the_boost_entirely() {
        for m in [0.0, 5.0, 120.0, 10_000.0] {
            assert_eq!(boost(m, 0, 0.0), 1.0, "k=0 时 m={m} 也不能动分数");
        }
    }

    /// 查询时的二次衰减:表陈旧 30 天 → 存的 60 分钟只值 30 分钟。
    #[test]
    fn a_stale_table_decays_again_at_query_time() {
        let k = 0.4;
        assert!((boost(60.0, 30, k) - boost(30.0, 0, k)).abs() < 1e-9);
    }

    fn df(day: &str, device: &str, docs: &[(&str, i64, i64)]) -> DayFile {
        DayFile {
            day: day.into(),
            device_id: device.into(),
            docs: docs
                .iter()
                .map(|(k, r, e)| DocDay { key: (*k).into(), read_ms: *r, edit_ms: *e })
                .collect(),
        }
    }

    /// `rel:` key 直接去掉前缀就是索引里的 `files.path`。
    #[test]
    fn rel_keys_become_vault_relative_paths() {
        let m = fold(&[df("2026-08-13", "d1", &[("rel:notes/a.md", 60_000, 0)])], &[], "2026-08-13");
        assert!((m["notes/a.md"] - 1.0).abs() < 1e-9);
        assert_eq!(m.len(), 1);
    }

    /// 同一文档跨天、跨设备求和,各自按自己的日期衰减。
    #[test]
    fn the_same_doc_sums_across_days_and_devices() {
        let m = fold(
            &[
                df("2026-08-13", "d1", &[("rel:a.md", 60_000, 0)]),  // 今天 → 1.0
                df("2026-07-14", "d2", &[("rel:a.md", 60_000, 0)]),  // 30 天前 → 0.5
            ],
            &[],
            "2026-08-13",
        );
        assert!((m["a.md"] - 1.5).abs() < 1e-6, "实得 {}", m["a.md"]);
    }

    /// 截断边界:第 365 天计入,第 366 天不计。**这条是成本上界的守卫**,
    /// 删掉它摄取会随 vault 年龄线性变慢而没人发现。
    #[test]
    fn the_age_cutoff_includes_day_365_and_excludes_day_366() {
        // 2026-08-13 往前 365 天 = 2025-08-13;366 天 = 2025-08-12
        let m = fold(
            &[
                df("2025-08-13", "d1", &[("rel:in.md", 60_000, 0)]),
                df("2025-08-12", "d1", &[("rel:out.md", 60_000, 0)]),
            ],
            &[],
            "2026-08-13",
        );
        assert!(m.contains_key("in.md"), "第 365 天必须计入");
        assert!(!m.contains_key("out.md"), "第 366 天必须被截断");
    }

    /// 信念 4:vault 外源文件的阅读时长归给它在 vault 里的镜像副本。
    #[test]
    fn abs_keys_are_credited_to_their_mirror() {
        let links = vec![MirrorLink {
            device_id: "d1".into(),
            source: "/Users/bruce/Downloads/x.md".into(),
            mirror: "sync/x.md".into(),
        }];
        let m = fold(
            &[df("2026-08-13", "d1", &[("abs:/Users/bruce/Downloads/x.md", 60_000, 0)])],
            &links,
            "2026-08-13",
        );
        assert!((m["sync/x.md"] - 1.0).abs() < 1e-9);
    }

    /// **必须按 deviceId 配对。** 两台机器上的同一个绝对路径是两个不同的
    /// 文件;跨设备匹配 `source` 会把别人的阅读时间算到你的镜像上。
    #[test]
    fn abs_keys_do_not_match_another_devices_mirror_link() {
        let links = vec![MirrorLink {
            device_id: "OTHER".into(),
            source: "/Users/bruce/Downloads/x.md".into(),
            mirror: "sync/x.md".into(),
        }];
        let m = fold(
            &[df("2026-08-13", "d1", &[("abs:/Users/bruce/Downloads/x.md", 60_000, 0)])],
            &links,
            "2026-08-13",
        );
        assert!(m.is_empty(), "设备不匹配时必须丢弃,不能猜");
    }

    /// 查不到镜像的 `abs:` key 直接丢弃 —— 它指向一个不在索引里的文件。
    #[test]
    fn unmapped_abs_keys_are_dropped() {
        let m = fold(&[df("2026-08-13", "d1", &[("abs:/tmp/y.md", 60_000, 0)])], &[], "2026-08-13");
        assert!(m.is_empty());
    }

    /// 无法解析的日期不能让整批摄取失败,只跳过该文件。
    #[test]
    fn an_unparseable_day_is_skipped_not_fatal() {
        let m = fold(
            &[
                df("not-a-date", "d1", &[("rel:bad.md", 60_000, 0)]),
                df("2026-08-13", "d1", &[("rel:good.md", 60_000, 0)]),
            ],
            &[],
            "2026-08-13",
        );
        assert!(!m.contains_key("bad.md"));
        assert!(m.contains_key("good.md"));
    }

    /// 零分钟的条目不进表:它对排序毫无作用,只会让表和 LEFT JOIN 变大。
    #[test]
    fn zero_minute_entries_are_not_stored() {
        let m = fold(&[df("2026-08-13", "d1", &[("rel:a.md", 0, 0)])], &[], "2026-08-13");
        assert!(m.is_empty());
    }

    use std::path::Path;

    fn write_day(root: &Path, name: &str, body: &str) {
        let dir = root.join(".notemd/analytics");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    /// 正常路径:文件名解析出 day + deviceId,JSON 里取两个计时器。
    #[test]
    fn collect_reads_day_and_device_from_the_filename() {
        let d = tempfile::tempdir().unwrap();
        write_day(
            d.path(),
            "2026-08-13.DEV-1.json",
            r#"{"deviceId":"DEV-1","deviceName":"mac","docs":{
                 "rel:a.md":{"2026-08-13":{"read_ms":60000,"edit_ms":1000,"open_count":2,
                   "edit_sessions":1,"net_chars":10,"mark_ops":0,
                   "first_seen_at":0,"last_active_at":0}}}}"#,
        );
        let files = collect(d.path(), "2026-08-13");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].day, "2026-08-13");
        assert_eq!(files[0].device_id, "DEV-1");
        assert_eq!(files[0].docs.len(), 1);
        assert_eq!(files[0].docs[0].key, "rel:a.md");
        assert_eq!(files[0].docs[0].read_ms, 60_000);
        assert_eq!(files[0].docs[0].edit_ms, 1_000);
    }

    /// 超龄文件**在读盘前**就被文件名筛掉 —— 这是 MAX_AGE_DAYS 省 IO 的地方,
    /// 不只是省算术。
    #[test]
    fn collect_skips_files_older_than_the_cutoff_without_reading_them() {
        let d = tempfile::tempdir().unwrap();
        write_day(d.path(), "2020-01-01.DEV-1.json", "这不是合法 JSON,但也不该被读");
        assert!(collect(d.path(), "2026-08-13").is_empty());
    }

    /// 单个损坏文件跳过,其余照常 —— 规格 §7 的容错要求。
    #[test]
    fn a_corrupt_file_is_skipped_and_the_rest_still_load() {
        let d = tempfile::tempdir().unwrap();
        write_day(d.path(), "2026-08-12.DEV-1.json", "{ 半个 JSON");
        write_day(
            d.path(),
            "2026-08-13.DEV-1.json",
            r#"{"deviceId":"DEV-1","deviceName":"m","docs":{"rel:ok.md":{"2026-08-13":{"read_ms":1,"edit_ms":0,"open_count":0,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}"#,
        );
        let files = collect(d.path(), "2026-08-13");
        assert_eq!(files.len(), 1, "损坏的那个跳过,好的那个还在");
        assert_eq!(files[0].docs[0].key, "rel:ok.md");
    }

    /// 没有目录不是错误 —— 从没开过洞察的用户就是这个状态。
    #[test]
    fn a_missing_analytics_dir_is_empty_not_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(collect(d.path(), "2026-08-13").is_empty());
    }

    /// 文件名不符合 `<day>.<deviceId>.json` 的一律忽略(README、.DS_Store…)。
    #[test]
    fn unrecognized_filenames_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        write_day(d.path(), "README.md", "x");
        write_day(d.path(), ".DS_Store", "x");
        write_day(d.path(), "2026-08-13.json", "x");
        assert!(collect(d.path(), "2026-08-13").is_empty());
    }

    /// 文件名里的日期是权威,JSON 里内嵌的日期桶键**不覆盖**它。
    /// 两者本该一致;不一致时(手工编辑、同步冲突残留)以文件名为准,
    /// 因为超龄截断就是按文件名做的,让内嵌键翻案会绕过截断。
    #[test]
    fn the_filename_day_wins_over_the_inner_bucket_key() {
        let d = tempfile::tempdir().unwrap();
        write_day(
            d.path(),
            "2026-08-13.DEV-1.json",
            r#"{"deviceId":"DEV-1","deviceName":"m","docs":{"rel:a.md":{"1999-01-01":{"read_ms":60000,"edit_ms":0,"open_count":0,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}"#,
        );
        let files = collect(d.path(), "2026-08-13");
        assert_eq!(files[0].day, "2026-08-13");
    }
}
