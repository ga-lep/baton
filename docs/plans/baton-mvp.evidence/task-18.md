# Evidence — Task 18: Remappable keybindings, help overlay and e editor
Commit: 535a0e9
Environment: target/debug/baton driven by target/debug/baton-drive (40x120 PTY), scratch BATON_CONFIG/STATE_DIR, BATON_RUNTIME_DIR=/tmp/b18r, BATON_NOTIFY_SINK=off, fake-claude profile. Editor is a stand-in script (appends argv to a file). Repo 1 path contains a space and `;$()`: "<scratch>/my repo;$(touch pwned)". Raw drive output: scratch dir t18/drive.out (not kept in repo). Excerpts below trimmed to relevant lines.

## Help overlay: `?` lists active bindings for both modes (defaults)
Status: PROVEN
```console
$ baton-drive --size 40x120 --step 'wait:NORMAL' --step 'send:?' --step 'wait:next_attention' --step dump -- target/debug/baton   (default config)
│         ┌Key bindings (? or Esc to close)─────────...
│         │Normal mode                                      Focus mode
│         │move_down       j, down                          unfocus         ctrl-\
│         │move_up         k, up                            next_attention  alt-n
│         │activate        enter, l                         session_1       alt-1
│         │open_project    o                                ... session_9   alt-9
│         │select_1 .. select_9   1 .. 9
│         │next_attention  n
│         │restart         r
│         │editor          e
│         │scroll_up       ctrl-u   scroll_down ctrl-d   page_up pgup   page_down pgdn
│         │live            G
│         │help            ?
│         │quit            q
 NORMAL │ ⏎ focus  n next-attention  r restart  e editor  o open project  ? help  q quit
```
All normal and focus actions are listed with default keys.

## Remap (next_attention = "x", unfocus = "ctrl-g") reflected in help and bar, and working
Status: PROVEN
```console
$ baton config check   (remapped config)  -> exit=0
$ baton-drive ... --step 'send:?' --step 'wait:next_attention +x' --step 'wait:unfocus +ctrl-g' ... (drive_exit=0)
  [help overlay waits for "next_attention +x" and "unfocus +ctrl-g" succeeded]
 NORMAL │ ⏎ focus  x next-attention  r restart  e editor  o open project  ? help  q quit
 (beta set to permission via 'baton debug send'; selected session stays "my repo..." )
after 'n':  title ┌my repo;$(touch pwned) · p ...   (unchanged, still sidebar "2 ◐ beta  permission")
after 'x':  title ┌beta · p · ◐ permission
after '1', Enter:  FOCUS │ Ctrl-g back  Alt-n next-attention  Alt-1..9 session
after \x07:        NORMAL │ ⏎ focus  x next-attention ...
```
The bar and help show the remap; x jumps to the attention session, n does not, Ctrl-g leaves focus (focus bar also says "Ctrl-g back").

## e launches the editor; path with spaces and ;$() is one argv element
Status: PROVEN
```console
$ cat <scratch>/argv.txt      (after pressing e on session 1)
ARG[<scratch>/my repo;$(touch pwned)]
---
$ ls "<scratch>/my repo;$(touch pwned)"; find / -name pwned ...
(empty; no file named pwned anywhere)
```
Exactly one argv element, unexpanded, no shell injection.

## Failing editor shows an error in the bar for ~5 s
Status: PROVEN
```console
$ (editor = "definitely-not-an-editor-xyz {path}") press e
 NORMAL │ cannot run definitely-not-an-editor-xyz: No such file or directory (os error 2)
$ ... sleep 7000 ms, dump
 NORMAL │ ⏎ focus  x next-attention  r restart  e editor  o open project  ? help  q quit
```
Error is shown, gone by 7 s (exact 5 s bound not timed here; covered by e2e test).

## Invalid keybinding configs: config check exits 1 naming the key
Status: PROVEN
```console
--- [keybindings.normal] teleport = "x"
invalid config: keybindings.normal.teleport: unknown action
exit=1
--- [keybindings.normal] quit = "ctrl-nope"
invalid config: keybindings.normal.quit: invalid key "ctrl-nope": unknown key name
exit=1
--- [keybindings.normal] restart = "n"
invalid config: keybindings.normal: key "n" is bound to both next_attention and restart
exit=1
--- [keybindings.focus] unfocus = "x"
invalid config: keybindings.focus.unfocus: "x" is a plain printable key and would swallow typing in focus mode; use a modified or non-character key such as ctrl-g
exit=1
```

## Parser/matching table tests, setsid/detached spawn, zombie reaping
Status: NOT PROVEN (internal — covered by tests)

## Cleanup
```console
$ baton daemon stop; pgrep -x baton; pgrep -x fake-claude
daemon stopped
(no output from either pgrep)
```
