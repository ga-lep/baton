//! Config parsing against the spec example and error cases.

use baton_core::config::{Config, ConfigError};
use baton_core::keymap::{FocusAction, NormalAction};
use std::path::PathBuf;

const SPEC_EXAMPLE: &str = r##"
editor = "code {path}"          # {path} = repo dir; spawned detached
notifications = true
scrollback_lines = 10000

[profiles.work]
command = "claude"

[profiles.personal]
command = "claude"
env = { CLAUDE_CONFIG_DIR = "~/.claude-personal" }

[[projects]]
name = "loop"
profile = "work"                 # default for the project's repos
repos = [
  { path = "~/code/alaloop" },
  { path = "~/code/powerloop", args = ["--model", "opus"] },
  { path = "~/code/hyperloop", profile = "personal" },   # per-repo override
]

[[projects]]
name = "blog"
profile = "personal"
repos = [{ path = "~/code/blog" }]

[pricing]                        # $/MTok, used for the "est." cost only
default = { input = 3.0, output = 15.0, cache_read = 0.3, cache_write = 3.75 }
# per-model overrides keyed by model id prefix

[keybindings.normal]
next_attention = "n"
restart = "r"
# …

[keybindings.focus]
unfocus = "ctrl-\\"
next_attention = "alt-n"
"##;

fn env(key: &str) -> Option<String> {
    match key {
        "HOME" => Some("/home/u".into()),
        "FOO" => Some("bar".into()),
        _ => None,
    }
}

fn parse(text: &str) -> Result<Config, ConfigError> {
    Config::parse(text, &env)
}

#[test]
fn spec_example_parses_and_resolves() {
    let c = parse(SPEC_EXAMPLE).expect("spec example");
    assert_eq!(c.editor, "code {path}");
    assert!(c.notifications);
    assert_eq!(c.scrollback_lines, 10000);
    assert_eq!(c.projects.len(), 2);
    let loop_ = &c.projects[0];
    assert_eq!(loop_.sessions.len(), 3);
    assert_eq!(
        loop_.sessions[0].repo,
        PathBuf::from("/home/u/code/alaloop")
    );
    assert_eq!(loop_.sessions[0].profile, "work");
    assert_eq!(loop_.sessions[1].args, ["--model", "opus"]);
    assert_eq!(loop_.sessions[2].profile, "personal");
    assert_eq!(
        loop_.sessions[2]
            .env
            .get("CLAUDE_CONFIG_DIR")
            .map(String::as_str),
        Some("/home/u/.claude-personal")
    );
    assert_eq!(c.projects[1].sessions[0].profile, "personal");
    assert!(c.pricing.default.is_some());
    let ctrl_backslash = "ctrl-\\".parse().expect("key");
    assert_eq!(
        c.keybindings.focus_action(&ctrl_backslash),
        Some(FocusAction::Unfocus)
    );
}

#[test]
fn keybindings_accept_strings_and_lists() {
    let c = parse(
        "[keybindings.normal]\nnext_attention = \"x\"\nquit = [\"q\", \"ctrl-c\"]\n\
         [keybindings.focus]\nunfocus = \"ctrl-g\"\n",
    )
    .expect("valid");
    let k = |s: &str| s.parse().expect("key");
    assert_eq!(
        c.keybindings.normal_action(&k("x")),
        Some(NormalAction::NextAttention)
    );
    assert_eq!(c.keybindings.normal_action(&k("n")), None);
    assert_eq!(
        c.keybindings.normal_action(&k("ctrl-c")),
        Some(NormalAction::Quit)
    );
    assert_eq!(
        c.keybindings.focus_action(&k("ctrl-g")),
        Some(FocusAction::Unfocus)
    );
}

#[test]
fn bad_keybindings_are_config_errors_naming_the_key() {
    for (text, needle) in [
        (
            "[keybindings.normal]\nteleport = \"x\"\n",
            "keybindings.normal.teleport",
        ),
        (
            "[keybindings.normal]\nquit = \"ctrl-nope\"\n",
            "keybindings.normal.quit",
        ),
        ("[keybindings.normal]\nrestart = \"n\"\n", "bound to both"),
        ("[keybindings.focus]\nunfocus = \"x\"\n", "swallow typing"),
        ("[keybindings.focus]\nunfocus = []\n", "no way out"),
        ("[keybindings.sidebar]\nquit = \"q\"\n", "unknown mode"),
    ] {
        let e = parse(text).expect_err(text).to_string();
        assert!(e.contains(needle), "{text}: {e}");
    }
}

#[test]
fn defaults_apply_to_empty_config() {
    let c = parse("").expect("empty");
    assert_eq!(c.editor, "xdg-open {path}");
    assert!(c.notifications);
    assert_eq!(c.scrollback_lines, 10000);
    assert_eq!(c.hook_timeout_secs, 20);
    assert!(c.projects.is_empty());
}

#[test]
fn implicit_default_profile_and_command_split() {
    let c = parse(
        r#"
[profiles.p]
command = "bash --norc"
env = { FOO = "$FOO/x" }
[[projects]]
name = "a"
repos = [{ path = "/tmp" }, { path = "/var", profile = "p", args = ["-x"] }]
"#,
    )
    .expect("ok");
    let s = &c.projects[0].sessions;
    assert_eq!(s[0].profile, "default");
    assert_eq!(s[0].argv, ["claude"]);
    assert_eq!(s[1].argv, ["bash", "--norc"]);
    assert_eq!(s[1].env["FOO"], "bar/x");
    assert_eq!(s[1].args, ["-x"]);
}

#[test]
fn command_is_expanded() {
    let c = parse(
        "[profiles.p]\ncommand = \"~/bin/claude --x\"\n[[projects]]\nname=\"a\"\nprofile=\"p\"\nrepos=[{path=\"/tmp\"}]\n",
    )
    .expect("ok");
    assert_eq!(
        c.projects[0].sessions[0].argv,
        ["/home/u/bin/claude", "--x"]
    );
}

#[test]
fn unknown_profile_is_an_error_with_context() {
    let e = parse("[[projects]]\nname=\"x\"\nprofile=\"nope\"\nrepos=[{path=\"/tmp\"}]\n")
        .expect_err("unknown");
    let msg = e.to_string();
    assert!(msg.contains("unknown profile \"nope\""), "{msg}");
    assert!(msg.contains('x'), "{msg}");
    let e = parse("[[projects]]\nname=\"x\"\nrepos=[{path=\"/tmp\",profile=\"zz\"}]\n")
        .expect_err("unknown");
    assert!(e.to_string().contains("unknown profile \"zz\""));
}

#[test]
fn structural_errors() {
    assert!(matches!(
        parse(
            "[[projects]]\nname=\"x\"\nrepos=[{path=\"/a\"}]\n[[projects]]\nname=\"x\"\nrepos=[{path=\"/b\"}]\n"
        ),
        Err(ConfigError::DuplicateProject { .. })
    ));
    assert!(matches!(
        parse("[[projects]]\nname=\"x\"\nrepos=[{path=\"/a\"},{path=\"/a\"}]\n"),
        Err(ConfigError::DuplicateRepo { .. })
    ));
    assert!(matches!(
        parse("[[projects]]\nname=\"x\"\nrepos=[]\n"),
        Err(ConfigError::EmptyRepos { .. })
    ));
    assert!(matches!(
        parse("[[projects]]\nname=\"a/b\"\nrepos=[{path=\"/a\"}]\n"),
        Err(ConfigError::InvalidProjectName { .. })
    ));
    assert!(matches!(
        parse("[[projects]]\nname=\"x\"\nrepos=[{path=\"$NOPE/a\"}]\n"),
        Err(ConfigError::Expand { .. })
    ));
}

#[test]
fn invalid_toml_is_a_parse_error() {
    assert!(matches!(parse("a.=1"), Err(ConfigError::Toml(_))));
    assert!(matches!(parse("[[projects]\n"), Err(ConfigError::Toml(_))));
}

#[test]
fn missing_file_is_empty_config() {
    let d = tempfile::tempdir().expect("tmp");
    let c = Config::load(&d.path().join("nope.toml"), &env).expect("empty");
    assert!(c.projects.is_empty());
}

#[test]
fn attach_redraw_nudge_defaults_on_and_can_be_disabled() {
    assert!(parse("").expect("empty").attach_redraw_nudge);
    let c = parse("attach_redraw_nudge = false").expect("parses");
    assert!(!c.attach_redraw_nudge);
}
