//! Provider-neutral token accounting stored with every completed run.
//!
//! Adapters normalize provider payloads into disjoint input buckets before they
//! get here.  That matters because some APIs report cached tokens as a subset of
//! `input_tokens`, while others report them beside ordinary input tokens.
use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostKind {
    /// Amount emitted by the harness/provider itself.
    ProviderReported,
    /// API list-price equivalent calculated by note.md.
    ListPriceEstimate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub amount_usd: f64,
    pub kind: CostKind,
    /// Date/version of the price catalog used for an estimate. A provider-
    /// reported amount does not need one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing_as_of: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Exact model reported or resolved for this invocation. Multi-model
    /// providers may leave this empty while still reporting an aggregate cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Non-cached, non-cache-write input tokens.
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// A subset of output_tokens; displayed as a breakdown, never billed twice.
    #[serde(default)]
    pub reasoning_tokens: u64,
    /// Provider total when available. If absent/zero, [`total_tokens`] derives
    /// the disjoint total.
    #[serde(default)]
    pub reported_total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        if self.reported_total_tokens > 0 {
            self.reported_total_tokens
        } else {
            self.input_tokens
                .saturating_add(self.cache_read_tokens)
                .saturating_add(self.cache_write_tokens)
                .saturating_add(self.output_tokens)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total_tokens() == 0 && self.cost.is_none()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rates {
    pub input_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_write_per_million: f64,
    pub output_per_million: f64,
}

/// Anthropic prices 5-minute and 1-hour cache writes differently. Claude's
/// aggregate cache-write bucket stays provider-neutral; the adapter passes this
/// optional wire-level breakdown only while calculating the estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnthropicCacheWrite {
    pub five_minute_tokens: u64,
    pub one_hour_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepseekPriceBand {
    Peak,
    OffPeak,
}

pub fn estimate(usage: &Usage, rates: Rates, pricing_as_of: &str) -> Cost {
    let million = 1_000_000_f64;
    let amount = usage.input_tokens as f64 * rates.input_per_million
        + usage.cache_read_tokens as f64 * rates.cache_read_per_million
        + usage.cache_write_tokens as f64 * rates.cache_write_per_million
        + usage.output_tokens as f64 * rates.output_per_million;
    Cost {
        amount_usd: amount / million,
        kind: CostKind::ListPriceEstimate,
        pricing_as_of: Some(pricing_as_of.to_string()),
    }
}

/// Current OpenAI API list-price equivalent for Codex models. This is never
/// presented as an actual Codex subscription charge. Unknown models return
/// None rather than silently borrowing another model's rate.
pub fn estimate_openai(model: &str, usage: &Usage) -> Option<Cost> {
    const AS_OF: &str = "2026-09-02";
    let rates = match model.trim() {
        "gpt-5.6" | "gpt-5.6-sol" => Rates {
            input_per_million: 4.0,
            cache_read_per_million: 0.4,
            cache_write_per_million: 5.0,
            output_per_million: 20.0,
        },
        "gpt-5.6-terra" => Rates {
            input_per_million: 2.0,
            cache_read_per_million: 0.2,
            cache_write_per_million: 2.5,
            output_per_million: 12.0,
        },
        "gpt-5.6-luna" => Rates {
            input_per_million: 0.2,
            cache_read_per_million: 0.02,
            cache_write_per_million: 0.25,
            output_per_million: 1.2,
        },
        _ => return None,
    };
    if priceable_tokens(usage) == 0 {
        return None;
    }
    // Codex reports one aggregate for the whole agent turn, which may contain
    // several model requests. The >272K premium is decided per request, so an
    // aggregate above that threshold cannot be priced faithfully: it may be
    // one premium request or many ordinary ones. Keep the tokens and omit the
    // estimate instead of presenting a made-up bill.
    let all_input = usage
        .input_tokens
        .saturating_add(usage.cache_read_tokens)
        .saturating_add(usage.cache_write_tokens);
    if all_input > 272_000 {
        return None;
    }
    Some(estimate(usage, rates, AS_OF))
}

/// Anthropic first-party, global, standard-speed token list prices. Dynamic
/// Claude Code aliases and cloud-provider model identifiers deliberately do
/// not match: their effective model or regional price cannot be inferred.
pub fn estimate_anthropic(
    model: &str,
    usage: &Usage,
    cache_write: Option<AnthropicCacheWrite>,
) -> Option<Cost> {
    const AS_OF: &str = "2026-09-02";
    let (input, cache_5m, cache_1h, cache_read, output) = match model.trim() {
        "claude-fable-5-1" | "claude-mythos-5-1" => (10.0, 12.5, 20.0, 0.25, 50.0),
        "claude-fable-5" | "claude-mythos-5" => (10.0, 12.5, 20.0, 1.0, 50.0),
        "claude-opus-5"
        | "claude-opus-4-8"
        | "claude-opus-4-7"
        | "claude-opus-4-6"
        | "claude-opus-4-5"
        | "claude-opus-4-5-20251101" => (5.0, 6.25, 10.0, 0.5, 25.0),
        "claude-sonnet-5" => (2.0, 2.5, 4.0, 0.2, 10.0),
        "claude-sonnet-4-6" | "claude-sonnet-4-5" | "claude-sonnet-4-5-20250929" => {
            (3.0, 3.75, 6.0, 0.3, 15.0)
        }
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => (1.0, 1.25, 2.0, 0.1, 5.0),
        _ => return None,
    };
    if priceable_tokens(usage) == 0 {
        return None;
    }

    let (five_minute_tokens, one_hour_tokens, pricing_as_of) = match cache_write {
        Some(split) => {
            if split.five_minute_tokens.checked_add(split.one_hour_tokens)
                != Some(usage.cache_write_tokens)
            {
                return None;
            }
            (split.five_minute_tokens, split.one_hour_tokens, AS_OF)
        }
        // Older result frames expose only the aggregate. Use the official 1h
        // list rate as a conservative upper estimate instead of silently
        // assuming the cheaper TTL.
        None if usage.cache_write_tokens > 0 => (
            0,
            usage.cache_write_tokens,
            "2026-09-02 (cache TTL unavailable; conservative 1h write rate)",
        ),
        None => (0, 0, AS_OF),
    };
    let million = 1_000_000_f64;
    let amount = usage.input_tokens as f64 * input
        + usage.cache_read_tokens as f64 * cache_read
        + five_minute_tokens as f64 * cache_5m
        + one_hour_tokens as f64 * cache_1h
        + usage.output_tokens as f64 * output;
    Some(Cost {
        amount_usd: amount / million,
        kind: CostKind::ListPriceEstimate,
        pricing_as_of: Some(pricing_as_of.to_string()),
    })
}

/// DeepSeek's public price band is selected by UTC: weekdays 01:00–04:00 and
/// 06:00–10:00 are peak, with half-price rates at all other times.
pub fn deepseek_price_band(at: DateTime<Utc>) -> DeepseekPriceBand {
    let weekday = at.weekday();
    let hour = at.hour();
    if !matches!(weekday, Weekday::Sat | Weekday::Sun)
        && ((1..4).contains(&hour) || (6..10).contains(&hour))
    {
        DeepseekPriceBand::Peak
    } else {
        DeepseekPriceBand::OffPeak
    }
}

/// DeepSeek USD list-price estimate using the price band at `at`. ACP usage is
/// a completed-turn aggregate, so this is an estimate rather than a bill
/// reconstruction when a run crosses a peak/off-peak boundary.
pub fn estimate_deepseek(model: &str, usage: &Usage, at: DateTime<Utc>) -> Option<Cost> {
    if priceable_tokens(usage) == 0 {
        return None;
    }
    let pro = match model.trim() {
        "deepseek-v4-flash" | "deepseek-v4-flash-0731" | "deepseek-v4-flash-vision-exp" => false,
        "deepseek-v4-pro" | "deepseek-v4-pro-0813" => true,
        _ => return None,
    };
    let price_band = deepseek_price_band(at);
    let rates = match (pro, price_band) {
        (false, DeepseekPriceBand::OffPeak) => Rates {
            input_per_million: 0.22,
            cache_read_per_million: 0.007,
            // DeepSeek automatically creates cache entries and publishes no
            // separate write fee; a disjoint write bucket is a cache miss.
            cache_write_per_million: 0.22,
            output_per_million: 0.66,
        },
        (false, DeepseekPriceBand::Peak) => Rates {
            input_per_million: 0.44,
            cache_read_per_million: 0.014,
            cache_write_per_million: 0.44,
            output_per_million: 1.32,
        },
        (true, DeepseekPriceBand::OffPeak) => Rates {
            input_per_million: 0.66,
            cache_read_per_million: 0.022,
            cache_write_per_million: 0.66,
            output_per_million: 1.98,
        },
        (true, DeepseekPriceBand::Peak) => Rates {
            input_per_million: 1.32,
            cache_read_per_million: 0.044,
            cache_write_per_million: 1.32,
            output_per_million: 3.96,
        },
    };
    let pricing_as_of = match price_band {
        DeepseekPriceBand::Peak => "2026-09-02 (DeepSeek peak rate at UTC completion)",
        DeepseekPriceBand::OffPeak => "2026-09-02 (DeepSeek off-peak rate at UTC completion)",
    };
    Some(estimate(usage, rates, pricing_as_of))
}

fn priceable_tokens(usage: &Usage) -> u64 {
    usage
        .input_tokens
        .saturating_add(usage.cache_read_tokens)
        .saturating_add(usage.cache_write_tokens)
        .saturating_add(usage.output_tokens)
}

pub fn compact(usage: Option<&Usage>) -> String {
    let Some(u) = usage.filter(|u| !u.is_empty()) else {
        return "Token usage unavailable".to_string();
    };
    let mut parts = if u.total_tokens() > 0 {
        let mut tokens = vec![
            format!("{} tokens", u.total_tokens()),
            format!("in {}", u.input_tokens),
        ];
        if u.cache_read_tokens > 0 {
            tokens.push(format!("cached {}", u.cache_read_tokens));
        }
        if u.cache_write_tokens > 0 {
            tokens.push(format!("cache write {}", u.cache_write_tokens));
        }
        tokens.push(format!("out {}", u.output_tokens));
        tokens
    } else {
        vec!["Token usage unavailable".to_string()]
    };
    if let Some(cost) = &u.cost {
        let marker = match cost.kind {
            CostKind::ProviderReported => "$",
            CostKind::ListPriceEstimate => "API list-price estimate ≈$",
        };
        parts.push(format!("{marker}{:.6}", cost.amount_usd));
    }
    parts.join(" · ")
}

/// Format the terminal hint for a run. A precheck skip is deliberately
/// distinguished from a harness that ran but did not expose usage data.
pub fn compact_run(status: crate::record::Status, usage: Option<&Usage>) -> String {
    if status == crate::record::Status::Skipped {
        "No model call (precheck skipped)".to_string()
    } else {
        compact(usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn total_uses_disjoint_buckets_and_not_reasoning_twice() {
        let u = Usage {
            input_tokens: 10,
            cache_read_tokens: 20,
            cache_write_tokens: 30,
            output_tokens: 40,
            reasoning_tokens: 35,
            ..Usage::default()
        };
        assert_eq!(u.total_tokens(), 100);
    }

    #[test]
    fn estimate_prices_each_bucket_once() {
        let u = Usage {
            input_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            cache_write_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Usage::default()
        };
        let c = estimate(
            &u,
            Rates {
                input_per_million: 2.0,
                cache_read_per_million: 0.2,
                cache_write_per_million: 2.5,
                output_per_million: 12.0,
            },
            "v1",
        );
        assert!((c.amount_usd - 16.7).abs() < f64::EPSILON);
        assert_eq!(c.kind, CostKind::ListPriceEstimate);
    }

    #[test]
    fn unknown_openai_model_has_no_guessed_price() {
        assert!(estimate_openai("future-model", &Usage::default()).is_none());
    }

    #[test]
    fn aggregate_above_the_per_request_threshold_is_not_guessed() {
        let u = Usage {
            input_tokens: 272_001,
            output_tokens: 1_000,
            ..Usage::default()
        };
        assert_eq!(estimate_openai("gpt-5.6-sol", &u), None);
    }

    #[test]
    fn anthropic_catalog_prices_cache_ttls_separately() {
        let u = Usage {
            input_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            cache_write_tokens: 2_000_000,
            output_tokens: 1_000_000,
            ..Usage::default()
        };
        let split = Some(AnthropicCacheWrite {
            five_minute_tokens: 1_000_000,
            one_hour_tokens: 1_000_000,
        });
        let cases = [
            ("claude-fable-5-1", 92.75),
            ("claude-fable-5", 93.5),
            ("claude-opus-5", 46.75),
            ("claude-sonnet-5", 18.7),
            ("claude-sonnet-4-6", 28.05),
            ("claude-haiku-4-5", 9.35),
        ];
        for (model, expected) in cases {
            let cost = estimate_anthropic(model, &u, split).expect(model);
            assert!((cost.amount_usd - expected).abs() < 1e-12, "{model}");
            assert_eq!(cost.kind, CostKind::ListPriceEstimate);
            assert_eq!(cost.pricing_as_of.as_deref(), Some("2026-09-02"));
        }
    }

    #[test]
    fn anthropic_uses_a_conservative_one_hour_rate_without_a_ttl_split() {
        let u = Usage {
            cache_write_tokens: 1_000_000,
            ..Usage::default()
        };
        let cost = estimate_anthropic("claude-sonnet-5", &u, None).unwrap();
        assert_eq!(cost.amount_usd, 4.0);
        assert_eq!(
            cost.pricing_as_of.as_deref(),
            Some("2026-09-02 (cache TTL unavailable; conservative 1h write rate)")
        );
        assert_eq!(
            estimate_anthropic(
                "claude-sonnet-5",
                &u,
                Some(AnthropicCacheWrite {
                    five_minute_tokens: 1,
                    one_hour_tokens: 2,
                })
            ),
            None,
            "contradictory telemetry must not be priced"
        );
    }

    #[test]
    fn anthropic_dynamic_aliases_and_unknown_models_are_not_guessed() {
        let u = Usage {
            input_tokens: 1,
            ..Usage::default()
        };
        for model in [
            "sonnet",
            "opusplan",
            "anthropic.claude-sonnet-4-5",
            "claude-future-model",
        ] {
            assert_eq!(estimate_anthropic(model, &u, None), None, "{model}");
        }
    }

    #[test]
    fn deepseek_uses_off_peak_and_peak_catalog_rates() {
        let u = Usage {
            input_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            cache_write_tokens: 1_000_000,
            output_tokens: 1_000_000,
            reasoning_tokens: 900_000,
            reported_total_tokens: 7,
            ..Usage::default()
        };
        let off_peak = Utc.with_ymd_and_hms(2026, 9, 2, 0, 59, 59).unwrap();
        let peak = Utc.with_ymd_and_hms(2026, 9, 2, 1, 0, 0).unwrap();
        assert_eq!(
            estimate_deepseek("deepseek-v4-flash", &u, off_peak)
                .unwrap()
                .amount_usd,
            1.107
        );
        assert_eq!(
            estimate_deepseek("deepseek-v4-flash-vision-exp", &u, peak)
                .unwrap()
                .amount_usd,
            2.214
        );
        assert_eq!(
            estimate_deepseek("deepseek-v4-pro", &u, off_peak)
                .unwrap()
                .amount_usd,
            3.322
        );
        assert_eq!(
            estimate_deepseek("deepseek-v4-pro-0813", &u, peak)
                .unwrap()
                .amount_usd,
            6.644
        );
    }

    #[test]
    fn deepseek_peak_boundaries_are_half_open_and_weekdays_only() {
        let at = |day, hour, minute, second| {
            Utc.with_ymd_and_hms(2026, 9, day, hour, minute, second)
                .unwrap()
        };
        for time in [
            at(2, 1, 0, 0),
            at(2, 3, 59, 59),
            at(2, 6, 0, 0),
            at(2, 9, 59, 59),
        ] {
            assert_eq!(deepseek_price_band(time), DeepseekPriceBand::Peak);
        }
        for time in [
            at(2, 0, 59, 59),
            at(2, 4, 0, 0),
            at(2, 10, 0, 0),
            at(5, 1, 0, 0),
        ] {
            assert_eq!(deepseek_price_band(time), DeepseekPriceBand::OffPeak);
        }
    }

    #[test]
    fn zero_buckets_and_unknown_deepseek_models_are_not_priced_from_total() {
        let at = Utc.with_ymd_and_hms(2026, 9, 2, 1, 0, 0).unwrap();
        let only_total = Usage {
            reported_total_tokens: 100,
            ..Usage::default()
        };
        assert_eq!(estimate_deepseek("deepseek-v4-pro", &only_total, at), None);
        let measured = Usage {
            input_tokens: 100,
            ..Usage::default()
        };
        for model in ["deepseek-chat", "deepseek-reasoner", "future-model"] {
            assert_eq!(estimate_deepseek(model, &measured, at), None, "{model}");
        }
    }

    #[test]
    fn provider_reported_cost_is_not_marked_estimated() {
        let u = Usage {
            cost: Some(Cost {
                amount_usd: 0.003,
                kind: CostKind::ProviderReported,
                pricing_as_of: None,
            }),
            ..Usage::default()
        };
        assert!(compact(Some(&u)).contains("$0.003000"));
        assert!(!compact(Some(&u)).contains("≈"));
    }

    #[test]
    fn estimated_cost_is_explicitly_named_in_the_terminal_tip() {
        let u = Usage {
            cost: Some(Cost {
                amount_usd: 0.003,
                kind: CostKind::ListPriceEstimate,
                pricing_as_of: Some("2026-09-02".into()),
            }),
            ..Usage::default()
        };
        assert_eq!(
            compact(Some(&u)),
            "Token usage unavailable · API list-price estimate ≈$0.003000"
        );
    }

    #[test]
    fn a_cost_without_tokens_does_not_claim_zero_usage() {
        let u = Usage {
            cost: Some(Cost {
                amount_usd: 0.003,
                kind: CostKind::ProviderReported,
                pricing_as_of: None,
            }),
            ..Usage::default()
        };
        assert_eq!(compact(Some(&u)), "Token usage unavailable · $0.003000");
    }

    #[test]
    fn skipped_run_is_not_reported_as_missing_usage() {
        assert_eq!(
            compact_run(crate::record::Status::Skipped, None),
            "No model call (precheck skipped)"
        );
    }
}
