//! `config.toml` parsing, validation and resolution into per-session launch specs.

use crate::keymap::{Keymap, KeymapError, Overrides};
use crate::pricing::Pricing;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Name of the implicit profile.
pub const DEFAULT_PROFILE: &str = "default";

/// Errors from loading or validating a config.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// File path.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// Invalid TOML or an invalid key/value shape.
    #[error("invalid config: {0}")]
    Toml(#[from] toml::de::Error),
    /// A project or repo names a profile that is not defined.
    #[error("{context}: unknown profile \"{profile}\"")]
    UnknownProfile {
        /// Where the profile was referenced.
        context: String,
        /// The missing profile.
        profile: String,
    },
    /// Two projects share a name.
    #[error("duplicate project name \"{name}\"")]
    DuplicateProject {
        /// Project name.
        name: String,
    },
    /// A project lists the same repo path twice.
    #[error("project \"{project}\": duplicate repo path {path}")]
    DuplicateRepo {
        /// Project name.
        project: String,
        /// Expanded repo path.
        path: PathBuf,
    },
    /// A project has no repos.
    #[error("project \"{project}\": repos is empty")]
    EmptyRepos {
        /// Project name.
        project: String,
    },
    /// A project name is empty or contains `/` (which would make session ids ambiguous).
    #[error("invalid project name \"{name}\": must be non-empty and not contain '/'")]
    InvalidProjectName {
        /// Project name.
        name: String,
    },
    /// `~` or `$VAR` expansion failed.
    #[error("{context}: {message}")]
    Expand {
        /// Where the value was found.
        context: String,
        /// Failure description.
        message: String,
    },
    /// An invalid `[keybindings]` section.
    #[error("invalid config: {0}")]
    Keybindings(#[from] KeymapError),
    /// A profile command is empty or has unbalanced quoting.
    #[error("profile \"{profile}\": invalid command: {message}")]
    BadCommand {
        /// Profile name.
        profile: String,
        /// Failure description.
        message: String,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    editor: Option<String>,
    notifications: Option<bool>,
    scrollback_lines: Option<usize>,
    hook_timeout_secs: Option<u64>,
    attach_redraw_nudge: Option<bool>,
    statusline: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, RawProfile>,
    #[serde(default)]
    projects: Vec<RawProject>,
    #[serde(default)]
    pricing: Pricing,
    #[serde(default)]
    keybindings: BTreeMap<String, BTreeMap<String, KeyList>>,
}

/// A keybinding value: one key string or a list of them.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum KeyList {
    One(String),
    Many(Vec<String>),
}

impl KeyList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => vec![s],
            Self::Many(v) => v,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    command: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    name: String,
    profile: Option<String>,
    repos: Vec<RawRepo>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepo {
    path: String,
    profile: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

/// A fully resolved session: what to run, where, and with which environment.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSpec {
    /// Owning project name.
    pub project: String,
    /// Expanded repo directory.
    pub repo: PathBuf,
    /// Effective profile name.
    pub profile: String,
    /// Profile command, split into argv.
    pub argv: Vec<String>,
    /// Extra args appended after Baton's own flags.
    pub args: Vec<String>,
    /// Profile environment, expanded.
    pub env: BTreeMap<String, String>,
}

/// A project and its sessions.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// Unique project name (no `/`).
    pub name: String,
    /// One session per repo.
    pub sessions: Vec<SessionSpec>,
}

/// Validated, resolved configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Editor command; `{path}` is the repo dir.
    pub editor: String,
    /// Whether desktop notifications are enabled.
    pub notifications: bool,
    /// Scrollback cap in lines.
    pub scrollback_lines: usize,
    /// Timeout after which a session with no hook activity becomes `unknown`.
    pub hook_timeout_secs: u64,
    /// Whether attaching nudges sessions to redraw (resize to `cols-1` then `cols`).
    pub attach_redraw_nudge: bool,
    /// The user's own status line command, run by `baton statusline` inside
    /// sessions (Baton's status line replaces the one in Claude's settings).
    pub statusline: Option<String>,
    /// Projects in file order.
    pub projects: Vec<Project>,
    /// Price table.
    pub pricing: Pricing,
    /// Validated key bindings (defaults with the config's overrides applied).
    pub keybindings: Keymap,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: "xdg-open {path}".to_owned(),
            notifications: true,
            scrollback_lines: 10_000,
            hook_timeout_secs: 20,
            attach_redraw_nudge: true,
            statusline: None,
            projects: Vec::new(),
            pricing: Pricing::default(),
            keybindings: Keymap::default(),
        }
    }
}

fn expand(
    s: &str,
    env: &dyn Fn(&str) -> Option<String>,
    context: &str,
) -> Result<String, ConfigError> {
    shellexpand::full_with_context(
        s,
        || env("HOME"),
        |var| -> Result<Option<String>, String> {
            env(var)
                .map(Some)
                .ok_or_else(|| format!("undefined variable ${var}"))
        },
    )
    .map(std::borrow::Cow::into_owned)
    .map_err(|e| ConfigError::Expand {
        context: context.to_owned(),
        message: e.to_string(),
    })
}

impl Config {
    /// Parses and validates `text`; `env` supplies `HOME` and `$VAR` values.
    pub fn parse(text: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(text)?;
        let defaults = Self::default();

        let mut profiles: BTreeMap<String, (Vec<String>, BTreeMap<String, String>)> =
            BTreeMap::new();
        profiles.insert(
            DEFAULT_PROFILE.to_owned(),
            (vec!["claude".to_owned()], BTreeMap::new()),
        );
        for (name, p) in &raw.profiles {
            let ctx = format!("profile \"{name}\"");
            let command = expand(p.command.as_deref().unwrap_or("claude"), env, &ctx)?;
            let argv = shell_words::split(&command).map_err(|e| ConfigError::BadCommand {
                profile: name.clone(),
                message: e.to_string(),
            })?;
            if argv.is_empty() {
                return Err(ConfigError::BadCommand {
                    profile: name.clone(),
                    message: "empty command".to_owned(),
                });
            }
            let mut vars = BTreeMap::new();
            for (k, v) in &p.env {
                vars.insert(k.clone(), expand(v, env, &format!("{ctx} env {k}"))?);
            }
            profiles.insert(name.clone(), (argv, vars));
        }

        let mut names = BTreeSet::new();
        let mut projects = Vec::new();
        for rp in raw.projects {
            if rp.name.is_empty() || rp.name.contains('/') {
                return Err(ConfigError::InvalidProjectName { name: rp.name });
            }
            if !names.insert(rp.name.clone()) {
                return Err(ConfigError::DuplicateProject { name: rp.name });
            }
            if rp.repos.is_empty() {
                return Err(ConfigError::EmptyRepos { project: rp.name });
            }
            let mut seen = BTreeSet::new();
            let mut sessions = Vec::new();
            for repo in rp.repos {
                let path = PathBuf::from(expand(
                    &repo.path,
                    env,
                    &format!("project \"{}\" repo path", rp.name),
                )?);
                if !seen.insert(path.clone()) {
                    return Err(ConfigError::DuplicateRepo {
                        project: rp.name,
                        path,
                    });
                }
                let profile = repo
                    .profile
                    .as_deref()
                    .or(rp.profile.as_deref())
                    .unwrap_or(DEFAULT_PROFILE);
                let Some((argv, vars)) = profiles.get(profile) else {
                    return Err(ConfigError::UnknownProfile {
                        context: format!("project \"{}\" repo {}", rp.name, path.display()),
                        profile: profile.to_owned(),
                    });
                };
                sessions.push(SessionSpec {
                    project: rp.name.clone(),
                    repo: path,
                    profile: profile.to_owned(),
                    argv: argv.clone(),
                    args: repo.args,
                    env: vars.clone(),
                });
            }
            projects.push(Project {
                name: rp.name,
                sessions,
            });
        }

        let overrides: Overrides = raw
            .keybindings
            .into_iter()
            .map(|(mode, actions)| {
                let actions = actions
                    .into_iter()
                    .map(|(action, keys)| (action, keys.into_vec()))
                    .collect();
                (mode, actions)
            })
            .collect();
        let keybindings = Keymap::from_overrides(&overrides)?;

        Ok(Self {
            editor: raw.editor.unwrap_or(defaults.editor),
            notifications: raw.notifications.unwrap_or(defaults.notifications),
            scrollback_lines: raw.scrollback_lines.unwrap_or(defaults.scrollback_lines),
            hook_timeout_secs: raw.hook_timeout_secs.unwrap_or(defaults.hook_timeout_secs),
            attach_redraw_nudge: raw
                .attach_redraw_nudge
                .unwrap_or(defaults.attach_redraw_nudge),
            statusline: raw.statusline.filter(|c| !c.trim().is_empty()),
            projects,
            pricing: raw.pricing,
            keybindings,
        })
    }

    /// Loads the config at `path`; a missing file yields the empty default config.
    pub fn load(path: &Path, env: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text, env),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }
}
