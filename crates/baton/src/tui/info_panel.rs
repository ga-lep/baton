//! The session info panel (spec section 4): path, profile, model, status,
//! uptime, context, tokens, cost and ids of the selected session.

use baton_core::transcript::sanitize_model;
use baton_proto::{QuotaWindow, SessionInfo};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::labels::status_label;

/// Number of cells in the context bar.
const BAR_CELLS: usize = 10;
/// Width of the label column (`"context  "`).
const LABEL_WIDTH: usize = 9;

/// Uptime such as `42s`, `3m07s` or `1h12m`.
pub fn uptime(started_at: u64, now: u64) -> String {
    let secs = now.saturating_sub(started_at);
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
    }
}

/// Token count such as `999`, `2.0k`, `84k`, `1.2M`.
pub fn compact(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => format!("{:.1}k", n as f64 / 1_000.0),
        10_000..1_000_000 => format!("{}k", n / 1_000),
        1_000_000..10_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        _ => format!("{}M", n / 1_000_000),
    }
}

/// `███████░░░ 68%`; `None` for a value that cannot be shown.
pub fn context_bar(pct: f32) -> Option<String> {
    if !pct.is_finite() || pct < 0.0 {
        return None;
    }
    let filled = ((pct / 10.0).round() as usize).min(BAR_CELLS);
    Some(format!(
        "{}{} {pct:.0}%",
        "█".repeat(filled),
        "░".repeat(BAR_CELLS - filled)
    ))
}

/// Time left until `at`, such as `47m`, `2h13m` or `3d04h`; `None` once past.
pub fn until(at: u64, now: u64) -> Option<String> {
    let secs = at.checked_sub(now).filter(|s| *s > 0)?;
    Some(match secs {
        0..3600 => format!("{}m", secs.div_ceil(60)),
        3600..86_400 => format!("{}h{:02}m", secs / 3600, secs % 3600 / 60),
        _ => format!("{}d{:02}h", secs / 86_400, secs % 86_400 / 3600),
    })
}

/// `██░░░░░░░░ 23% ↻2h13m` for a window that has not reset yet.
pub fn quota_bar(w: &QuotaWindow, now: u64) -> Option<String> {
    Some(format!(
        "{} ↻{}",
        context_bar(w.used_pct)?,
        until(w.resets_at, now)?
    ))
}

/// `~$4.10 (est.)`; sub-cent amounts get four decimals. `None` if unusable.
pub fn cost(usd: f64) -> Option<String> {
    if !usd.is_finite() || usd < 0.0 {
        return None;
    }
    Some(if usd >= 0.01 {
        format!("~${usd:.2} (est.)")
    } else {
        format!("~${usd:.4} (est.)")
    })
}

/// `7f3c…a91e` for a long id, the id itself when short.
pub fn abbreviate_id(id: &str) -> String {
    let id = clean(id);
    let n = id.chars().count();
    if n <= 12 {
        return id;
    }
    let head: String = id.chars().take(4).collect();
    let tail: String = id.chars().skip(n - 4).collect();
    format!("{head}…{tail}")
}

/// Replaces a leading `home` by `~` and shortens to `width` cells with a
/// leading `…`. Control characters are dropped.
pub fn fit_path(path: &str, home: Option<&str>, width: usize) -> String {
    let path = clean(path);
    let shown = match home.filter(|h| h.len() > 1) {
        Some(h) if path == h => "~".to_owned(),
        Some(h) if path.strip_prefix(h).is_some_and(|r| r.starts_with('/')) => {
            format!("~{}", &path[h.len()..])
        }
        _ => path,
    };
    let n = shown.chars().count();
    if n <= width {
        return shown;
    }
    match width {
        0 => String::new(),
        w => format!("…{}", shown.chars().skip(n - (w - 1)).collect::<String>()),
    }
}

/// The panel's lines for `s`, at most `width` cells wide.
pub fn lines(
    s: &SessionInfo,
    now: u64,
    home: Option<&str>,
    width: usize,
    notice: Option<&str>,
) -> Vec<Line<'static>> {
    let value_width = width.saturating_sub(LABEL_WIDTH);
    let row = |label: &str, value: Span<'static>| {
        let text = clip(&value.content, value_width);
        Line::from(vec![
            Span::styled(
                format!("{label:<LABEL_WIDTH$}"),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(text, value.style),
        ])
    };
    let plain = |text: String| Span::raw(text);
    let na = || Span::styled("n/a".to_owned(), Style::default().fg(Color::DarkGray));
    let usage = s.usage.as_ref();
    let model = usage
        .and_then(|u| u.model.as_deref())
        .or(s.model.as_deref())
        .and_then(sanitize_model);

    let mut out = vec![
        Line::from(fit_path(&s.repo, home, width)),
        row(
            "profile",
            plain(clean(s.profile.as_deref().unwrap_or("default"))),
        ),
        row("model", model.map_or_else(na, plain)),
        row("status", plain(status_label(s.status))),
        row("uptime", plain(uptime(s.started_at, now))),
    ];
    out.push(row(
        "context",
        usage
            .and_then(|u| u.context_pct)
            .and_then(context_bar)
            .map_or_else(na, |bar| {
                let hot = usage.and_then(|u| u.context_pct).is_some_and(|p| p >= 90.0);
                let color = if hot { Color::Red } else { Color::Reset };
                Span::styled(bar, Style::default().fg(color))
            }),
    ));
    out.push(row(
        "tokens",
        usage.map_or_else(na, |u| {
            plain(format!(
                "{} in / {} out",
                compact(u.input),
                compact(u.output)
            ))
        }),
    ));
    out.push(row(
        "cache",
        usage.map_or_else(na, |u| {
            plain(format!(
                "{} read / {} write",
                compact(u.cache_read),
                compact(u.cache_write)
            ))
        }),
    ));
    out.push(row(
        "cost",
        usage
            .and_then(|u| u.cost_usd)
            .and_then(cost)
            .map_or_else(na, plain),
    ));
    // Claude drops a window once it resets; so do we, until the next report.
    for (label, window) in [
        ("5h", s.quota.and_then(|q| q.five_hour)),
        ("week", s.quota.and_then(|q| q.seven_day)),
    ] {
        let bar = window.and_then(|w| Some((quota_bar(&w, now)?, w.used_pct)));
        out.push(row(
            label,
            bar.map_or_else(na, |(bar, pct)| {
                let color = match pct {
                    p if p >= 90.0 => Color::Red,
                    p if p >= 75.0 => Color::Yellow,
                    _ => Color::Reset,
                };
                Span::styled(bar, Style::default().fg(color))
            }),
        ));
    }
    out.push(row(
        "id",
        s.claude_session_id
            .as_deref()
            .map_or_else(na, |id| plain(abbreviate_id(id))),
    ));
    if let Some(launch) = &s.launch {
        out.push(row("launch", plain(clean(launch))));
    }
    if let Some(msg) = notice {
        out.push(Line::from(format!("! {}", clean(msg))));
    }
    out
}

/// Drops control characters and invisible formatting characters (bidi
/// overrides, zero-width marks) from text that came from outside the TUI.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(*c,
                    '\u{ad}' | '\u{61c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
                    | '\u{2060}'..='\u{206f}' | '\u{feff}')
        })
        .collect()
}

/// At most `width` characters of `s`.
fn clip(s: &str, width: usize) -> String {
    s.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use baton_proto::{Quota, SessionId, Status, Usage};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::{Block, Borders, Paragraph};

    fn session(usage: Option<Usage>) -> SessionInfo {
        SessionInfo {
            id: SessionId("x//home/u/code/powerloop".into()),
            project: "x".into(),
            repo: "/home/u/code/powerloop".into(),
            profile: Some("work".into()),
            status: Status::Running,
            claude_session_id: Some("7f3c1111-2222-3333-4444-5555666aa91e".into()),
            transcript_path: None,
            model: Some("opus".into()),
            started_at: 0,
            exit_code: None,
            usage,
            launch: Some("resume".into()),
            quota: None,
        }
    }

    fn full_usage() -> Usage {
        Usage {
            input: 1_200_000,
            output: 84_000,
            cache_read: 2_000,
            cache_write: 20,
            context_pct: Some(68.0),
            cost_usd: Some(4.1),
            model: Some("claude-opus-5-5".into()),
        }
    }

    /// Draws the panel in a bordered box of the given size.
    fn render(s: &SessionInfo, now: u64, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).expect("terminal");
        t.draw(|f| {
            let text = lines(
                s,
                now,
                Some("/home/u"),
                usize::from(w.saturating_sub(2)),
                None,
            );
            f.render_widget(
                Paragraph::new(text).block(Block::default().borders(Borders::ALL)),
                f.area(),
            );
        })
        .expect("draw");
        let buf = t.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn full_state_matches_the_spec_layout() {
        let out = render(&session(Some(full_usage())), 4320, 32, 16);
        for want in [
            "~/code/powerloop",
            "profile  work",
            "model    claude-opus-5-5",
            "status   running",
            "uptime   1h12m",
            "context  ███████░░░ 68%",
            "tokens   1.2M in / 84k out",
            "cache    2.0k read / 20 write",
            "cost     ~$4.10 (est.)",
            "id       7f3c…a91e",
            "launch   resume",
        ] {
            assert!(out.contains(want), "missing {want:?} in\n{out}");
        }
    }

    #[test]
    fn quota_rows_show_bars_and_time_to_reset() {
        let mut s = session(None);
        s.quota = Some(Quota {
            five_hour: Some(QuotaWindow {
                used_pct: 23.0,
                resets_at: 1000 + 2 * 3600 + 13 * 60,
            }),
            seven_day: Some(QuotaWindow {
                used_pct: 91.0,
                resets_at: 1000 + 3 * 86_400 + 4 * 3600,
            }),
        });
        let out = render(&s, 1000, 40, 16);
        assert!(out.contains("5h       ██░░░░░░░░ 23% ↻2h13m"), "{out}");
        assert!(out.contains("week     █████████░ 91% ↻3d04h"), "{out}");
    }

    #[test]
    fn missing_or_reset_quota_windows_show_n_a() {
        let mut s = session(None);
        assert!(render(&s, 0, 32, 16).contains("5h       n/a"));
        s.quota = Some(Quota {
            five_hour: Some(QuotaWindow {
                used_pct: 50.0,
                resets_at: 100,
            }),
            seven_day: None,
        });
        let out = render(&s, 100, 32, 16);
        assert!(out.contains("5h       n/a"), "{out}");
        assert!(out.contains("week     n/a"), "{out}");
    }

    #[test]
    fn time_until_reset() {
        assert_eq!(until(100, 100), None);
        assert_eq!(until(100, 200), None);
        assert_eq!(until(101, 100).as_deref(), Some("1m"));
        assert_eq!(until(100 + 47 * 60, 100).as_deref(), Some("47m"));
        assert_eq!(until(3600, 0).as_deref(), Some("1h00m"));
        assert_eq!(until(86_400 + 3600, 0).as_deref(), Some("1d01h"));
    }

    #[test]
    fn unavailable_usage_shows_n_a_and_the_rest_still_renders() {
        let out = render(&session(None), 5, 32, 16);
        for want in [
            "model    opus",
            "context  n/a",
            "tokens   n/a",
            "cache    n/a",
            "cost     n/a",
            "uptime   5s",
        ] {
            assert!(out.contains(want), "missing {want:?} in\n{out}");
        }
        assert!(!out.contains("est."), "{out}");
    }

    #[test]
    fn exited_sessions_show_the_exit_code() {
        let mut s = session(None);
        s.status = Status::Exited(3);
        assert!(render(&s, 0, 32, 16).contains("status   exited 3"));
    }

    #[test]
    fn hostile_text_cannot_reach_the_terminal() {
        let mut s = session(Some(Usage {
            model: Some("evil\u{1b}[2Jmodel\u{202e}".into()),
            ..full_usage()
        }));
        s.repo = "/tmp/a\u{1b}]0;x\u{7}b".into();
        let out = render(&s, 0, 40, 16);
        assert!(!out.contains('\u{1b}') && !out.contains('\u{202e}') && !out.contains('\u{7}'));
        assert!(out.contains("evil[2Jmodel"), "{out}");
    }

    #[test]
    fn tiny_widths_do_not_panic() {
        for w in [0, 1, 3, 10] {
            let text = lines(&session(Some(full_usage())), 0, None, w, Some("notice"));
            assert!(text.iter().all(|l| l.width() <= w.max(1) + LABEL_WIDTH));
        }
    }

    #[test]
    fn helpers_format_compactly() {
        assert_eq!(uptime(100, 105), "5s");
        assert_eq!(uptime(0, 187), "3m07s");
        assert_eq!(uptime(0, 4320), "1h12m");
        assert_eq!(uptime(50, 10), "0s");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(2000), "2.0k");
        assert_eq!(compact(84_000), "84k");
        assert_eq!(compact(1_200_000), "1.2M");
        assert_eq!(compact(12_500_000), "12M");
        assert_eq!(context_bar(68.0).as_deref(), Some("███████░░░ 68%"));
        assert_eq!(context_bar(0.0).as_deref(), Some("░░░░░░░░░░ 0%"));
        assert_eq!(context_bar(130.0).as_deref(), Some("██████████ 130%"));
        assert_eq!(context_bar(f32::NAN), None);
        assert_eq!(context_bar(-1.0), None);
        assert_eq!(cost(4.1).as_deref(), Some("~$4.10 (est.)"));
        assert_eq!(cost(0.002775).as_deref(), Some("~$0.0028 (est.)"));
        assert_eq!(cost(f64::NAN), None);
        assert_eq!(
            abbreviate_id("7f3c1111-2222-3333-4444-5555666aa91e"),
            "7f3c…a91e"
        );
        assert_eq!(abbreviate_id("short"), "short");
        assert_eq!(fit_path("/home/u/code/x", Some("/home/u"), 40), "~/code/x");
        assert_eq!(
            fit_path("/home/user2/x", Some("/home/u"), 40),
            "/home/user2/x"
        );
        assert_eq!(fit_path("/a/b/c/d/e/f", None, 6), "…d/e/f");
    }
}
