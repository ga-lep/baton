//! Tolerant aggregation of token usage from a Claude Code transcript (JSONL).
//!
//! The transcript format is internal to Claude Code and may change, so every
//! line that does not look as expected is ignored.

use crate::pricing::{Pricing, Tokens, estimate};
use baton_proto::Usage;
use serde_json::Value;
use std::collections::{HashSet, VecDeque};

/// Longest model id kept for display.
pub const MAX_MODEL_LEN: usize = 64;
/// How many message ids are remembered for de-duplication.
const MAX_SEEN_IDS: usize = 100_000;

/// A model id that is safe to show in a terminal: ASCII graphic characters
/// only (so no control or format characters), at most [`MAX_MODEL_LEN`] long.
/// `None` if nothing is left.
pub fn sanitize_model(raw: &str) -> Option<String> {
    let clean: String = raw
        .chars()
        .filter(char::is_ascii_graphic)
        .take(MAX_MODEL_LEN)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

/// Running totals over the lines of one transcript.
#[derive(Debug, Default)]
pub struct UsageAccumulator {
    tokens: Tokens,
    seen: HashSet<String>,
    order: VecDeque<String>,
    context_tokens: Option<u64>,
    model: Option<String>,
}

impl UsageAccumulator {
    /// An empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one transcript line. Anything that is not a well-formed
    /// `assistant` entry with usage is ignored.
    pub fn feed_line(&mut self, line: &str) {
        let Ok(serde_json::Value::Object(entry)) = serde_json::from_str(line) else {
            return;
        };
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(message) = entry.get("message") else {
            return;
        };
        let Some(usage) = message.get("usage").and_then(Value::as_object) else {
            return;
        };
        const FIELDS: [&str; 4] = [
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ];
        if !FIELDS
            .iter()
            .any(|f| usage.get(*f).is_some_and(Value::is_u64))
        {
            return; // not the usage shape we know
        }
        let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
        let t = Tokens {
            input: field("input_tokens"),
            output: field("output_tokens"),
            cache_read: field("cache_read_input_tokens"),
            cache_write: field("cache_creation_input_tokens"),
        };
        // Claude writes placeholder entries (`<synthetic>`) for local errors.
        if let Some(model) = message
            .get("model")
            .and_then(Value::as_str)
            .filter(|m| !m.starts_with('<'))
            .and_then(sanitize_model)
        {
            self.model = Some(model);
        }
        let sidechain = entry
            .get("isSidechain")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !sidechain {
            self.context_tokens = Some(
                t.input
                    .saturating_add(t.cache_read)
                    .saturating_add(t.cache_write),
            );
        }
        if !message
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| self.remember(id))
        {
            return; // another entry of a message already counted
        }
        self.tokens = Tokens {
            input: self.tokens.input.saturating_add(t.input),
            output: self.tokens.output.saturating_add(t.output),
            cache_read: self.tokens.cache_read.saturating_add(t.cache_read),
            cache_write: self.tokens.cache_write.saturating_add(t.cache_write),
        };
    }

    /// Records `id`; `false` if it was already known.
    fn remember(&mut self, id: &str) -> bool {
        if !self.seen.insert(id.to_owned()) {
            return false;
        }
        self.order.push_back(id.to_owned());
        if self.order.len() > MAX_SEEN_IDS
            && let Some(old) = self.order.pop_front()
        {
            self.seen.remove(&old);
        }
        true
    }

    /// Summed tokens over all distinct API messages (main and sidechain).
    pub fn tokens(&self) -> Tokens {
        self.tokens
    }

    /// Prompt tokens of the latest non-sidechain assistant message.
    pub fn context_tokens(&self) -> Option<u64> {
        self.context_tokens
    }

    /// Model of the latest assistant entry.
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// The wire summary: totals, context % (0-100) and estimated cost.
    pub fn snapshot(&self, pricing: &Pricing) -> Usage {
        let model = self.model.as_deref();
        let window = pricing.context_window(model);
        Usage {
            input: self.tokens.input,
            output: self.tokens.output,
            cache_read: self.tokens.cache_read,
            cache_write: self.tokens.cache_write,
            context_pct: self
                .context_tokens
                .map(|c| (c as f64 / window as f64 * 100.0) as f32),
            cost_usd: estimate(pricing, model, &self.tokens),
            model: self.model.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Price;
    use std::collections::BTreeMap;

    const SAMPLE: &str = include_str!("../tests/fixtures/transcript_sample.jsonl");

    fn fed(text: &str) -> UsageAccumulator {
        let mut a = UsageAccumulator::new();
        text.lines().for_each(|l| a.feed_line(l));
        a
    }

    fn asst(id: &str, side: bool, i: u64, o: u64, cr: u64, cw: u64) -> String {
        format!(
            r#"{{"type":"assistant","isSidechain":{side},"message":{{"id":"{id}","model":"m","usage":{{"input_tokens":{i},"output_tokens":{o},"cache_read_input_tokens":{cr},"cache_creation_input_tokens":{cw}}}}}}}"#
        )
    }

    #[test]
    fn dedupes_by_message_id_and_sums_main_and_sidechain() {
        let a = fed(SAMPLE);
        assert_eq!(
            a.tokens(),
            Tokens {
                input: 10 + 1 + 20,
                output: 5 + 2 + 7,
                cache_read: 100 + 3 + 200,
                cache_write: 20 + 4 + 30,
            }
        );
    }

    #[test]
    fn context_uses_the_latest_non_sidechain_message_and_model_the_latest_entry() {
        let a = fed(SAMPLE);
        assert_eq!(a.context_tokens(), Some(20 + 200 + 30));
        assert_eq!(a.model(), Some("claude-opus-5-5"));
        // A sidechain entry later in the file moves the model but not the context.
        let mut b = fed(SAMPLE);
        b.feed_line(&asst("later", true, 1, 1, 1, 1).replace("\"m\"", "\"side\""));
        assert_eq!(b.context_tokens(), Some(250));
        assert_eq!(b.model(), Some("side"));
    }

    #[test]
    fn garbage_and_foreign_lines_are_ignored() {
        let mut a = UsageAccumulator::new();
        for l in [
            "",
            "not json",
            "[1,2]",
            r#"{"type":"user","message":{"usage":{"input_tokens":9}}}"#,
            r#"{"type":"assistant"}"#,
            r#"{"type":"assistant","message":{"id":"x","usage":"nope"}}"#,
            r#"{"type":"assistant","message":{"id":"x","usage":{"input_tokens":-3,"output_tokens":"a"}}}"#,
        ] {
            a.feed_line(l);
        }
        assert_eq!(a.tokens(), Tokens::default());
        assert_eq!(a.context_tokens(), None);
    }

    #[test]
    fn entries_without_an_id_are_counted_each_time() {
        let mut a = UsageAccumulator::new();
        let line = r#"{"type":"assistant","message":{"usage":{"input_tokens":4}}}"#;
        a.feed_line(line);
        a.feed_line(line);
        assert_eq!(a.tokens().input, 8);
    }

    #[test]
    fn sums_saturate_and_the_id_memory_is_bounded() {
        let mut a = UsageAccumulator::new();
        a.feed_line(&asst("a", false, u64::MAX, 0, 0, 0));
        a.feed_line(&asst("b", false, 5, 0, 0, 0));
        assert_eq!(a.tokens().input, u64::MAX);
        let mut a = UsageAccumulator::new();
        for i in 0..(MAX_SEEN_IDS + 10) {
            a.feed_line(&asst(&format!("id{i}"), false, 1, 0, 0, 0));
        }
        assert!(a.seen.len() <= MAX_SEEN_IDS && a.order.len() <= MAX_SEEN_IDS);
    }

    #[test]
    fn snapshot_computes_context_percent_and_cost() {
        let a = fed(SAMPLE);
        let pricing = Pricing {
            default: Some(Price {
                input: 3.0,
                output: 15.0,
                cache_read: 0.3,
                cache_write: 3.75,
                context_window: None,
            }),
            models: BTreeMap::new(),
        };
        let u = a.snapshot(&pricing);
        assert_eq!(
            (u.input, u.output, u.cache_read, u.cache_write),
            (31, 14, 303, 54)
        );
        let pct = f64::from(u.context_pct.expect("pct"));
        assert!((pct - 250.0 / 200_000.0 * 100.0).abs() < 1e-5, "{pct}");
        assert_eq!(
            u.cost_usd,
            estimate(&pricing, Some("claude-opus-5-5"), &a.tokens())
        );
        assert!(u.cost_usd.is_some());
        assert_eq!(u.model.as_deref(), Some("claude-opus-5-5"));
        let none = a.snapshot(&Pricing::default());
        assert_eq!(none.cost_usd, None);
    }

    #[test]
    fn no_assistant_message_means_no_context_percent() {
        let u = UsageAccumulator::new().snapshot(&Pricing::default());
        assert_eq!(u.context_pct, None);
        assert_eq!(u.model, None);
    }

    #[test]
    fn model_ids_are_stripped_and_capped() {
        assert_eq!(
            sanitize_model("claude-opus-5-5"),
            Some("claude-opus-5-5".into())
        );
        assert_eq!(
            sanitize_model("cl\u{1b}[31maude\u{202e}-\u{200b}x\n").as_deref(),
            Some("cl[31maude-x")
        );
        assert_eq!(sanitize_model("\u{1b}\u{7}"), None);
        assert_eq!(
            sanitize_model(&"a".repeat(500)).map(|m| m.len()),
            Some(MAX_MODEL_LEN)
        );
        let line = asst("z", false, 1, 1, 1, 1).replace("\"m\"", "\"x\\u001b[2Jy\"");
        assert_eq!(fed(&line).model(), Some("x[2Jy"));
    }
}
