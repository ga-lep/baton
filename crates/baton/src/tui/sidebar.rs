//! The project tree: configured projects merged with the daemon's sessions.

use baton_core::attention::needs_attention;
use baton_core::config::Config;
use baton_core::paths;
use baton_proto::{SessionId, SessionInfo, Status};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::ListItem;

use super::labels::{badge, status_label};

/// Where the sidebar cursor is, by identity so it survives list changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    /// A project row.
    Project(String),
    /// A session row.
    Session(SessionId),
}

/// One visible sidebar row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Row<'a> {
    /// A project header.
    Project {
        /// Project name.
        name: &'a str,
        /// Whether the daemon has any running or exited session for it, as
        /// opposed to none or only remembered ones.
        open: bool,
        /// Whether any of those sessions is still running.
        live: bool,
    },
    /// A session under its project; `n` is its 1-based position there.
    Session {
        /// 1-based number within the project.
        n: usize,
        /// The session.
        info: &'a SessionInfo,
    },
}

impl Row<'_> {
    /// The cursor value that points at this row.
    pub fn cursor(&self) -> Cursor {
        match self {
            Row::Project { name, .. } => Cursor::Project((*name).to_owned()),
            Row::Session { info, .. } => Cursor::Session(info.id.clone()),
        }
    }

    /// The sidebar text of this row.
    pub fn text(&self) -> String {
        match self {
            Row::Project {
                name, open: true, ..
            } => format!("▾ {name}"),
            Row::Project {
                name, open: false, ..
            } => format!("▸ {name}  (closed)"),
            Row::Session { n, info } => format!(
                "  {n} {} {}  {}",
                badge(info.status),
                repo_name(info),
                status_label(info.status)
            ),
        }
    }

    /// The style of this row: projects without live sessions are dim and
    /// sessions that need attention are highlighted.
    pub fn style(&self) -> Style {
        match self {
            Row::Project { live: false, .. } => Style::default().add_modifier(Modifier::DIM),
            Row::Session { info, .. } if needs_attention(info.status) => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            _ => Style::default(),
        }
    }

    /// The list item for this row.
    pub fn item(&self) -> ListItem<'static> {
        ListItem::new(self.text()).style(self.style())
    }
}

/// Last path component of a session's repo.
pub fn repo_name(s: &SessionInfo) -> &str {
    s.repo.rsplit('/').next().unwrap_or(&s.repo)
}

/// The rows to show: configured projects in config order, then projects the
/// daemon knows that the config does not (so no session is ever hidden).
pub fn rows<'a>(projects: &'a [String], sessions: &'a [SessionInfo]) -> Vec<Row<'a>> {
    let mut names: Vec<&str> = projects.iter().map(String::as_str).collect();
    for s in sessions {
        if !names.contains(&s.project.as_str()) {
            names.push(&s.project);
        }
    }
    let mut out = Vec::new();
    for name in names {
        let mine: Vec<&SessionInfo> = sessions.iter().filter(|s| s.project == name).collect();
        out.push(Row::Project {
            name,
            // Remembered-only sessions do not make a project open.
            open: mine.iter().any(|s| s.status != Status::Closed),
            live: mine
                .iter()
                .any(|s| !matches!(s.status, Status::Exited(_) | Status::Closed)),
        });
        for (i, info) in mine.into_iter().enumerate() {
            out.push(Row::Session { n: i + 1, info });
        }
    }
    out
}

/// What the TUI reads from the config file on attach.
pub struct Settings {
    /// Project names, in order.
    pub projects: Vec<String>,
    /// Key bindings.
    pub keymap: baton_core::keymap::Keymap,
    /// The `editor` command template.
    pub editor: String,
}

/// Project names, key bindings and editor from the config file.
///
/// # Errors
/// A message describing why the config could not be read or is invalid.
pub fn load_settings() -> Result<Settings, String> {
    let path = paths::config_file().map_err(|e| e.to_string())?;
    let config = Config::load(&path, &|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .map_err(|e| e.to_string())?;
    Ok(Settings {
        projects: config.projects.into_iter().map(|p| p.name).collect(),
        keymap: config.keybindings,
        editor: config.editor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(project: &str, repo: &str, status: Status) -> SessionInfo {
        SessionInfo {
            id: SessionId(format!("{project}/{repo}")),
            project: project.into(),
            repo: repo.into(),
            profile: None,
            status,
            claude_session_id: None,
            transcript_path: None,
            model: None,
            started_at: 0,
            exit_code: None,
            usage: None,
            launch: None,
        }
    }

    #[test]
    fn rows_merge_config_order_with_sessions_and_unknown_projects() {
        let projects = vec!["b".to_owned(), "a".to_owned()];
        let sessions = vec![
            info("a", "/r/one", Status::Running),
            info("z", "/r/zed", Status::Idle),
            info("a", "/r/two", Status::Exited(1)),
        ];
        let text: Vec<String> = rows(&projects, &sessions).iter().map(Row::text).collect();
        assert_eq!(
            text,
            vec![
                "▸ b  (closed)",
                "▾ a",
                "  1 ● one  running",
                "  2 ✗ two  exited 1",
                "▾ z",
                "  1 ○ zed  idle",
            ]
        );
    }

    #[test]
    fn only_projects_without_live_sessions_are_dim() {
        let projects = vec!["b".to_owned(), "a".to_owned(), "c".to_owned()];
        let sessions = vec![
            info("a", "/r/one", Status::Running),
            info("c", "/r/x", Status::Exited(0)),
        ];
        let live: Vec<bool> = rows(&projects, &sessions)
            .iter()
            .filter_map(|r| match r {
                Row::Project { live, .. } => Some(*live),
                Row::Session { .. } => None,
            })
            .collect();
        assert_eq!(live, vec![false, true, false]);
    }

    #[test]
    fn sessions_needing_attention_are_highlighted() {
        let sessions = vec![
            info("a", "/r/p", Status::Permission),
            info("a", "/r/y", Status::YourTurn),
            info("a", "/r/i", Status::Idle),
            info("a", "/r/r", Status::Running),
            info("a", "/r/u", Status::Unknown),
        ];
        let styles: Vec<Style> = rows(&["a".to_owned()], &sessions)
            .iter()
            .skip(1)
            .map(Row::style)
            .collect();
        let hot = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        assert_eq!(
            styles,
            vec![
                hot,
                hot,
                Style::default(),
                Style::default(),
                Style::default()
            ]
        );
    }

    #[test]
    fn remembered_sessions_sit_under_a_project_that_is_still_closed() {
        let projects = vec!["a".to_owned(), "b".to_owned()];
        let sessions = vec![
            info("a", "/r/one", Status::Closed),
            info("b", "/r/two", Status::Idle),
        ];
        let rows = rows(&projects, &sessions);
        let text: Vec<String> = rows.iter().map(Row::text).collect();
        assert_eq!(
            text,
            vec![
                "▸ a  (closed)",
                "  1 ◌ one  closed",
                "▾ b",
                "  1 ○ two  idle",
            ]
        );
        assert!(matches!(
            rows[0],
            Row::Project {
                open: false,
                live: false,
                ..
            }
        ));
    }
}
