//! Terminal input encoding and screen emulation for the embedded application.

pub mod encode;
pub mod screen;
pub mod vt100_screen;

#[cfg(test)]
mod encode_tests;
