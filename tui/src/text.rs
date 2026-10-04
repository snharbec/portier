//! Turning mail into lines of terminal text: dates, wrapping, and the text of a conversation.

use std::collections::HashSet;

use chrono::{DateTime, Datelike, Local, TimeZone};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::api::{Message, Thread, display_name};

pub const SOFT: Color = Color::DarkGray;
pub const ACCENT: Color = Color::Blue;
pub const SIGNAL: Color = Color::Yellow;

fn local(timestamp: i64) -> DateTime<Local> {
    Local.timestamp_opt(timestamp, 0).earliest().unwrap_or_else(Local::now)
}

/// Time today, weekday this week, otherwise a date: as the web lists show it.
pub fn short_date(timestamp: i64, now: DateTime<Local>) -> String {
    let date = local(timestamp);
    if date.date_naive() == now.date_naive() {
        date.format("%H:%M").to_string()
    } else if (now - date).num_days() < 6 && date <= now {
        date.format("%a").to_string()
    } else if date.year() == now.year() {
        date.format("%-d %b").to_string()
    } else {
        date.format("%-d %b %y").to_string()
    }
}

pub fn full_date(timestamp: i64) -> String {
    local(timestamp).format("%a %-d %b %Y, %H:%M").to_string()
}

/// Cuts `text` to at most `width` terminal columns, with an ellipsis when something is lost.
pub fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

/// `fit`, then padded with spaces to exactly `width` columns.
pub fn cell(text: &str, width: usize) -> String {
    let fitted = fit(text, width);
    let pad = width.saturating_sub(fitted.width());
    format!("{fitted}{}", " ".repeat(pad))
}

/// Wraps plain text at word boundaries; words longer than a line are broken.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let paragraph = paragraph.trim_end();
        if paragraph.width() <= width {
            lines.push(paragraph.to_string());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let mut word = word.to_string();
            while word.width() > width {
                // A word (often a link) wider than the line: fill up, then break it.
                let room = width.saturating_sub(current.width() + usize::from(!current.is_empty()));
                let mut head = String::new();
                let mut used = 0;
                for c in word.chars() {
                    let w = c.width().unwrap_or(0);
                    if used + w > room.max(1) {
                        break;
                    }
                    head.push(c);
                    used += w;
                }
                let rest = word[head.len()..].to_string();
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(&head);
                lines.push(std::mem::take(&mut current));
                word = rest;
            }
            if current.is_empty() {
                current = word;
            } else if current.width() + 1 + word.width() <= width {
                current.push(' ');
                current.push_str(&word);
            } else {
                lines.push(std::mem::replace(&mut current, word));
            }
        }
        lines.push(current);
    }
    lines
}

/// The body of a mail as text lines. A mail written as HTML is rendered to text, with its
/// links listed; one written as text is wrapped.
pub fn body_lines(message: &Message, width: usize) -> Vec<String> {
    let width = width.max(20);
    let text = if message.body_html.trim().is_empty() {
        None
    } else {
        html2text::from_read(message.body_html.as_bytes(), width).ok()
    };
    let mut lines: Vec<String> = match text {
        Some(text) => text.lines().map(|line| line.trim_end().to_string()).collect(),
        None => wrap(&message.body_text, width),
    };
    // Mail likes to end in blank lines, and to stack several of them.
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines
}

fn size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", (bytes as f64 / 1024.0).round())
    } else {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

/// A whole conversation as styled lines, `width` columns wide. Like the web view, the last
/// mail and those that were unseen when it was opened are shown in full; earlier ones as one
/// line each unless `all` asks for everything.
pub fn conversation(thread: &Thread, width: usize, all: bool, unseen_at_open: &HashSet<i64>) -> Vec<Line<'static>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let soft = Style::default().fg(SOFT);
    let mut lines: Vec<Line<'static>> = Vec::new();

    let subject = if thread.subject.is_empty() {
        "(no subject)"
    } else {
        &thread.subject
    };
    for line in wrap(subject, width) {
        lines.push(Line::styled(line, bold));
    }
    if let Some(sender) = &thread.sender {
        let place = match sender.category.as_deref() {
            Some("important") => "Home",
            Some("feed") => "Nice to know",
            Some("junk") => "Junk",
            _ => "Not screened yet",
        };
        lines.push(Line::from(vec![
            Span::raw(display_name(Some(&sender.display_name), Some(&sender.address))),
            Span::styled(format!("  <{}>  ", sender.address), soft),
            Span::styled(format!("[{place}]"), Style::default().fg(ACCENT)),
        ]));
    }
    if let Some(until) = thread.snoozed_until {
        lines.push(Line::styled(
            format!("Delayed. Returns to Home {}.", full_date(until)),
            Style::default().fg(Color::Green),
        ));
    }
    if !thread.note.is_empty() {
        for (index, line) in wrap(&thread.note, width.saturating_sub(2)).into_iter().enumerate() {
            let mark = if index == 0 { "▌ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(mark, Style::default().fg(SIGNAL)),
                Span::styled(line, Style::default().fg(SIGNAL)),
            ]));
        }
    }

    let last = thread.messages.len().saturating_sub(1);
    for (index, message) in thread.messages.iter().enumerate() {
        lines.push(Line::styled("─".repeat(width), soft));
        let who = if message.is_outgoing {
            "You".to_string()
        } else {
            display_name(Some(&message.from.name), Some(&message.from.address))
        };
        let date = full_date(message.date);
        let open = all || index == last || unseen_at_open.contains(&message.id);
        if !open {
            let preview = message.body_text.split_whitespace().collect::<Vec<_>>().join(" ");
            let room = width.saturating_sub(who.width() + date.width() + 6);
            lines.push(Line::from(vec![
                Span::styled("▸ ", soft),
                Span::raw(who),
                Span::styled(format!("  {}  ", fit(&preview, room)), soft),
                Span::styled(date, soft),
            ]));
            continue;
        }
        let gap = width.saturating_sub(who.width() + date.width());
        lines.push(Line::from(vec![
            Span::styled(who, bold),
            Span::raw(" ".repeat(gap.max(1))),
            Span::styled(date, soft),
        ]));
        let recipients: Vec<String> = message
            .to
            .iter()
            .chain(&message.cc)
            .map(|a| display_name(Some(&a.name), Some(&a.address)))
            .collect();
        lines.push(Line::styled(
            fit(&format!("{} to {}", message.from.address, recipients.join(", ")), width),
            soft,
        ));
        lines.push(Line::raw(""));
        for line in body_lines(message, width) {
            // Quoted text steps back, as it does in the web view.
            let quoted = line.trim_start().starts_with('>');
            lines.push(if quoted {
                Line::styled(line, soft)
            } else {
                Line::raw(line)
            });
        }
        if !message.attachments.is_empty() {
            lines.push(Line::raw(""));
            for file in &message.attachments {
                lines.push(Line::from(vec![
                    Span::styled("📎 ", soft),
                    Span::raw(file.filename.clone()),
                    Span::styled(format!("  {}", size(file.size)), soft),
                ]));
            }
        }
    }
    lines
}

/// The mails of a list one below the other, each in full, as the web client's
/// "Read all on one page" shows them.
pub fn page(messages: &[Message], width: usize) -> Vec<Line<'static>> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let soft = Style::default().fg(SOFT);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for message in messages {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled("━".repeat(width), soft));
        let subject = if message.subject.is_empty() {
            "(no subject)"
        } else {
            &message.subject
        };
        for line in wrap(subject, width) {
            lines.push(Line::styled(line, bold));
        }
        let who = if message.is_outgoing {
            "You".to_string()
        } else {
            display_name(Some(&message.from.name), Some(&message.from.address))
        };
        let date = full_date(message.date);
        let mark = if message.seen || message.is_outgoing {
            ""
        } else {
            "● "
        };
        let gap = width.saturating_sub(mark.width() + who.width() + date.width());
        lines.push(Line::from(vec![
            Span::styled(mark, Style::default().fg(ACCENT)),
            Span::raw(who),
            Span::raw(" ".repeat(gap.max(1))),
            Span::styled(date, soft),
        ]));
        lines.push(Line::raw(""));
        for line in body_lines(message, width) {
            let quoted = line.trim_start().starts_with('>');
            lines.push(if quoted {
                Line::styled(line, soft)
            } else {
                Line::raw(line)
            });
        }
        for file in &message.attachments {
            lines.push(Line::from(vec![
                Span::styled("📎 ", soft),
                Span::raw(file.filename.clone()),
                Span::styled(format!("  {}", size(file.size)), soft),
            ]));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_keeps_words_and_breaks_long_ones() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(wrap("a\n\nb", 20), ["a", "", "b"]);
        let link = wrap("see https://example.org/a/very/long/path/that/never/ends now", 20);
        assert!(link.iter().all(|line| line.width() <= 20), "{link:?}");
        assert_eq!(
            link.join("").replace(' ', ""),
            "seehttps://example.org/a/very/long/path/that/never/endsnow"
        );
    }

    #[test]
    fn fitting_counts_columns_not_bytes() {
        assert_eq!(fit("Grüße aus Köln", 8), "Grüße a…");
        assert_eq!(cell("ab", 4), "ab  ");
        assert_eq!(cell("日本語のメール", 7).width(), 7);
    }

    #[test]
    fn dates_shorten_with_distance() {
        let now = Local.with_ymd_and_hms(2026, 10, 4, 12, 0, 0).unwrap();
        let at = |y, m, d, h| Local.with_ymd_and_hms(y, m, d, h, 30, 0).unwrap().timestamp();
        assert_eq!(short_date(at(2026, 10, 4, 9), now), "09:30");
        assert_eq!(short_date(at(2026, 10, 2, 9), now), "Fri");
        assert_eq!(short_date(at(2026, 7, 25, 9), now), "25 Jul");
        assert_eq!(short_date(at(2025, 7, 25, 9), now), "25 Jul 25");
    }

    #[test]
    fn html_mail_becomes_text_with_its_links() {
        let message: Message = serde_json::from_value(serde_json::json!({
            "id": 1, "from": { "name": "A", "address": "a@example.org" }, "to": [], "cc": [],
            "date": 0, "seen": true, "is_outgoing": false, "body_text": "ignored",
            "body_html": "<h1>Offer</h1><p>Read <a href=\"https://example.org/x\">more</a>.</p><p></p><p></p>",
            "attachments": []
        }))
        .unwrap();
        let text = body_lines(&message, 60).join("\n");
        assert!(
            text.contains("Offer") && text.contains("more") && text.contains("https://example.org/x"),
            "{text}"
        );
        assert!(!text.contains('<'));
    }
}
