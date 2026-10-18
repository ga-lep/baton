# Evidence — Task 1: Project summary model and lines
Commit: 4cc4aa8
Environment: local cargo (package name is `baton-tui`, not `baton`; the plan's `cargo test -p baton` does not resolve). No outside surface: pure model and accessors; rendering arrives in Task 2.

## All criteria (activity mapping, lines output, clipping, overflow, accessors)
Status: NOT PROVEN (internal - covered by tests)

```console
$ cargo test -p baton-tui
test tui::app::tests::project_sessions_follow_sidebar_order_even_when_collapsed ... ok
test tui::app::tests::summary_project_is_set_only_on_a_project_row ... ok
test tui::project_summary::tests::activity_maps_every_status ... ok
test tui::project_summary::tests::exited_counts_as_off_and_empty_groups_are_left_out ... ok
test tui::project_summary::tests::no_entries_say_no_sessions ... ok
test tui::project_summary::tests::lines_are_clipped_and_cleaned ... ok
test tui::project_summary::tests::overflow_ends_with_a_more_line ... ok
test tui::project_summary::tests::three_sessions_render_tally_and_entries ... ok
test result: ok. 210 passed; 0 failed (plus all other suites ok; filtered to relevant lines)
```

## cargo test / clippy / fmt pass
Status: PROVEN

```console
$ cargo clippy --all-targets --all-features -- -D warnings; echo clippy_exit=$?
clippy_exit=0
$ cargo fmt --check; echo fmt=$?
fmt=0
```
