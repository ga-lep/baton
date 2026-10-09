# Evidence — Task 2: terminal mode scanner / responder
Commit: 1b7386a
Environment: `cargo test -p baton-core term::` in the repo, plus a scratch crate in a mktemp dir (path dependency on crates/baton-core, since deleted). The scratch program used a hand-written approximation of the Finding 9 bytes, not the exact capture. The exact capture is the `CLAUDE_START` constant in the unit tests.

## TermModes exposes the listed fields
Status: PROVEN
```console
$ grep -n "pub " crates/baton-core/src/term/scanner.rs (fields)
19: pub modify_other_keys: u8,  20: pub kitty_flags: u8,  21: pub sync_output: bool,
22: pub focus_reporting: bool,  23: pub mouse: MouseMode,  24: pub sgr_mouse: bool,  25: pub alt_screen: bool,
```
The scratch program, an external consumer, printed: `TermModes { modify_other_keys: 2, kitty_flags: 5, sync_output: true, focus_reporting: true, mouse: ButtonMotion, sgr_mouse: true, alt_screen: true }`.

## Finding 9 bytes give kitty=5, mok=2, sync=true; the reset gives 0/0/false
Status: PROVEN
```console
$ cargo test -p baton-core term::
test term::scanner::tests::claude_startup_sets_modes ... ok
test term::scanner::tests::fresh_reset_example ... ok
test term::scanner::tests::reset_sequence_clears_modes ... ok
test result: ok. 11 passed; 0 failed
```
In the scratch program, after the reset bytes: `modify_other_keys: 0, kitty_flags: 0, sync_output: false`.

## Splitting at every byte boundary gives the same modes and replies
Status: PROVEN
```console
test term::scanner::tests::every_split_matches_single_feed ... ok
```
The test loops over every split index 0..=len on three inputs, including CLAUDE_START and OSC/DCS-containing input. It compares both modes and replies.

## DA1 gives exactly one reply; the other queries give none
Status: PROVEN
```console
scratch output:
"\u{1b}[c" -> "\u{1b}[?62;22c"
"\u{1b}[?u" -> ""
"\u{1b}[>0q" -> ""
"\u{1b}]7501;?\u{1b}\\" -> ""
tests replies, unanswered_queries: ok
```

## DSR 6n uses the 1-based cursor passed in; OSC/DCS contents are not misparsed
Status: PROVEN
```console
scratch: "\u{1b}[6n" -> "\u{1b}[12;34R"   (cursor passed in = (12,34))
test term::scanner::tests::string_contents_not_misparsed ... ok
test term::scanner::tests::state_recovers_after_string ... ok
```
The cursor argument is passed as `(row, col)`, and the reply echoes it as `12;34`.

## Verdict
EVIDENCE: PROVEN
