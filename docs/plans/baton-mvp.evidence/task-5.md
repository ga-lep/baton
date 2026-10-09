# Evidence — Task 5: Screen abstraction with a vt100 implementation
Commit: 3b1c604
Environment: local `cargo test -p baton term::` and `cargo tree -i vt100`; no outside surface (exercised externally in Task 6).

## Screen trait (Send) with process/resize/size/cursor/encode_modes/snapshot/scrollback_len/scrollback_rows/set_view_offset/title/render; Vt100Screen::new(rows, cols, scrollback_cap)
Status: PROVEN (compile-time + unit tests)

```console
$ grep -n "fn \|trait" crates/baton/src/term/screen.rs
11:pub trait Screen: Send {
13:    fn process(&mut self, bytes: &[u8]) -> Vec<u8>;
15:    fn resize(&mut self, rows: u16, cols: u16);
17:    fn size(&self) -> (u16, u16);
19:    fn cursor(&self) -> (u16, u16);
21:    fn encode_modes(&self) -> EncodeModes;
26:    fn snapshot(&mut self) -> Vec<u8>;
28:    fn scrollback_len(&mut self) -> usize;
30:    fn scrollback_rows(&mut self, start: usize, count: usize) -> Vec<Vec<u8>>;
32:    fn set_view_offset(&mut self, n: usize);
34:    fn title(&self) -> String;
36:    fn render(&self, area: Rect, buf: &mut Buffer, show_cursor: bool);
$ cargo test -p baton term::
test term::vt100_screen::tests::dsr_uses_screen_cursor ... ok
test term::vt100_screen::tests::resize_size_title ... ok
test term::vt100_screen::tests::render_red_line ... ok
test term::vt100_screen::tests::scrollback_trimmed_and_ordered ... ok
test term::vt100_screen::tests::snapshot_alt_screen ... ok
test term::vt100_screen::tests::snapshot_colored ... ok
test term::vt100_screen::tests::snapshot_mouse_and_modes ... ok
test term::vt100_screen::tests::snapshot_modify_other_keys_and_kitty ... ok
(plus 7 term::encode_tests ... ok)
test result: ok. 15 passed; 0 failed; 0 ignored
```
All methods exist; DSR test asserts `process(b"ab\r\ncd\x1b[6n") == b"\x1b[2;3R"` (cursor from screen); resize/size/title test asserts `size()==(5,20)`, `title()=="hello"`.

## Test: snapshot of A fed into fresh B gives identical contents, cursor, encode_modes, alt-screen (colored, alt screen, mouse modes, modifyOtherKeys)
Status: PROVEN

Key assertions (vt100_screen.rs `assert_same`): `assert_eq!(a.contents(), b.contents()); assert_eq!(a.cursor(), b.cursor()); assert_eq!(a.encode_modes(), b.encode_modes()); assert_eq!(a.alt_screen(), b.alt_screen());`
Cases passing: snapshot_colored, snapshot_alt_screen, snapshot_mouse_and_modes (focus 1004, 2004, 1000/1002, SGR 1006, DECCKM, keypad), snapshot_modify_other_keys_and_kitty (asserts `modify_other_keys == 2`, plus alt screen and 1003).

## Test: scrollback beyond cap trimmed; scrollback_rows oldest-first
Status: PROVEN

Key assertions: 20 lines into 3-row screen, cap 5: `scrollback_len() == 5`; `scrollback_rows(0,10).len()==5`, row i contains `line{13+i}`; `scrollback_rows(2,2)[0]` contains `line15`. Test `scrollback_trimmed_and_ordered ... ok`.

## Test: render into TestBackend shows expected text and colors for SGR-red line
Status: PROVEN

Key assertions: `line == "hello"` and `buf[(0,0)].fg == Color::Indexed(1)`. Test `render_red_line ... ok`.

## `cargo tree -i vt100` resolves to a single version
Status: PROVEN

```console
$ cargo tree -i vt100
vt100 v0.16.2
├── baton v0.1.0 (/home/glepape/project/baton/crates/baton)
├── baton-testkit v0.1.0 (/home/glepape/project/baton/crates/baton-testkit)
└── tui-term v0.3.4
    └── baton v0.1.0 (/home/glepape/project/baton/crates/baton)
```
Only v0.16.2 appears.

## Verdict
EVIDENCE: PROVEN
