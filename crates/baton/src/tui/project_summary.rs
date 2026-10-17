//! The project summary shown in the lower sidebar block while the cursor is
//! on a project row: a tally of what the sessions are doing, then one entry
//! per session.

use baton_core::attention::needs_attention;
use baton_proto::{SessionInfo, Status};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use super::info_panel::{clean, clip};
use super::labels::{badge, status_label};
use super::sidebar::repo_name;

/// What a session is doing, in the user's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// The process is running (starting, running, or no hook events yet).
    Work,
    /// Waiting for the user.
    Wait,
    /// Idle.
    Idle,
    /// Exited or only remembered.
    Off,
}

impl Activity {
    /// Tally order and wording.
    const ALL: [(Activity, &'static str); 4] = [
        (Activity::Work, "work"),
        (Activity::Wait, "wait"),
        (Activity::Idle, "idle"),
        (Activity::Off, "off"),
    ];
}

/// The activity group of a status.
pub fn activity(status: Status) -> Activity {
    if needs_attention(status) {
        return Activity::Wait;
    }
    match status {
        Status::Starting | Status::Running | Status::Unknown => Activity::Work,
        Status::Idle => Activity::Idle,
        Status::Permission | Status::YourTurn => Activity::Wait,
        Status::Exited(_) | Status::Closed => Activity::Off,
    }
}

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
    Activity::ALL
        .iter()
        .filter_map(|(a, word)| {
            let n = entries
                .iter()
                .filter(|e| activity(e.info.status) == *a)
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
    fn activity_maps_every_status() {
        let table = [
            (Status::Starting, Activity::Work),
            (Status::Running, Activity::Work),
            (Status::Unknown, Activity::Work),
            (Status::Permission, Activity::Wait),
            (Status::YourTurn, Activity::Wait),
            (Status::Idle, Activity::Idle),
            (Status::Exited(3), Activity::Off),
            (Status::Closed, Activity::Off),
        ];
        for (status, want) in table {
            assert_eq!(activity(status), want, "{status:?}");
        }
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
        let ls = lines(&es, 30, 14);
        assert_eq!(
            texts(&ls),
            vec![
                "1 work · 1 wait · 1 idle",
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
    fn exited_counts_as_off_and_empty_groups_are_left_out() {
        let infos = [info("api", None, Status::Exited(2))];
        let ls = lines(&entries(&infos, &[None]), 30, 14);
        assert_eq!(text(&ls[0]), "1 off");
        assert_eq!(text(&ls[1]), "1 ✗ api  exited 2");
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
