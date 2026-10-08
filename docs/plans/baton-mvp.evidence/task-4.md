# Evidence — Task 4: Input encoder (term::encode)
Commit: ba704a4
Environment: `cargo test -p baton term::encode` on the local checkout (no server; internal module, exercised from outside in Task 6). Test file: crates/baton/src/term/encode_tests.rs.

## Test run (per-test output)
Status: PROVEN
```console
$ cargo test -p baton term::encode -- --nocapture --test-threads=1
test term::encode_tests::focus ... ok
test term::encode_tests::key_ignored_events ... ok
test term::encode_tests::key_table ... ok
test term::encode_tests::keypad_application_mode ... ok
test term::encode_tests::mouse_ignored ... ok
test term::encode_tests::mouse_wheel ... ok
test term::encode_tests::paste_table ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.00s
```
All 7 encoder tests pass.

## API: encode_key, encode_paste, encode_mouse, encode_focus
Status: PROVEN
```console
$ grep -n "pub fn" crates/baton/src/term/encode.rs
145:pub fn encode_key(ev: &KeyEvent, modes: &EncodeModes) -> Option<Vec<u8>> {
194:pub fn encode_paste(text: &str, modes: &EncodeModes) -> Vec<u8> {
215:pub fn encode_mouse(ev: &MouseEvent, origin: (u16, u16), modes: &EncodeModes) -> Option<Vec<u8>> {
240:pub fn encode_focus(focused: bool, modes: &EncodeModes) -> Option<Vec<u8>> {
```
Signatures match the plan.

## EncodeModes holds app_cursor, app_keypad, bracketed_paste and TermModes
Status: PROVEN
```console
$ grep -n "pub struct EncodeModes" -A8 crates/baton/src/term/encode.rs
10:pub struct EncodeModes {
11-    /// DECCKM: cursor keys send `SS3` instead of `CSI`.
12-    pub app_cursor: bool,
13-    /// DECKPAM: keypad keys send `SS3` sequences.
14-    pub app_keypad: bool,
15-    /// `?2004`: pasted text is bracketed.
16-    pub bracketed_paste: bool,
17-    /// Modes tracked from the application's output stream.
18-    pub term: TermModes,
```

## Table tests have at least 60 rows and cover the listed cases
Status: PROVEN
```console
$ sed -n '/fn key_table/,/assert!(rows/p' crates/baton/src/term/encode_tests.rs | grep -c '^        ('
92
```
92 rows in key_table, and the test asserts `rows.len() >= 60`. key_table passes. Representative rows (input, modifiers, modes, expected bytes) from the file:
```
(c('é'), NONE, plain(), "é".as_bytes()), (c('中'),...), (c('😀'),...)   UTF-8 bytes
(KeyCode::Enter, NONE, plain(), b"\r")      (KeyCode::Tab, ..., b"\t")
(KeyCode::Backspace, NONE, plain(), b"\x7f")   (KeyCode::Esc, ..., b"\x1b")
(c('a'), CTRL, plain(), b"\x01")  (c('z'), CTRL, plain(), b"\x1a")
(c('@'), CTRL, plain(), b"\x00")  (c(' '), CTRL, plain(), b"\x00")
(c('['), CTRL, plain(), b"\x1b")  (c(']'), CTRL, b"\x1d")  (c('^'),.. b"\x1e")  (c('_'),.. b"\x1f")
(KeyCode::Up, NONE, plain(), b"\x1b[A")
(KeyCode::Up, NONE, cursor(), b"\x1bOA")            DECCKM on
(KeyCode::Right, CTRL, plain(), b"\x1b[1;5C")
(KeyCode::Delete, CTRL, plain(), b"\x1b[3;5~")  (KeyCode::PageUp, SHIFT, ..., b"\x1b[5;2~")
(KeyCode::F(1), NONE, plain(), b"\x1bOP")  (KeyCode::F(12), NONE, plain(), b"\x1b[24~")
(c('x'), ALT, plain(), b"\x1bx")   (KeyCode::Enter, ALT, plain(), b"\x1b\r")
(KeyCode::BackTab, SHIFT, plain(), b"\x1b[Z")
```
Rows cover every category in the plan's table (printable/UTF-8, Enter, Tab, Backspace, Esc, Ctrl letters and punctuation, arrows normal/DECCKM/modified, nav keys, F1-F12, Alt-x, Alt-Enter, BackTab).

## Shift-Enter
Status: PROVEN
```
(KeyCode::Enter, SHIFT, plain(),   b"\x1b\r")        // no mok / kitty: meta-enter
(KeyCode::Enter, SHIFT, mok(1),    b"\x1b[27;2;13~")
(KeyCode::Enter, SHIFT, mok(2),    b"\x1b[27;2;13~")
(KeyCode::Enter, SHIFT, kitty(1),  b"\x1b[13;2u")
(KeyCode::Enter, SHIFT, kitty(5),  b"\x1b[13;2u")    // bit 1 set among others
(KeyCode::Enter, SHIFT, kitty(2),  b"\x1b\r")        // bit 1 not set
```
key_table passes with these rows. The plan's "M0 spike confirms which Claude accepts" is a spike matter, not checkable here.

## modifyOtherKeys = 2: Ctrl-<punct> and Ctrl-Enter use ESC[27;<mod>;<code>~
Status: PROVEN
```
(c('['), CTRL, mok(2), b"\x1b[27;5;91~")   (c(']'), CTRL, mok(2), b"\x1b[27;5;93~")
(c(' '), CTRL, mok(2), b"\x1b[27;5;32~")   (KeyCode::Enter, CTRL, mok(2), b"\x1b[27;5;13~")
(c('a'), CTRL, mok(2), b"\x01")   (KeyCode::Enter, CTRL, mok(1), b"\r")   // negative controls
```

## Paste
Status: PROVEN
```
encode_paste("hi", bracketed)                  == b"\x1b[200~hi\x1b[201~"
encode_paste("x\x1b[201~y", bracketed)         == b"\x1b[200~xy\x1b[201~"      // embedded end marker stripped
encode_paste("\x1b[2\x1b[201~01~", bracketed)  == b"\x1b[200~\x1b[201~"        // forged reassembly also stripped
encode_paste("a\nb\r\nc", plain())             == b"a\rb\rc"                   // bracketed off: newline -> \r
```
paste_table passes.

## Mouse wheel
Status: PROVEN
```
ScrollUp   (10,5) origin (0,0), SGR+Press  -> b"\x1b[<64;11;6M"
ScrollDown (10,5) origin (0,0)             -> b"\x1b[<65;11;6M"
ScrollUp   (10,5) origin (4,2)             -> b"\x1b[<64;7;4M"     // panel-relative
SGR on, mouse None / SGR off / default modes / outside origin / click / move -> None
```
mouse_wheel and mouse_ignored pass. The tests also show Ctrl+wheel yields code 80, an extra beyond the plan.

## Focus
Status: PROVEN
```
focus_reporting on:  encode_focus(true)  == b"\x1b[I"   encode_focus(false) == b"\x1b[O"
default modes:       encode_focus(true) == None          encode_focus(false) == None
```

## Key releases and ignored events return None
Status: PROVEN
```
Release 'a' -> None;  F(13) -> None;  CapsLock -> None   (Repeat 'a' -> Some(b"a"))
```
key_ignored_events passes.

## Notes
Also tested, though not in the criteria: application keypad mode (keypad_application_mode: `0` -> `\x1bOp`, Enter -> `\x1bOM`).
Working tree was clean before and after (`git status --short` empty apart from this file).

## Verdict
EVIDENCE: PROVEN
