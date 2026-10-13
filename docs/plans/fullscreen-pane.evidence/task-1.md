# Evidence — Task 1: fullscreen keymap actions
Commit: 48a7db5
Environment: cargo debug build of `baton`; configs in a scratch dir outside the repo, selected via XDG_CONFIG_HOME.

## NormalAction::Fullscreen / FocusAction::Fullscreen exist, names, default keys, in all()
Status: PROVEN
Diff (git diff c3ae668..48a7db5 -- crates/baton-core/src/keymap.rs) adds `Fullscreen` to both enums, to `all()`, `name()` = "fullscreen", defaults `["z","alt-z"]` (normal) and `["alt-z"]` (focus). Behaviour confirmed by test `keymap::tests::fullscreen_action_defaults_overrides_and_describe ... ok` (cargo test -p baton-core).

## Keymap::default() builds; z / alt-z / focus alt-z resolve to fullscreen
Status: PROVEN
Test above asserts these lookups and passes; default keymap builds without conflict (default_keys_all_parse_and_default_equals_empty_overrides ... ok).

## Overrides take effect; focus collision with next_attention is a Conflict
Status: PROVEN
```console
$ XDG_CONFIG_HOME=<scratch>/n baton config check   # [keybindings.normal] fullscreen = "f"
exit=0
$ XDG_CONFIG_HOME=<scratch>/f12 baton config check # [keybindings.focus] fullscreen = "f12"
exit=0
$ XDG_CONFIG_HOME=<scratch>/alt-n baton config check # [keybindings.focus] fullscreen = "alt-n"
invalid config: keybindings.focus: key "alt-n" is bound to both next_attention and fullscreen
exit=1
```
Overrides accepted; collision rejected with a keybindings.focus conflict. Lookup effects (f -> Fullscreen, z -> None, f12 -> Fullscreen, alt-z -> None) are asserted in the passing unit test.

## describe_normal()/describe_focus() contain ("fullscreen","z, alt-z") / ("fullscreen","alt-z")
Status: PROVEN
Asserted in the passing unit test above (internal API, no CLI surface).

## Existing default-keys-parse test and cargo test -p baton-core green
Status: PROVEN
```console
$ cargo test -p baton-core
test keymap::tests::fullscreen_action_defaults_overrides_and_describe ... ok
test keymap::tests::default_keys_all_parse_and_default_equals_empty_overrides ... ok
test result: ok. 119 passed; 0 failed
test result: ok. 15 passed; 0 failed
test result: ok. 1 passed; 0 failed
```

## Verdict
EVIDENCE: PROVEN
