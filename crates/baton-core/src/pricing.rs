//! Price table types (USD per million tokens). Cost computation lives elsewhere.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Prices in USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Price {
    /// Input tokens.
    pub input: f64,
    /// Output tokens.
    pub output: f64,
    /// Cache-read tokens.
    pub cache_read: f64,
    /// Cache-write tokens.
    pub cache_write: f64,
}

/// The `[pricing]` table: a default price plus overrides keyed by model id prefix.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Pricing {
    /// Fallback price.
    pub default: Option<Price>,
    /// Per-model overrides keyed by model id prefix.
    #[serde(flatten)]
    pub models: BTreeMap<String, Price>,
}
