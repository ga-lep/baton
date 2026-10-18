# Evidence — Task 1: App state for a summary-showing main pane
Commit: 41ccd61
Environment: local cargo, package baton-tui. Evidence hint: none (internal App state; visible change lands in Task 2). Criteria are checked by running the named tests.

## summary_project() Some on project row, None on session row / no cursor / focus / fullscreen
Status: PROVEN (via test run; internal)
```console
$ cargo test -p baton-tui
test tui::app::tests::summary_project_is_set_only_on_a_project_row ... ok
```
Extended test passes.

## on_screen_is_none_while_a_project_summary_is_shown
Status: PROVEN (via test run; internal)
```console
test tui::app::tests::on_screen_is_none_while_a_project_summary_is_shown ... ok
```

## session_keys_on_a_project_row_only_hint
Status: PROVEN (via test run; internal)
```console
test tui::app::tests::session_keys_on_a_project_row_only_hint ... ok
```

## o / Enter / Space on a project row unchanged
Status: PROVEN (existing tests pass unchanged; diff touches only app.rs and app/tests.rs)
```console
$ cargo test -p baton-tui   (lib: 216 passed; 0 failed; all other suites ok)
```

## fmt, clippy, tests pass
Status: PROVEN
```console
$ cargo fmt --check; echo "fmt exit=$?"
fmt exit=0
$ cargo clippy --all-targets --all-features -- -D warnings; echo "clippy exit=$?"
clippy exit=0
$ cargo test -p baton-tui
test result: ok. 216 passed; 0 failed (and every other suite: 0 failed)
```
