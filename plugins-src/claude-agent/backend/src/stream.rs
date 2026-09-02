//! Line-by-line parsing of `--output-format stream-json --verbose`. Only four
//! kinds of event matter to us; everything else (including non-JSON noise) is
//! dropped rather than surfaced.
//!
//! The `Event` shape itself is the wire contract shared with the window and with
//! deepseek-agent, so it lives in `agent-run-core`; only the claude-specific
//! PARSING is here.
pub use agent_run_core::event::{Event, RunResult};
use agent_run_core::usage::{estimate_anthropic, AnthropicCacheWrite, Cost, CostKind, Usage};

/// Parse one line. `None` means the line produces no event.
pub fn parse_line(line: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("type")?.as_str()? {
        "system" => Some(Event::System {
            subtype: v
                .get("subtype")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "assistant" => {
            let blocks = v.pointer("/message/content")?.as_array()?;
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                        return Some(Event::ToolUse {
                            name: name.to_string(),
                            brief: tool_brief(b),
                        });
                    }
                    Some("text") => {
                        let t = b.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if !t.is_empty() {
                            return Some(Event::Text {
                                text: t.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        "result" => Some(Event::Result(RunResult {
            is_error: v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false),
            result: v
                .get("result")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            session_id: v
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(str::to_string),
            num_turns: v.get("num_turns").and_then(|n| n.as_u64()),
            usage: result_usage(&v),
        })),
        _ => None,
    }
}

/// Claude Code's terminal result carries the Anthropic usage buckets already
/// split into ordinary input, cache read and cache creation. Keep those buckets
/// disjoint: adding either cache count back into `input_tokens` would bill the
/// same token twice in the shared estimator.
fn result_usage(result: &serde_json::Value) -> Option<Usage> {
    let raw = result.get("usage").and_then(|v| v.as_object());
    let reported_cost = result
        .get("total_cost_usd")
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .or_else(|| model_usage_cost(result))
        .map(provider_cost);
    if raw.is_none() && reported_cost.is_none() {
        return None;
    }
    let tokens = |key: &str| {
        raw.and_then(|u| u.get(key))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    };
    let mut usage = Usage {
        model: result_model(result),
        input_tokens: tokens("input_tokens"),
        cache_read_tokens: tokens("cache_read_input_tokens"),
        cache_write_tokens: tokens("cache_creation_input_tokens"),
        output_tokens: tokens("output_tokens"),
        cost: reported_cost,
        ..Usage::default()
    };
    if usage.cost.is_none() && !unsupported_price_modifier(raw) {
        if let (Some(model), Ok(cache_write)) = (
            usage.model.as_deref(),
            anthropic_cache_write(raw, usage.cache_write_tokens),
        ) {
            usage.cost = estimate_anthropic(model, &usage, cache_write);
        }
    }
    (!usage.is_empty()).then_some(usage)
}

fn provider_cost(amount_usd: f64) -> Cost {
    Cost {
        amount_usd,
        kind: CostKind::ProviderReported,
        pricing_as_of: None,
    }
}

/// Newer multi-model result frames may omit the top-level total while still
/// reporting one cost per model. Only sum a complete, non-empty map; a partial
/// sum would look authoritative while understating the provider's amount.
fn model_usage_cost(result: &serde_json::Value) -> Option<f64> {
    let models = result.get("modelUsage")?.as_object()?;
    if models.is_empty() {
        return None;
    }
    models
        .values()
        .try_fold(0.0, |sum, row| {
            let amount = row
                .get("costUSD")
                .or_else(|| row.get("cost_usd"))?
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)?;
            Some(sum + amount)
        })
        .filter(|sum| sum.is_finite())
}

fn anthropic_cache_write(
    raw: Option<&serde_json::Map<String, serde_json::Value>>,
    aggregate: u64,
) -> Result<Option<AnthropicCacheWrite>, ()> {
    let Some(cache) = raw
        .and_then(|usage| usage.get("cache_creation"))
        .and_then(|value| value.as_object())
    else {
        return Ok(None);
    };
    let five_minute_tokens = cache
        .get("ephemeral_5m_input_tokens")
        .and_then(|value| value.as_u64())
        .ok_or(())?;
    let one_hour_tokens = cache
        .get("ephemeral_1h_input_tokens")
        .and_then(|value| value.as_u64())
        .ok_or(())?;
    let split = AnthropicCacheWrite {
        five_minute_tokens,
        one_hour_tokens,
    };
    (five_minute_tokens.checked_add(one_hour_tokens) == Some(aggregate))
        .then_some(Some(split))
        .ok_or(())
}

/// These modifiers have separate official prices which cannot be reconstructed
/// from the four aggregate token buckets alone.
fn unsupported_price_modifier(raw: Option<&serde_json::Map<String, serde_json::Value>>) -> bool {
    let speed_is_fast = raw
        .and_then(|usage| usage.get("speed"))
        .and_then(|value| value.as_str())
        .is_some_and(|speed| speed.eq_ignore_ascii_case("fast"));
    let geo_is_us = raw
        .and_then(|usage| usage.get("inference_geo"))
        .and_then(|value| value.as_str())
        .is_some_and(|geo| geo.eq_ignore_ascii_case("us"));
    speed_is_fast || geo_is_us
}

/// Newer Claude Code results expose `model`; older/multi-model results expose a
/// `modelUsage` map. A single map key is an exact answer. More than one is not:
/// the aggregate usage must not be attributed to an arbitrary model.
fn result_model(result: &serde_json::Value) -> Option<String> {
    if let Some(models) = result
        .get("modelUsage")
        .and_then(|value| value.as_object())
        .filter(|models| !models.is_empty())
    {
        return (models.len() == 1)
            .then(|| models.keys().next().cloned())
            .flatten();
    }
    if let Some(model) = result
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(model.to_string());
    }
    None
}

/// One-line summary of a tool call: file path first, then command, then pattern.
fn tool_brief(block: &serde_json::Value) -> String {
    let i = match block.get("input") {
        Some(i) => i,
        None => return String::new(),
    };
    for k in ["file_path", "path", "command", "pattern", "url"] {
        if let Some(s) = i.get(k).and_then(|v| v.as_str()) {
            return s.chars().take(120).collect();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_result_line() {
        let l = r#"{"type":"result","subtype":"success","result":"done","session_id":"s1","num_turns":12,"is_error":false}"#;
        assert_eq!(
            parse_line(l),
            Some(Event::Result(RunResult {
                is_error: false,
                result: "done".into(),
                session_id: Some("s1".into()),
                num_turns: Some(12),
                usage: None,
            }))
        );
    }

    #[test]
    fn parses_disjoint_usage_provider_cost_and_explicit_model() {
        let line = r#"{"type":"result","result":"done","is_error":false,"model":"claude-sonnet-4-5","usage":{"input_tokens":11,"cache_read_input_tokens":22,"cache_creation_input_tokens":33,"output_tokens":44},"total_cost_usd":0.0123}"#;
        let Some(Event::Result(result)) = parse_line(line) else {
            panic!("expected result")
        };
        assert_eq!(
            result.usage,
            Some(Usage {
                model: Some("claude-sonnet-4-5".into()),
                input_tokens: 11,
                cache_read_tokens: 22,
                cache_write_tokens: 33,
                output_tokens: 44,
                cost: Some(Cost {
                    amount_usd: 0.0123,
                    kind: CostKind::ProviderReported,
                    pricing_as_of: None,
                }),
                ..Usage::default()
            })
        );
    }

    #[test]
    fn estimates_anthropic_list_price_when_provider_cost_is_absent() {
        let line = r#"{"type":"result","result":"done","model":"claude-sonnet-5","usage":{"input_tokens":1000000,"cache_read_input_tokens":1000000,"cache_creation_input_tokens":2000000,"output_tokens":1000000,"cache_creation":{"ephemeral_5m_input_tokens":1000000,"ephemeral_1h_input_tokens":1000000}}}"#;
        let Some(Event::Result(result)) = parse_line(line) else {
            panic!("expected result")
        };
        let cost = result.usage.unwrap().cost.expect("official estimate");
        assert_eq!(cost.kind, CostKind::ListPriceEstimate);
        assert!((cost.amount_usd - 18.7).abs() < 1e-12);
        assert_eq!(cost.pricing_as_of.as_deref(), Some("2026-09-02"));
    }

    #[test]
    fn provider_cost_wins_over_a_different_local_estimate() {
        let line = r#"{"type":"result","result":"done","model":"claude-sonnet-5","usage":{"input_tokens":1000000},"total_cost_usd":0.0}"#;
        let Some(Event::Result(result)) = parse_line(line) else {
            panic!("expected result")
        };
        let cost = result.usage.unwrap().cost.expect("reported cost");
        assert_eq!(cost.kind, CostKind::ProviderReported);
        assert_eq!(cost.amount_usd, 0.0);
    }

    #[test]
    fn complete_model_usage_costs_are_summed_but_partial_maps_are_not() {
        let complete = r#"{"type":"result","result":"done","usage":{"input_tokens":1},"modelUsage":{"claude-sonnet-5":{"costUSD":0.1},"claude-opus-5":{"costUSD":0.2}}}"#;
        let Some(Event::Result(result)) = parse_line(complete) else {
            panic!("expected result")
        };
        let usage = result.usage.unwrap();
        assert_eq!(usage.model, None, "multi-model usage has no single model");
        assert_eq!(usage.cost.unwrap().kind, CostKind::ProviderReported);
        assert!(
            (model_usage_cost(&serde_json::from_str(complete).unwrap()).unwrap() - 0.3).abs()
                < 1e-12
        );

        let partial = r#"{"type":"result","result":"done","model":"claude-sonnet-5","usage":{"input_tokens":1},"modelUsage":{"claude-sonnet-5":{"costUSD":0.1},"claude-opus-5":{}}}"#;
        let Some(Event::Result(result)) = parse_line(partial) else {
            panic!("expected result")
        };
        let usage = result.usage.unwrap();
        assert_eq!(usage.model, None);
        assert_eq!(usage.cost, None);
    }

    #[test]
    fn contradictory_cache_details_and_unsupported_modifiers_are_not_guessed() {
        for usage in [
            r#"{"input_tokens":1,"cache_creation_input_tokens":3,"cache_creation":{"ephemeral_5m_input_tokens":1,"ephemeral_1h_input_tokens":1}}"#,
            r#"{"input_tokens":1,"speed":"fast"}"#,
            r#"{"input_tokens":1,"inference_geo":"us"}"#,
        ] {
            let line = format!(
                r#"{{"type":"result","result":"done","model":"claude-sonnet-5","usage":{usage}}}"#
            );
            let Some(Event::Result(result)) = parse_line(&line) else {
                panic!("expected result")
            };
            assert_eq!(result.usage.unwrap().cost, None, "{usage}");
        }
    }

    #[test]
    fn takes_a_model_from_a_single_model_usage_key_only() {
        let one = r#"{"type":"result","result":"done","usage":{"input_tokens":1},"modelUsage":{"claude-opus-4-1":{"inputTokens":1}}}"#;
        let Some(Event::Result(result)) = parse_line(one) else {
            panic!("expected result")
        };
        assert_eq!(
            result.usage.unwrap().model.as_deref(),
            Some("claude-opus-4-1")
        );

        let many = r#"{"type":"result","result":"done","usage":{"input_tokens":1},"modelUsage":{"claude-a":{},"claude-b":{}}}"#;
        let Some(Event::Result(result)) = parse_line(many) else {
            panic!("expected result")
        };
        assert_eq!(result.usage.unwrap().model, None);
    }

    #[test]
    fn no_usage_or_cost_keeps_the_optional_field_absent() {
        let Some(Event::Result(result)) = parse_line(r#"{"type":"result","result":"done"}"#) else {
            panic!("expected result")
        };
        assert_eq!(result.usage, None);

        let Some(Event::Result(result)) =
            parse_line(r#"{"type":"result","result":"done","usage":{}}"#)
        else {
            panic!("expected result")
        };
        assert_eq!(result.usage, None, "an empty object is not measured zero");
    }

    #[test]
    fn parses_a_tool_use_with_a_file_brief() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}"#;
        assert_eq!(
            parse_line(l),
            Some(Event::ToolUse {
                name: "Read".into(),
                brief: "src/a.rs".into()
            })
        );
    }

    #[test]
    fn parses_assistant_text() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        assert_eq!(
            parse_line(l),
            Some(Event::Text {
                text: "hello".into()
            })
        );
    }

    #[test]
    fn drops_noise_lines() {
        assert_eq!(parse_line("not json at all"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line(r#"{"type":"user","message":{}}"#), None);
        assert_eq!(parse_line(r#"{"no_type":1}"#), None);
    }

    #[test]
    fn treats_is_error_true_as_a_failed_result() {
        let l =
            r#"{"type":"result","subtype":"error_max_turns","result":"hit limit","is_error":true}"#;
        match parse_line(l) {
            Some(Event::Result(r)) => {
                assert!(r.is_error);
                assert_eq!(r.result, "hit limit");
            }
            other => panic!("expected a result event, got {other:?}"),
        }
    }

    #[test]
    fn truncates_an_absurdly_long_tool_brief() {
        let long = "x".repeat(500);
        let l = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"{long}"}}}}]}}}}"#
        );
        match parse_line(&l) {
            Some(Event::ToolUse { brief, .. }) => assert_eq!(brief.chars().count(), 120),
            other => panic!("expected a tool_use event, got {other:?}"),
        }
    }
}
