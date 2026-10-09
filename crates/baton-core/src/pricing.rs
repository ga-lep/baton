//! Price table (USD per million tokens), context windows and cost estimation.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Context window assumed when the config names none for a model.
pub const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;

/// Prices in USD per million tokens, plus an optional context window.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    /// Input tokens.
    pub input: f64,
    /// Output tokens.
    pub output: f64,
    /// Cache-read tokens.
    pub cache_read: f64,
    /// Cache-write tokens.
    pub cache_write: f64,
    /// Context window in tokens (default [`DEFAULT_CONTEXT_WINDOW`]).
    pub context_window: Option<u64>,
}

/// The `[pricing]` table: a default price plus overrides in
/// `[pricing.models."<model id prefix>"]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    /// Fallback price.
    pub default: Option<Price>,
    /// Per-model overrides keyed by model id prefix.
    #[serde(default)]
    pub models: BTreeMap<String, Price>,
}

/// Token counts to price.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Cache-write (creation) tokens.
    pub cache_write: u64,
}

impl Pricing {
    /// The price entry for `model`: the longest matching prefix, else `default`.
    pub fn lookup(&self, model: Option<&str>) -> Option<&Price> {
        model
            .and_then(|m| {
                self.models
                    .iter()
                    .filter(|(prefix, _)| m.starts_with(prefix.as_str()))
                    .max_by_key(|(prefix, _)| prefix.len())
                    .map(|(_, price)| price)
            })
            .or(self.default.as_ref())
    }

    /// Context window of `model` in tokens (never 0).
    pub fn context_window(&self, model: Option<&str>) -> u64 {
        self.lookup(model)
            .and_then(|p| p.context_window)
            .filter(|w| *w > 0)
            .unwrap_or(DEFAULT_CONTEXT_WINDOW)
    }
}

/// Estimated cost in USD of `tokens` on `model`, or `None` with no matching price.
pub fn estimate(pricing: &Pricing, model: Option<&str>, tokens: &Tokens) -> Option<f64> {
    let p = pricing.lookup(model)?;
    // u64 -> f64 can round above 2^53 tokens; irrelevant for an estimate.
    let mtok = |n: u64, rate: f64| n as f64 / 1_000_000.0 * rate;
    Some(
        mtok(tokens.input, p.input)
            + mtok(tokens.output, p.output)
            + mtok(tokens.cache_read, p.cache_read)
            + mtok(tokens.cache_write, p.cache_write),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(input: f64, window: Option<u64>) -> Price {
        Price {
            input,
            output: 5.0,
            cache_read: 0.5,
            cache_write: 2.0,
            context_window: window,
        }
    }

    fn table() -> Pricing {
        Pricing {
            default: Some(price(3.0, None)),
            models: BTreeMap::from([
                ("claude-opus".to_owned(), price(15.0, Some(1_000_000))),
                ("claude-opus-5".to_owned(), price(10.0, None)),
            ]),
        }
    }

    #[test]
    fn longest_prefix_wins_and_default_is_the_fallback() {
        let t = table();
        assert_eq!(
            t.lookup(Some("claude-opus-5-5")).map(|p| p.input),
            Some(10.0)
        );
        assert_eq!(t.lookup(Some("claude-opus-4")).map(|p| p.input), Some(15.0));
        assert_eq!(t.lookup(Some("claude-haiku")).map(|p| p.input), Some(3.0));
        assert_eq!(t.lookup(None).map(|p| p.input), Some(3.0));
        assert_eq!(Pricing::default().lookup(Some("x")), None);
    }

    #[test]
    fn context_window_defaults_to_200k() {
        let t = table();
        assert_eq!(t.context_window(Some("claude-opus-4")), 1_000_000);
        assert_eq!(
            t.context_window(Some("claude-opus-5-5")),
            DEFAULT_CONTEXT_WINDOW
        );
        assert_eq!(t.context_window(Some("other")), DEFAULT_CONTEXT_WINDOW);
        assert_eq!(
            Pricing::default().context_window(None),
            DEFAULT_CONTEXT_WINDOW
        );
        let zero = Pricing {
            default: Some(price(1.0, Some(0))),
            models: BTreeMap::new(),
        };
        assert_eq!(zero.context_window(None), DEFAULT_CONTEXT_WINDOW);
    }

    #[test]
    fn cost_is_tokens_times_rate_per_mtok() {
        let t = Tokens {
            input: 2_000_000,
            output: 1_000_000,
            cache_read: 4_000_000,
            cache_write: 500_000,
        };
        // 2*3 + 1*5 + 4*0.5 + 0.5*2
        let c = estimate(&table(), Some("claude-haiku"), &t).expect("priced");
        assert!((c - 14.0).abs() < 1e-9, "{c}");
        assert_eq!(estimate(&Pricing::default(), None, &t), None);
    }

    #[test]
    fn parses_from_toml() {
        let p: Pricing = toml::from_str(
            "default = { input = 3.0, output = 15.0, cache_read = 0.3, cache_write = 3.75 }\n\
             [models.\"claude-opus\"]\ninput = 15.0\noutput = 75.0\ncache_read = 1.5\n\
             cache_write = 18.75\ncontext_window = 1000000\n",
        )
        .expect("toml");
        assert_eq!(p.context_window(Some("claude-opus-5")), 1_000_000);
        assert_eq!(p.default.map(|d| d.input), Some(3.0));
    }
}
