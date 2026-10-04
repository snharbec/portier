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
    app.drawn.clear();
    let [top, middle, bottom] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area);

    let title = Line::from(vec![
        Span::styled(
            " Portier ",
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::styled(
            format!("  {}", heading(app)),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), top);
    let user = Span::styled(format!("{} ", app.user), Style::default().fg(SOFT));
    frame.render_widget(Paragraph::new(Line::from(user)).right_aligned(), top);

    if app.compose.is_some() {
        // Writing takes the whole window.
        compose(frame, app, middle, bottom);
        return;
    }

    // A narrow terminal has no room for the side bar; the letters still reach every list.
    let main = if area.width >= 70 {
        let [side, main] = Layout::horizontal([Constraint::Length(SIDE_WIDTH), Constraint::Min(1)]).areas(middle);
        side_bar(frame, app, side);
        main
    } else {
        middle
    };

    match (&app.open, app.split) {
        _ if app.reading.is_some() => one_page(frame, app, main),
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
        let hints: &[&str] = if app.reading.is_some() {
            &["Space page", "↑↓ scroll", "Esc back to the list", "? keys", "q quit"]
        } else if app.selecting {
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
                "/ search",
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
        Mode::Note { text, .. } => input(frame, bottom, " Note: ", text),
        Mode::Search { text } => input(frame, bottom, " Search: ", text),
        Mode::Name { text } => input(frame, bottom, " Name for the side bar: ", text),
        Mode::Folders { names, cursor, .. } => folders(frame, area, names, *cursor),
        _ if app.files.is_some() || app.viewing.is_some() => {}
        Mode::Normal | Mode::Delay => {}
    }

    if let Some(files) = &app.files {
        attachments(frame, area, files, app.picker.is_some());
    }
    if app.viewing.is_some() {
        picture(frame, app, middle, bottom);
    }
    if app.help {
        help(frame, area);
    }
}

/// What the top line says the list is.
fn heading(app: &App) -> String {
    if let Some(compose) = &app.compose {
        return match compose.kind.as_str() {
            "reply" => "Reply",
            "reply_all" => "Reply to all",
            "forward" => "Forward",
            _ => "New message",
        }
        .to_string();
    }
    match app.place {
        Place::Search => match app.saved_here() {
            Some(saved) => format!("{}  (saved search: {})", saved.name, saved.query),
            None => format!("Search: {}", app.query),
        },
        place if app.is_narrowed() => format!("{}, unseen only", place.name()),
        place => place.name().to_string(),
    }
}

/// A line of text being typed, on the bottom line, with the cursor at its end.
fn input(frame: &mut Frame, bottom: Rect, label: &str, text: &str) {
    let room = (bottom.width as usize).saturating_sub(label.width() + 1);
    // The end of a long text stays in view while typing.
    let mut shown = text.to_string();
    while shown.width() > room {
        shown.remove(0);
    }
    let line = Line::from(vec![
        Span::styled(
            label.to_string(),
            Style::default().fg(SIGNAL).add_modifier(Modifier::BOLD),
        ),
        Span::raw(shown.clone()),
    ]);
    frame.render_widget(Clear, bottom);
    frame.render_widget(Paragraph::new(line), bottom);
    frame.set_cursor_position((bottom.x + (label.width() + shown.width()) as u16, bottom.y));
}

fn side_bar(frame: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize - 1;
    let mut lines = Vec::new();
    // Which line of the screen each entry is on, for the mouse.
    let mut targets = Vec::new();
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
        targets.push((area.y + lines.len() as u16, crate::app::SideTarget::Place(place)));
        lines.push(Line::from(vec![
            Span::styled(if here { "▌" } else { " " }, Style::default().fg(ACCENT)),
            Span::styled(name.chars().skip(1).collect::<String>(), style),
            Span::styled(format!("{badge} "), badge_style),
        ]));
    }
    // Saved searches, each with the number of unseen mails it finds.
    if !app.saved.is_empty() {
        lines.push(Line::styled("─".repeat(width), Style::default().fg(SOFT)));
        let here = app.saved_here().map(|saved| saved.id);
        for (index, saved) in app.saved.iter().enumerate() {
            targets.push((area.y + lines.len() as u16, crate::app::SideTarget::Saved(index)));
            let badge = if saved.unread > 0 {
                saved.unread.to_string()
            } else {
                String::new()
            };
            let name = cell(&format!("⌕ {}", saved.name), width.saturating_sub(badge.len() + 2));
            let chosen = here == Some(saved.id);
            let mut style = Style::default();
            if chosen {
                style = style.add_modifier(Modifier::BOLD);
                if app.focus == Focus::Side {
                    style = style.add_modifier(Modifier::REVERSED);
                }
            } else {
                style = style.fg(SOFT);
            }
            lines.push(Line::from(vec![
                Span::styled(if chosen { "▌" } else { " " }, Style::default().fg(ACCENT)),
                Span::styled(name, style),
                Span::styled(format!("{badge} "), Style::default().fg(SOFT)),
            ]));
        }
    }
    targets.retain(|(row, _)| *row < area.bottom());
    *app.drawn.side.borrow_mut() = targets;
    app.drawn.side_area.set(area);
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
    // A search result names the list it is found in, where a list shows the number of mails.
    let count = if !thread.tag.is_empty() {
        format!("[{}] ", thread.tag)
    } else if thread.count > 1 {
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

fn list(frame: &mut Frame, app: &mut App, area: Rect) {
    let width = area.width as usize;
    app.fit_briefing(width);
    let height = area.height as usize;
    app.list_width.set(width);
    let now = Local::now();
    let open_id = app.open.as_ref().map(|open| open.thread.id);
    // Keep the cursor in view, with the heading of its area when that fits.
    let first = if app.cursor < height {
        0
    } else {
        app.cursor + 1 - height
    };
    app.drawn.list.set(area);
    app.drawn.first.set(first);
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
            Row::Said { text, soft } => Line::styled(
                format!("    {text}"),
                if *soft {
                    Style::default().fg(SOFT)
                } else {
                    Style::default()
                },
            ),
        };
        if index == app.cursor && app.focus == Focus::List && matches!(row, Row::Mail(_) | Row::Entry { .. }) {
            line = line.style(Style::default().bg(Color::Indexed(238)));
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The form a mail is written in.
fn compose(frame: &mut Frame, app: &mut App, area: Rect, bottom: Rect) {
    use crate::compose::Field;
    let soft = Style::default().fg(SOFT);
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2).min(100),
        ..area
    };
    let width = area.width as usize;
    let status = app.status.clone();
    let Some(compose) = &mut app.compose else { return };

    // The original mail, read in place of the form.
    if compose.show_original {
        let lines = match &compose.original {
            Some(original) => crate::text::page(std::slice::from_ref(original), width),
            None => Vec::new(),
        };
        let height = area.height as usize;
        compose.original_scroll = compose.original_scroll.min(lines.len().saturating_sub(height));
        let (count, scroll) = (lines.len(), compose.original_scroll);
        let shown: Vec<Line> = lines.into_iter().skip(scroll).take(height).collect();
        frame.render_widget(Paragraph::new(shown), area);
        frame.render_widget(
            Paragraph::new(Line::styled(
                " Space page  ↑↓ scroll  any other key: back to your mail",
                soft,
            )),
            bottom,
        );
        app.page = height;
        app.lines = count;
        return;
    }

    const LABEL: usize = 9;
    let mut lines: Vec<Line> = Vec::new();
    let mut cursor: Option<(u16, u16)> = None;
    let field = |name: &str, value: String, here: bool, lines: &mut Vec<Line>, cursor: &mut Option<(u16, u16)>| {
        let room = width.saturating_sub(LABEL);
        // The end of a long value stays in view, where the typing happens.
        let mut shown = value;
        while shown.width() > room.saturating_sub(1) {
            shown.remove(0);
        }
        if here {
            *cursor = Some((area.x + (LABEL + shown.width()) as u16, area.y + lines.len() as u16));
        }
        let label = Style::default().fg(if here { ACCENT } else { SOFT });
        lines.push(Line::from(vec![
            Span::styled(cell(name, LABEL), label),
            Span::raw(shown),
        ]));
    };
    let from = match compose.accounts.get(compose.account) {
        Some(account) => format!("{} <{}>", account.label, account.address),
        None => "No mail account: add one in the web client".to_string(),
    };
    let from = if compose.field == Field::From && compose.accounts.len() > 1 {
        format!("{from}   (← → changes)")
    } else {
        from
    };
    let mut no_cursor = None;
    field("From", from, compose.field == Field::From, &mut lines, &mut no_cursor);
    field(
        "To",
        compose.to.clone(),
        compose.field == Field::To,
        &mut lines,
        &mut cursor,
    );
    let to_row = lines.len();
    field(
        "Cc",
        compose.cc.clone(),
        compose.field == Field::Cc,
        &mut lines,
        &mut cursor,
    );
    field(
        "Bcc",
        compose.bcc.clone(),
        compose.field == Field::Bcc,
        &mut lines,
        &mut cursor,
    );
    field(
        "Subject",
        compose.subject.clone(),
        compose.field == Field::Subject,
        &mut lines,
        &mut cursor,
    );
    lines.push(Line::styled("─".repeat(width), soft));
    let head = lines.len();

    // Below the text: what is sent along.
    let mut foot: Vec<Line> = Vec::new();
    let tick = |on: bool| if on { "[x] " } else { "[ ] " };
    let mark = |here: bool| Span::styled(if here { "▌" } else { " " }, Style::default().fg(ACCENT));
    if let Some(who) = &compose.source_from {
        foot.push(Line::from(vec![
            mark(compose.field == Field::Quote),
            Span::raw(format!(
                "{}Include the message from {who} below yours",
                tick(compose.quote)
            )),
        ]));
        if compose.kind == "forward" && compose.source_files > 0 {
            let files = if compose.source_files == 1 {
                "attachment".to_string()
            } else {
                format!("{} attachments", compose.source_files)
            };
            foot.push(Line::from(vec![
                mark(compose.field == Field::Files),
                Span::raw(format!("{}Forward its {files}", tick(compose.forward_files))),
            ]));
        }
    }
    for (index, file) in compose.files.iter().enumerate() {
        let here = compose.field == Field::Attachment(index);
        foot.push(Line::from(vec![
            mark(here),
            Span::raw(format!("📎 {}", file.filename)),
            Span::styled(
                format!(
                    "  {} KB{}",
                    (file.size + 1023) / 1024,
                    if here { "   (Backspace removes)" } else { "" }
                ),
                soft,
            ),
        ]));
    }
    if !foot.is_empty() {
        foot.insert(0, Line::styled("─".repeat(width), soft));
    }

    // The text takes what is left between head and foot.
    let rows = (area.height as usize).saturating_sub(head + foot.len()).max(1);
    compose.width = width;
    let (text, at_row, at_col) = compose.body.screen(width);
    if at_row < compose.body.top {
        compose.body.top = at_row;
    } else if at_row >= compose.body.top + rows {
        compose.body.top = at_row + 1 - rows;
    }
    let top = compose.body.top;
    if text.len() == 1 && text[0].is_empty() && compose.field != Field::Body {
        lines.push(Line::styled("Write your message", soft));
    }
    for row in text.into_iter().skip(top).take(rows) {
        if lines.len() < head + rows {
            lines.push(Line::raw(row));
        }
    }
    while lines.len() < head + rows {
        lines.push(Line::raw(""));
    }
    if compose.field == Field::Body {
        cursor = Some((area.x + at_col as u16, area.y + (head + at_row - top) as u16));
    }
    lines.extend(foot);
    frame.render_widget(Paragraph::new(lines), area);

    // Known recipients that match what is typed, right under the To line.
    if !compose.suggestions.is_empty() {
        let height = compose.suggestions.len() as u16 + 2;
        let popup = Rect {
            x: area.x + LABEL as u16,
            y: area.y + to_row as u16,
            width: 60.min(area.width.saturating_sub(LABEL as u16)),
            height: height.min(area.height.saturating_sub(to_row as u16)),
        };
        let items: Vec<Line> = compose
            .suggestions
            .iter()
            .enumerate()
            .map(|(index, contact)| {
                let line = if contact.group {
                    let count = contact.address.split(',').count();
                    Line::from(vec![
                        Span::raw(format!(" {} ", contact.name)),
                        Span::styled(
                            format!(
                                "group, {count} {}: {} ",
                                if count == 1 { "address" } else { "addresses" },
                                contact.address
                            ),
                            soft,
                        ),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw(format!(
                            " {} ",
                            crate::api::display_name(Some(&contact.name), Some(&contact.address))
                        )),
                        Span::styled(format!("{} ", contact.address), soft),
                    ])
                };
                if compose.picked == Some(index) {
                    line.style(Style::default().add_modifier(Modifier::REVERSED))
                } else {
                    line
                }
            })
            .collect();
        let block = Block::default()
            .borders(Borders::ALL)
            .title_bottom(" ↓ ↑ pick, Enter takes ");
        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(items).block(block), popup);
    }

    // The bottom line: the path of a file being attached, what just happened, or the keys.
    if let Some(path) = &compose.attaching {
        input(frame, bottom, " File to attach (path): ", path);
        return;
    }
    let hint = if status.is_empty() {
        let mut text = String::new();
        for hint in [
            "Ctrl+S send",
            "Tab next field",
            "Esc save and leave",
            "Ctrl+A attach",
            "Ctrl+O original",
            "Ctrl+E editor",
            "Ctrl+X discard",
        ] {
            if hint == "Ctrl+O original" && compose.original.is_none() {
                continue;
            }
            if text.width() + hint.width() + 3 > bottom.width as usize {
                break;
            }
            text.push_str("  ");
            text.push_str(hint);
        }
        Line::styled(text.split_off(1.min(text.len())), soft)
    } else {
        Line::raw(format!(" {}", fit(&status, bottom.width.saturating_sub(2) as usize)))
    };
    frame.render_widget(Paragraph::new(hint), bottom);
    if let Some(position) = cursor {
        frame.set_cursor_position(position);
    }
}

/// Every mail of the list, one below the other.
fn one_page(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let width = (inner.width as usize).min(100);
    let height = inner.height as usize;
    let Some(reading) = &mut app.reading else { return };
    let lines = crate::text::page(&reading.messages, width);
    app.page = height;
    app.lines = lines.len();
    reading.scroll = reading.scroll.min(lines.len().saturating_sub(height));
    let shown: Vec<Line> = lines.into_iter().skip(reading.scroll).take(height).collect();
    frame.render_widget(Paragraph::new(shown), inner);
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
    app.drawn.mail.set(area);
    frame.render_widget(block, area);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };
    let Some(open) = &mut app.open else { return };

    // Text is easier to read in a column than across a very wide terminal.
    let width = (inner.width as usize).min(100);
    // The sender's picture sits at the top right; the text keeps clear of it.
    const PICTURE: (u16, u16) = (8, 4);
    let pictured = open.picture.is_some() && app.picker.is_some() && inner.height >= 10 && width >= 50;
    let width = if pictured {
        width - PICTURE.0 as usize - 2
    } else {
        width
    };
    let lines = conversation(&open.thread, width, open.all, &open.unseen_at_open);
    let height = inner.height as usize;
    app.page = height;
    app.lines = lines.len();
    open.scroll = open.scroll.min(lines.len().saturating_sub(height));
    let shown: Vec<Line> = lines.into_iter().skip(open.scroll).take(height).collect();
    frame.render_widget(Paragraph::new(shown), inner);
    if pictured && open.scroll == 0 {
        let spot = Rect {
            x: inner.x + width as u16 + 2,
            y: inner.y,
            width: PICTURE.0,
            height: PICTURE.1,
        };
        if let (Some(picker), Some(picture)) = (&app.picker, &mut open.picture)
            && let Some(protocol) = picture.fitted(picker, spot)
        {
            frame.render_widget(ratatui_image::Image::new(protocol), spot);
        }
    }

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
            ("Mouse", "Click a list or a mail; the wheel scrolls"),
            (
                "Ctrl Z",
                "Undo the last action, for 4 seconds after it (also a sent mail)",
            ),
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
        "Writing mail",
        &[
            ("c", "Write a new mail"),
            ("r", "Reply"),
            ("Shift R", "Reply to all"),
            ("f", "Forward"),
            ("Enter", "In Drafts: take up the draft"),
            (
                "Ctrl S",
                "While writing: send (Ctrl Return where the terminal knows it)",
            ),
            ("Tab", "While writing: next field; Esc saves the draft and leaves"),
            (
                "Ctrl A O E",
                "Attach a file; read the original mail; use your own editor",
            ),
        ],
    ),
    (
        "Search and lists",
        &[
            ("/", "Search: words, from: to: subject: note: attachment: received:"),
            ("Shift F", "All mail from the sender of this mail"),
            ("Shift S", "Save the shown search in the side bar, or rename it"),
            ("Shift X", "Remove the saved search from the side bar"),
            ("Shift U", "Unseen mail only, in Nice to know, Junk and Archive"),
            ("Shift P", "Read all mails of the list on one page (not Junk)"),
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
            ("Shift A", "Attachments: Enter shows or opens one, s saves it"),
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

/// The attachments of the opened conversation.
fn attachments(frame: &mut Frame, area: Rect, files: &crate::app::Files, pictures: bool) {
    let height = (files.items.len() as u16 + 2).min(area.height.saturating_sub(2)).max(3);
    let popup = centered(area, 70.min(area.width.saturating_sub(2)), height);
    let rows = height.saturating_sub(2) as usize;
    let first = if files.cursor < rows {
        0
    } else {
        files.cursor + 1 - rows
    };
    let room = popup.width.saturating_sub(14) as usize;
    let lines: Vec<Line> = files
        .items
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .map(|(index, file)| {
            let size = format!("{} KB", (file.size + 1023) / 1024);
            let line = Line::from(vec![
                Span::raw(format!(" {} ", cell(&file.name, room))),
                Span::styled(format!("{size:>9} "), Style::default().fg(SOFT)),
            ]);
            if index == files.cursor {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect();
    let keys = if pictures {
        " Enter shows or opens, o opens, s saves, Esc closes "
    } else {
        " Enter opens, s saves to Downloads, Esc closes "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Attachments ")
        .title_bottom(keys);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

/// An image attachment, as large as the window allows.
fn picture(frame: &mut Frame, app: &mut App, area: Rect, bottom: Rect) {
    frame.render_widget(Clear, area);
    let Some(viewing) = &mut app.viewing else { return };
    let caption = Line::styled(format!(" {}   any key closes", viewing.name), Style::default().fg(SOFT));
    frame.render_widget(Clear, bottom);
    frame.render_widget(Paragraph::new(caption), bottom);
    let spot = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    if let Some(picker) = &app.picker
        && let Some(protocol) = viewing.picture.fitted(picker, spot)
    {
        frame.render_widget(ratatui_image::Image::new(protocol), spot);
    }
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
