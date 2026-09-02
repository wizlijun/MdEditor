//! Provider-neutral token accounting stored with every completed run.
//!
//! Adapters normalize provider payloads into disjoint input buckets before they
//! get here.  That matters because some APIs report cached tokens as a subset of
//! `input_tokens`, while others report them beside ordinary input tokens.
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
            CostKind::ListPriceEstimate => "≈$",
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
