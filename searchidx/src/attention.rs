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
}
