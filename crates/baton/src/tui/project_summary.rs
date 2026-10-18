//! The project summary drawn in the main pane while the cursor is on a project
//! row in normal mode (not focus mode, not fullscreen): a tally that counts
//! the sessions by their status, using the same labels as the rest of the app,
//! then one entry per session.

use baton_core::attention::needs_attention;
use baton_proto::{SessionInfo, Status};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use super::info_panel::{clean, clip};
use super::labels::{badge, status_label};
use super::sidebar::repo_name;

/// Tally groups in display order: the word and a representative status.
/// A session belongs to the group whose status has the same variant, so
/// `Exited(_)` matches every exit code.
const GROUPS: [(&str, Status); 8] = [
    ("running", Status::Running),
    ("permission", Status::Permission),
    ("your turn", Status::YourTurn),
    ("idle", Status::Idle),
    ("starting", Status::Starting),
    ("unknown", Status::Unknown),
    ("exited", Status::Exited(0)),
    ("closed", Status::Closed),
];

/// One session of the summarized project.
pub struct Entry<'a> {
    /// Number in the sidebar.
    pub n: usize,
    /// The session.
    pub info: &'a SessionInfo,
    /// Its git branch, if known.
    pub branch: Option<&'a str>,
}

/// The summary lines, each at most `width` cells and at most `height` lines.
/// When the entries do not fit, the last line is `+N more`.
pub fn lines(entries: &[Entry<'_>], width: usize, height: usize) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }
    if entries.is_empty() {
        return vec![Line::from(clip("no sessions", width))];
    }
    let total = 1 + 2 * entries.len();
    let (allowed, shown) = if total <= height {
        (height, entries.len())
    } else {
        let content = height - 1;
        (content, content.saturating_sub(1) / 2)
    };
    let mut out = Vec::new();
    if allowed >= 1 {
        out.push(Line::from(clip(&tally(entries), width)));
    }
    for e in &entries[..shown] {
        out.extend(entry_lines(e, width));
    }
    if shown < entries.len() {
        out.push(Line::from(clip(
            &format!("+{} more", entries.len() - shown),
            width,
        )));
    }
    out
}

fn tally(entries: &[Entry<'_>]) -> String {
    GROUPS
        .iter()
        .filter_map(|(word, group)| {
            let n = entries
                .iter()
                .filter(|e| std::mem::discriminant(&e.info.status) == std::mem::discriminant(group))
                .count();
            (n > 0).then(|| format!("{n} {word}"))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn entry_lines(e: &Entry<'_>, width: usize) -> [Line<'static>; 2] {
    let s = e.info.status;
    let head = format!(
        "{} {} {}  {}",
        e.n,
        badge(s),
        clean(repo_name(e.info)),
        status_label(s)
    );
    let head_style = if needs_attention(s) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let profile = clean(e.info.profile.as_deref().unwrap_or("default"));
    let detail = match e.branch {
        Some(b) => format!("  ⎇ {} · {profile}", clean(b)),
        None => format!("  {profile}"),
    };
    [
        Line::styled(clip(&head, width), head_style),
        Line::styled(clip(&detail, width), Style::default().fg(Color::DarkGray)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use baton_proto::SessionId;

    fn info(repo: &str, profile: Option<&str>, status: Status) -> SessionInfo {
        SessionInfo {
            id: SessionId(format!("x/{repo}")),
            project: "x".into(),
            repo: format!("/r/{repo}"),
            profile: profile.map(Into::into),
            status,
            claude_session_id: None,
            transcript_path: None,
            model: None,
            started_at: 0,
            exit_code: None,
            usage: None,
            launch: None,
            quota: None,
        }
    }

    fn text(l: &Line<'_>) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn texts(ls: &[Line<'_>]) -> Vec<String> {
        ls.iter().map(text).collect()
    }

    fn entries<'a>(infos: &'a [SessionInfo], branches: &[Option<&'a str>]) -> Vec<Entry<'a>> {
        infos
            .iter()
            .zip(branches)
            .enumerate()
            .map(|(i, (info, branch))| Entry {
                n: i + 1,
                info,
                branch: *branch,
            })
            .collect()
    }

    #[test]
    fn no_entries_say_no_sessions() {
        assert_eq!(texts(&lines(&[], 30, 14)), vec!["no sessions"]);
    }

    #[test]
    fn three_sessions_render_tally_and_entries() {
        let infos = [
            info("api", Some("work"), Status::Running),
            info("web", Some("personal"), Status::Permission),
            info("cli", None, Status::Idle),
        ];
        let es = entries(&infos, &[Some("main"), None, Some("feat/x")]);
        let ls = lines(&es, 40, 14);
        assert_eq!(
            texts(&ls),
            vec![
                "1 running · 1 permission · 1 idle",
                "1 ● api  running",
                "  ⎇ main · work",
                "2 ◐ web  permission",
                "  personal",
                "3 ○ cli  idle",
                "  ⎇ feat/x · default",
            ]
        );
        assert_eq!(
            ls[3].style,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(ls[2].style.fg, Some(Color::DarkGray));
        assert_eq!(ls[4].style.fg, Some(Color::DarkGray));
        assert_eq!(ls[6].style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn running_your_turn_idle_tally() {
        let infos = [
            info("a", None, Status::Running),
            info("b", None, Status::YourTurn),
            info("c", None, Status::Idle),
        ];
        let ls = lines(&entries(&infos, &[None; 3]), 40, 14);
        assert_eq!(text(&ls[0]), "1 running · 1 your turn · 1 idle");
        let yellow_bold = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        assert_eq!(ls[3].style, yellow_bold);
        assert_eq!(ls[5].style, Style::default());
    }

    #[test]
    fn exited_sessions_are_counted_together() {
        let infos = [
            info("api", None, Status::Exited(2)),
            info("web", None, Status::Exited(0)),
        ];
        let ls = lines(&entries(&infos, &[None; 2]), 30, 14);
        assert_eq!(text(&ls[0]), "2 exited");
        assert_eq!(text(&ls[1]), "1 ✗ api  exited 2");
        assert_eq!(text(&ls[3]), "2 ✗ web  exited 0");
    }

    #[test]
    fn every_status_group_in_fixed_order() {
        let infos = [
            info("a", None, Status::Closed),
            info("b", None, Status::Idle),
            info("c", None, Status::Exited(9)),
            info("d", None, Status::Running),
            info("e", None, Status::Unknown),
            info("f", None, Status::YourTurn),
            info("g", None, Status::Starting),
            info("h", None, Status::Permission),
        ];
        let ls = lines(&entries(&infos, &[None; 8]), 200, 40);
        assert_eq!(
            text(&ls[0]),
            "1 running · 1 permission · 1 your turn · 1 idle · 1 starting · 1 unknown · 1 exited · 1 closed"
        );
    }

    #[test]
    fn group_words_match_status_labels() {
        assert_eq!(GROUPS.len(), 8);
        for (word, status) in GROUPS {
            let label = status_label(status);
            if matches!(status, Status::Exited(_)) {
                assert!(label.starts_with(word), "{label}");
            } else {
                assert_eq!(label, word);
            }
        }
    }

    #[test]
    fn lines_are_clipped_and_cleaned() {
        let long = "r".repeat(50);
        let infos = [info(&long, Some("p"), Status::Running)];
        let branch = format!("{}\x1b[31m\u{202e}x", "b".repeat(50));
        for width in [10, 30] {
            let ls = lines(&entries(&infos, &[Some(&branch)]), width, 14);
            for l in &ls {
                assert!(text(l).chars().count() <= width, "{}", text(l));
            }
        }
        let ls = lines(&entries(&infos, &[Some("a\x1bb\u{202e}c")]), 30, 14);
        assert_eq!(text(&ls[2]), "  ⎇ abc · p");
    }

    #[test]
    fn overflow_ends_with_a_more_line() {
        let infos: Vec<SessionInfo> = (0..8)
            .map(|i| info(&format!("r{i}"), None, Status::Idle))
            .collect();
        let es = entries(&infos, &[None; 8]);
        let ls = lines(&es, 30, 14);
        assert_eq!(ls.len(), 14);
        assert_eq!(text(&ls[13]), "+2 more");
        for h in [0, 1, 2] {
            assert!(lines(&es, 30, h).len() <= h);
        }
        assert_eq!(lines(&es[..1], 30, 3).len(), 3);
    }
}
