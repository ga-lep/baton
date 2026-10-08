//! Terminal byte-stream inspection: mode tracking and query replies.

pub mod responder;
pub mod scanner;

pub use scanner::{MouseMode, Scanner, TermModes};
