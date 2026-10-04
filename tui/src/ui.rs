//! Drawing: top line, side bar, list, mail, bottom line, and the list of keys.

use chrono::Local;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, Focus, Mode, Place, Row, Split},
    text::{ACCENT, SIGNAL, SOFT, cell, conversation, fit, short_date},
};

const SIDE_WIDTH: u16 = 20;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [top, middle, bottom] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area);

    let title = Line::from(vec![
        Span::styled(
            " Portier ",
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::styled(
            format!("  {}", app.place.name()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), top);
    let user = Span::styled(format!("{} ", app.user), Style::default().fg(SOFT));
    frame.render_widget(Paragraph::new(Line::from(user)).right_aligned(), top);

    // A narrow terminal has no room for the side bar; the letters still reach every list.
    let main = if area.width >= 70 {
        let [side, main] = Layout::horizontal([Constraint::Length(SIDE_WIDTH), Constraint::Min(1)]).areas(middle);
        side_bar(frame, app, side);
        main
    } else {
        middle
    };

    match (&app.open, app.split) {
        (None, _) => list(frame, app, main),
        (Some(_), Split::Off) => mail(frame, app, main, false),
        (Some(_), Split::Beside) if main.width >= 70 => {
            let width = (main.width * 2 / 5).clamp(30, 60);
            let [left, right] = Layout::horizontal([Constraint::Length(width), Constraint::Min(1)]).areas(main);
            list(frame, app, left);
            mail(frame, app, right, true);
        }
        // Below the list; also where side by side has no room, as in the web client.
        (Some(_), _) => {
            let height = (main.height * 2 / 5).clamp(4, 14);
            let [upper, lower] = Layout::vertical([Constraint::Length(height), Constraint::Min(1)]).areas(main);
            list(frame, app, upper);
            mail(frame, app, lower, false);
        }
    }

    let hint = if app.status.is_empty() {
        // As many hints as fit, the most needed first.
        let hints: &[&str] = if app.selecting {
            &[
                "Space tick",
                "* all",
                "a archive",
                "d trash",
                "u unseen",
                "i important",
                "z delay",
                "m move",
                "Esc stop",
            ]
        } else if app.place == Place::Screener {
            &[
                "? keys",
                "Enter read",
                "i Home",
                "n Nice to know",
                "J Junk",
                "Esc close",
                "q quit",
            ]
        } else if app.open.is_some() {
            &[
                "? keys",
                "↑↓ mail",
                "Space page",
                "a archive",
                "d trash",
                "u unseen",
                "i important",
                "z delay",
                "t note",
                "Esc close",
            ]
        } else {
            &[
                "? keys",
                "↑↓ mail",
                "Enter open",
                "a archive",
                "d trash",
                "t note",
                "v select",
                "s split",
                "Tab side bar",
                "q quit",
            ]
        };
        let mut text = String::new();
        for hint in hints {
            if text.width() + hint.width() + 3 > area.width as usize {
                break;
            }
            text.push_str("  ");
            text.push_str(hint);
        }
        Line::styled(text.split_off(1), Style::default().fg(SOFT))
    } else {
        Line::raw(format!(" {}", fit(&app.status, area.width.saturating_sub(2) as usize)))
    };
    frame.render_widget(Paragraph::new(hint), bottom);

    match &app.mode {
        Mode::Note { text, .. } => {
            // The note is written on the bottom line, with the cursor at its end.
            let label = " Note: ";
            let room = (area.width as usize).saturating_sub(label.width() + 1);
            // The end of a long note stays in view while typing.
            let mut shown = text.clone();
            while shown.width() > room {
                shown.remove(0);
            }
            let line = Line::from(vec![
                Span::styled(label, Style::default().fg(SIGNAL).add_modifier(Modifier::BOLD)),
                Span::raw(shown.clone()),
            ]);
            frame.render_widget(Clear, bottom);
            frame.render_widget(Paragraph::new(line), bottom);
            frame.set_cursor_position((bottom.x + (label.width() + shown.width()) as u16, bottom.y));
        }
        Mode::Folders { names, cursor, .. } => folders(frame, area, names, *cursor),
        Mode::Normal | Mode::Delay => {}
    }

    if app.help {
        help(frame, area);
    }
}

fn side_bar(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize - 1;
    let mut lines = Vec::new();
    for place in Place::ALL {
        if place.starts_group() {
            lines.push(Line::styled("─".repeat(width), Style::default().fg(SOFT)));
        }
        let (count, loud) = place.badge(&app.counts);
        let badge = if count > 0 { count.to_string() } else { String::new() };
        let name = cell(&format!(" {}", place.name()), width.saturating_sub(badge.len() + 1));
        let here = place == app.place;
        let mut style = Style::default();
        if here {
            style = style.add_modifier(Modifier::BOLD);
            if app.focus == Focus::Side {
                style = style.add_modifier(Modifier::REVERSED);
            }
        } else {
            style = style.fg(SOFT);
        }
        let badge_style = if loud {
            Style::default().fg(SIGNAL).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(SOFT)
        };
        lines.push(Line::from(vec![
            Span::styled(if here { "▌" } else { " " }, Style::default().fg(ACCENT)),
            Span::styled(name.chars().skip(1).collect::<String>(), style),
            Span::styled(format!("{badge} "), badge_style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// One mail of a list as a single line: mark, who, number of mails, subject, start of the
/// text or your note, date.
fn mail_row(
    thread: &crate::api::ThreadSummary,
    width: usize,
    now: chrono::DateTime<Local>,
    open: bool,
) -> Line<'static> {
    let unseen = thread.unread > 0;
    let strong = if unseen {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let soft = Style::default().fg(SOFT);
    let date = short_date(thread.date, now);
    let who_width = if width >= 70 { 20 } else { 14 };
    let count = if thread.count > 1 {
        format!("{} ", thread.count)
    } else {
        String::new()
    };
    let subject = if thread.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        thread.subject.clone()
    };
    // What is left between the sender and the date is shared by subject and preview.
    let room = width.saturating_sub(2 + who_width + 1 + count.width() + date.width() + 2);
    let subject = fit(&subject, room);
    let rest = room.saturating_sub(subject.width() + 2);
    let mut spans = vec![
        Span::styled(
            if open {
                "▶ "
            } else if unseen {
                "● "
            } else {
                "  "
            },
            Style::default().fg(ACCENT),
        ),
        Span::styled(cell(&thread.who(), who_width), strong),
        Span::raw(" "),
        Span::styled(count, soft),
        Span::styled(subject.clone(), strong),
    ];
    let mut used = subject.width();
    if rest >= 6 {
        let (text, style) = if let Some(until) = thread.snoozed_until {
            // A delayed mail says when it comes back, as the web list does.
            (
                format!("Returns {}", crate::text::full_date(until)),
                Style::default().fg(Color::Green),
            )
        } else if thread.note.is_empty() {
            let preview = if thread.is_outgoing {
                format!("You: {}", thread.snippet)
            } else {
                thread.snippet.clone()
            };
            (preview, soft)
        } else {
            (format!("✎ {}", thread.note), Style::default().fg(SIGNAL))
        };
        let text = fit(&text, rest);
        used += 2 + text.width();
        spans.push(Span::raw("  "));
        spans.push(Span::styled(text, style));
    }
    spans.push(Span::raw(" ".repeat(room.saturating_sub(used) + 1)));
    spans.push(Span::styled(date, soft));
    Line::from(spans)
}

fn list(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;
    let height = area.height as usize;
    let now = Local::now();
    let open_id = app.open.as_ref().map(|open| open.thread.id);
    // Keep the cursor in view, with the heading of its area when that fits.
    let first = if app.cursor < height {
        0
    } else {
        app.cursor + 1 - height
    };
    let mut lines = Vec::new();
    for (index, row) in app.rows.iter().enumerate().skip(first).take(height) {
        let mut line = match row {
            Row::Header { title, count } => Line::from(vec![
                Span::styled(
                    format!(" {title} "),
                    Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
                Span::styled(format!(" {count}"), Style::default().fg(SOFT)),
            ]),
            Row::Mail(thread) => {
                let mut line = mail_row(
                    thread,
                    width.saturating_sub(if app.selecting { 4 } else { 0 }),
                    now,
                    open_id == Some(thread.id),
                );
                if app.selecting {
                    // The tick of the selection, in front of the row.
                    let ticked = app.selected.contains(&thread.id);
                    let style = if ticked {
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(SOFT)
                    };
                    line.spans
                        .insert(0, Span::styled(if ticked { "[x] " } else { "[ ] " }, style));
                }
                line
            }
            Row::Entry { who, text, date, .. } => {
                let date = short_date(*date, now);
                let room = width.saturating_sub(2 + 28 + 1 + date.width() + 1);
                Line::from(vec![
                    Span::raw("  "),
                    Span::raw(cell(who, 28)),
                    Span::raw(" "),
                    Span::styled(cell(text, room), Style::default().fg(SOFT)),
                    Span::raw(" "),
                    Span::styled(date, Style::default().fg(SOFT)),
                ])
            }
            Row::Empty(text) => Line::styled(format!("   {text}"), Style::default().fg(SOFT)),
        };
        if index == app.cursor && app.focus == Focus::List && matches!(row, Row::Mail(_) | Row::Entry { .. }) {
            line = line.style(Style::default().bg(Color::Indexed(238)));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn mail(frame: &mut Frame, app: &mut App, area: Rect, beside: bool) {
    // A line between the list and the mail, on the side they meet.
    let block = Block::default()
        .borders(if beside {
            Borders::LEFT
        } else if app.split == Split::Off {
            Borders::NONE
        } else {
            Borders::TOP
        })
        .border_style(Style::default().fg(SOFT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let Some(open) = &mut app.open else { return };

    // Text is easier to read in a column than across a very wide terminal.
    let width = (inner.width as usize).min(100);
    let lines = conversation(&open.thread, width, open.all, &open.unseen_at_open);
    let height = inner.height as usize;
    app.page = height;
    app.lines = lines.len();
    open.scroll = open.scroll.min(lines.len().saturating_sub(height));
    let shown: Vec<Line> = lines.into_iter().skip(open.scroll).take(height).collect();
    frame.render_widget(Paragraph::new(shown), inner);

    // Where in the mail the reader is, when it does not fit.
    if app.lines > height {
        let seen = ((open.scroll + height) * 100 / app.lines).min(100);
        let mark = Span::styled(format!(" {seen}% "), Style::default().fg(SOFT));
        let spot = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };
        frame.render_widget(Paragraph::new(Line::from(mark)).right_aligned(), spot);
    }
}

const KEYS: &[(&str, &[(&str, &str)])] = &[
    (
        "Everywhere",
        &[
            ("?", "Show this list"),
            ("Shift H", "Home (also 1)"),
            ("Shift I", "Important"),
            ("Shift D", "Delayed"),
            ("Shift N", "Nice to know (also 3)"),
            ("2", "Screener"),
            ("Tab", "Side bar and back; ↑ ↓ choose a list there"),
            ("s", "Split view: beside, below, off"),
            ("Ctrl L", "Load again (also F5)"),
            ("q", "Quit"),
        ],
    ),
    (
        "In a mail list",
        &[
            ("↓ ↑", "Next, previous mail (also j, k); opens it in split view"),
            ("Enter", "Open the mail"),
        ],
    ),
    (
        "Acting on mail: opened, under the cursor, or ticked",
        &[
            ("a", "Archive"),
            ("d", "Move to Trash"),
            ("b", "Move back to Home, out of the Trash"),
            ("u", "Mark as unseen; an unseen mail in the list: as seen"),
            ("i", "Move to Important, or back to Home"),
            ("z", "Delay, then 1 2 3 7 for the days; delayed mail: back to Home"),
            ("t", "Add or edit your note (Enter saves, Esc cancels)"),
            ("m", "Move to a folder of the mail account"),
            ("v", "Select several: Space ticks, * ticks all, Esc stops"),
        ],
    ),
    (
        "Screener",
        &[
            ("Enter", "Read the sender's newest mail"),
            ("i", "Sender goes to Home"),
            ("n", "Sender is nice to know"),
            ("Shift J", "Sender is junk"),
        ],
    ),
    (
        "Reading a mail",
        &[
            ("↓ ↑", "Next, previous mail"),
            ("Space", "Page down (also Ctrl ↓, PgDn)"),
            ("Backspace", "Page up (also Ctrl ↑, PgUp)"),
            ("x", "All mails of the conversation in full, or only the newest"),
            ("o", "Open the mail in the web browser"),
            ("Esc", "Close the mail"),
        ],
    ),
];

fn help(frame: &mut Frame, area: Rect) {
    let mut lines = Vec::new();
    for (title, keys) in KEYS {
        lines.push(Line::styled(*title, Style::default().fg(SOFT)));
        for (key, what) in *keys {
            lines.push(Line::from(vec![
                Span::styled(cell(key, 11), Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(*what),
            ]));
        }
        lines.push(Line::raw(""));
    }
    lines.pop();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keyboard shortcuts ")
        .title_bottom(" any key closes ");
    // One column when it fits the height, else two side by side.
    let tall = lines.len() as u16 + 2;
    if tall <= area.height || area.width < 120 {
        let popup = centered(area, 84.min(area.width.saturating_sub(2)), tall.min(area.height));
        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(lines).block(block), popup);
        return;
    }
    // Split at the blank line nearest the middle, so no group is torn apart.
    let middle = lines.len() / 2;
    let cut = (0..lines.len())
        .filter(|&index| lines[index].spans.iter().all(|span| span.content.is_empty()))
        .min_by_key(|&index| index.abs_diff(middle))
        .unwrap_or(middle);
    let right = lines.split_off(cut + 1);
    lines.pop();
    let height = (lines.len().max(right.len()) as u16 + 2).min(area.height);
    let popup = centered(area, 150.min(area.width.saturating_sub(2)), height);
    frame.render_widget(Clear, popup);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);
    frame.render_widget(Paragraph::new(lines), left_area);
    frame.render_widget(Paragraph::new(right), right_area);
}

/// The folders of the mail account, to choose where mail moves to.
fn folders(frame: &mut Frame, area: Rect, names: &[String], cursor: usize) {
    let height = (names.len() as u16 + 2).min(area.height.saturating_sub(2)).max(3);
    let popup = centered(area, 50.min(area.width.saturating_sub(2)), height);
    let rows = height.saturating_sub(2) as usize;
    let first = if cursor < rows { 0 } else { cursor + 1 - rows };
    let lines: Vec<Line> = names
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .map(|(index, name)| {
            let line = Line::raw(format!(" {name}"));
            if index == cursor {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Move to folder ")
        .title_bottom(" Enter moves, Esc cancels ");
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [_, row, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(height), Constraint::Fill(1)])
        .areas(area);
    let [_, cell, _] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Fill(1), Constraint::Length(width), Constraint::Fill(1)])
        .areas(row);
    cell
}
