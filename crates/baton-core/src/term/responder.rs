//! Replies to terminal queries that the daemon must answer itself.

/// Reply to Primary Device Attributes (`CSI c`).
pub const DA1_REPLY: &[u8] = b"\x1b[?62;22c";
/// Reply to DSR 5 (`CSI 5n`): terminal OK.
pub const DSR_OK_REPLY: &[u8] = b"\x1b[0n";

/// Reply to DSR 6 (`CSI 6n`) for a 1-based cursor position.
pub fn cursor_report(row: u16, col: u16) -> Vec<u8> {
    format!("\x1b[{row};{col}R").into_bytes()
}

/// Reply for a complete CSI sequence, or `None` if it is not answered.
///
/// `private` is the leading private marker (`?`, `>`, ...) if any,
/// `params` the raw parameter bytes and `intermediates` the 0x20-0x2F bytes.
pub fn reply_for_csi(
    private: Option<u8>,
    params: &[u8],
    intermediates: &[u8],
    final_byte: u8,
    cursor: (u16, u16),
) -> Option<Vec<u8>> {
    if private.is_some() || !intermediates.is_empty() {
        return None;
    }
    match (final_byte, params) {
        (b'c', b"" | b"0") => Some(DA1_REPLY.to_vec()),
        (b'n', b"5") => Some(DSR_OK_REPLY.to_vec()),
        (b'n', b"6") => Some(cursor_report(cursor.0, cursor.1)),
        _ => None,
    }
}
