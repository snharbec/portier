//! What the terminal client shows and what its keys do. Drawing is in `ui.rs`.

use std::collections::HashSet;

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::api::{Client, Counts, Draft, ScreenerEntry, Thread, ThreadSummary};

/// The lists of the side bar, in the web client's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Home,
    Important,
    Delayed,
    Feed,
    Screener,
    Archive,
    Sent,
    Drafts,
    Junk,
    Trash,
}

impl Place {
    pub const ALL: [Place; 10] = [
        Place::Home,
        Place::Important,
        Place::Delayed,
        Place::Feed,
        Place::Screener,
        Place::Archive,
        Place::Sent,
        Place::Drafts,
        Place::Junk,
        Place::Trash,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Place::Home => "Home",
            Place::Important => "Important",
            Place::Delayed => "Delayed",
            Place::Feed => "Nice to know",
            Place::Screener => "Screener",
            Place::Archive => "Archive",
            Place::Sent => "Sent",
            Place::Drafts => "Drafts",
            Place::Junk => "Junk",
            Place::Trash => "Trash",
        }
    }

    /// The side bar's groups: a gap is drawn before the first place of each.
    pub fn starts_group(self) -> bool {
        matches!(self, Place::Screener | Place::Archive)
    }

    /// The `box` the server lists this place under; the Screener and Drafts are no mail lists.
    fn mailbox(self) -> Option<&'static str> {
        Some(match self {
            Place::Home => "important",
            Place::Important => "flagged",
            Place::Delayed => "delayed",
            Place::Feed => "feed",
            Place::Archive => "archive",
            Place::Sent => "sent",
            Place::Junk => "junk",
            Place::Trash => "trash",
            Place::Screener | Place::Drafts => return None,
        })
    }

    /// The number beside the place, and whether it asks for attention (the signal colour).
    pub fn badge(self, counts: &Counts) -> (i64, bool) {
        match self {
            Place::Home => (counts.unread_important, true),
            Place::Important => (counts.unread_flagged, true),
            Place::Delayed => (counts.unread_delayed, false),
            Place::Feed => (counts.unread_feed, false),
            Place::Screener => (counts.screener, true),
            Place::Archive => (counts.unread_archive, false),
            Place::Sent => (0, false),
            Place::Drafts => (counts.drafts, true),
            Place::Junk => (counts.unread_junk, false),
            Place::Trash => (counts.unread_trash, false),
        }
    }
}

/// One line of the list in the middle.
#[derive(Clone, Debug)]
pub enum Row {
    /// Heading of an area of Home, with its number of mails.
    Header {
        title: &'static str,
        count: usize,
    },
    Mail(ThreadSummary),
    /// A sender waiting in the Screener, or a draft: shown, not yet opened from here.
    Entry {
        who: String,
        text: String,
        date: i64,
    },
    /// What an empty area or list says.
    Empty(String),
}

impl Row {
    fn selectable(&self) -> bool {
        matches!(self, Row::Mail(_) | Row::Entry { .. })
    }
}

/// Where the opened mail is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Split {
    /// In place of the list.
    Off,
    Beside,
    Below,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Side,
    List,
}

pub struct Open {
    pub thread: Thread,
    /// First line shown.
    pub scroll: usize,
    /// Show every mail of the conversation in full.
    pub all: bool,
    pub unseen_at_open: HashSet<i64>,
}

pub struct App {
    client: Client,
    pub user: String,
    pub counts: Counts,
    pub place: Place,
    pub rows: Vec<Row>,
    /// Index into `rows` of the row the keys are on.
    pub cursor: usize,
    pub open: Option<Open>,
    pub split: Split,
    pub focus: Focus,
    pub help: bool,
    /// One line for the bottom of the screen: what just happened, or what went wrong.
    pub status: String,
    pub quit: bool,
    /// Mails read beside the list keep their place in Home's Unseen area while the mail
    /// area is open (as in the web client), so the arrow keys do not skip the ones after.
    held: Vec<i64>,
    /// Height of the mail area and number of its lines, as last drawn; paging needs both.
    pub page: usize,
    pub lines: usize,
}

impl App {
    pub async fn new(client: Client, user: String) -> Result<Self> {
        let mut app = Self {
            client,
            user,
            counts: Counts::default(),
            place: Place::Home,
            rows: Vec::new(),
            cursor: 0,
            open: None,
            split: Split::Off,
            focus: Focus::List,
            help: false,
            status: String::new(),
            quit: false,
            held: Vec::new(),
            page: 20,
            lines: 0,
        };
        app.refresh().await?;
        Ok(app)
    }

    /// Address of the opened mail in the web client.
    pub fn web_address(&self) -> Option<String> {
        self.open
            .as_ref()
            .map(|open| format!("{}/thread/{}", self.client.base, open.thread.id))
    }

    /// Loads the counts and the current list again, and the opened mail if there is one.
    pub async fn refresh(&mut self) -> Result<()> {
        self.counts = self.client.get("/counts").await?;
        self.load_rows().await?;
        if let Some(open) = &self.open {
            // A mail that is gone (moved elsewhere by another client) simply stays as it was read.
            if let Ok(thread) = self.client.get::<Thread>(&format!("/threads/{}", open.thread.id)).await
                && let Some(open) = &mut self.open
            {
                open.thread = thread;
            }
        }
        Ok(())
    }

    async fn threads(&self, mailbox: &str) -> Result<Vec<ThreadSummary>> {
        self.client.get(&format!("/threads?box={mailbox}")).await
    }

    async fn load_rows(&mut self) -> Result<()> {
        let at = self.current_id();
        let mut rows = Vec::new();
        match self.place {
            Place::Home => {
                let inbox = self.threads("important").await?;
                let flagged = self.threads("flagged").await?;
                let held = &self.held;
                let (unseen, seen): (Vec<_>, Vec<_>) =
                    inbox.into_iter().partition(|t| t.unread > 0 || held.contains(&t.id));
                let truly_unseen = unseen.iter().filter(|t| t.unread > 0).count();
                let areas = [
                    ("Unseen", truly_unseen, unseen, "No unseen messages. Area is empty."),
                    (
                        "Important",
                        flagged.len(),
                        flagged,
                        "No flagged messages. Area is empty.",
                    ),
                    ("Seen", seen.len(), seen, "No seen messages. Area is empty."),
                ];
                for (title, count, list, empty) in areas {
                    rows.push(Row::Header { title, count });
                    if list.is_empty() {
                        rows.push(Row::Empty(empty.to_string()));
                    }
                    rows.extend(list.into_iter().map(Row::Mail));
                }
            }
            Place::Screener => {
                let waiting: Vec<ScreenerEntry> = self.client.get("/screener").await?;
                rows.extend(waiting.into_iter().map(|entry| Row::Entry {
                    who: crate::api::display_name(Some(&entry.display_name), Some(&entry.address)),
                    text: format!(
                        "{}  ({} {})",
                        entry.subject,
                        entry.count,
                        if entry.count == 1 { "mail" } else { "mails" }
                    ),
                    date: entry.date,
                }));
                if rows.is_empty() {
                    rows.push(Row::Empty("Nobody is waiting. New senders appear here.".into()));
                }
            }
            Place::Drafts => {
                let drafts: Vec<Draft> = self.client.get("/drafts").await?;
                rows.extend(drafts.into_iter().map(|draft| Row::Entry {
                    who: if draft.to_addrs.is_empty() {
                        "no recipient yet".into()
                    } else {
                        format!("to {}", draft.to_addrs)
                    },
                    text: if draft.subject.is_empty() {
                        "(no subject)".into()
                    } else {
                        draft.subject
                    },
                    date: draft.updated_at,
                }));
                if rows.is_empty() {
                    rows.push(Row::Empty("No drafts.".into()));
                }
            }
            place => {
                let mailbox = place.mailbox().expect("mail list");
                rows.extend(self.threads(mailbox).await?.into_iter().map(Row::Mail));
                if rows.is_empty() {
                    rows.push(Row::Empty(format!("{} is empty.", place.name())));
                }
            }
        }
        self.rows = rows;
        // Stay on the mail the cursor was on; otherwise on the nearest row that can be chosen.
        self.cursor = at
            .and_then(|id| self.index_of(id))
            .or_else(|| self.nearest(self.cursor))
            .unwrap_or(0);
        Ok(())
    }

    fn current_id(&self) -> Option<i64> {
        match self.rows.get(self.cursor) {
            Some(Row::Mail(thread)) => Some(thread.id),
            _ => None,
        }
    }

    fn index_of(&self, id: i64) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| matches!(row, Row::Mail(t) if t.id == id))
    }

    /// The selectable row at `from`, or the next one after it, or the last one before it.
    fn nearest(&self, from: usize) -> Option<usize> {
        let from = from.min(self.rows.len().saturating_sub(1));
        (from..self.rows.len())
            .chain((0..from).rev())
            .find(|&index| self.rows[index].selectable())
    }

    /// Moves the cursor to the next (1) or previous (-1) row that can be chosen.
    fn step(&mut self, by: isize) -> bool {
        let mut index = self.cursor as isize + by;
        while index >= 0 && (index as usize) < self.rows.len() {
            if self.rows[index as usize].selectable() {
                self.cursor = index as usize;
                return true;
            }
            index += by;
        }
        false
    }

    async fn go(&mut self, place: Place) -> Result<()> {
        self.place = place;
        self.open = None;
        self.held.clear();
        self.cursor = 0;
        self.focus = Focus::List;
        self.load_rows().await
    }

    /// Opens the mail under the cursor. Reading it marks it as seen, as in the web client.
    async fn open_current(&mut self) -> Result<()> {
        let Some(Row::Mail(summary)) = self.rows.get(self.cursor) else {
            return Ok(());
        };
        let (id, was_unseen) = (summary.id, summary.unread > 0);
        let thread: Thread = self.client.get(&format!("/threads/{id}")).await?;
        let unseen_at_open: HashSet<i64> = thread.messages.iter().filter(|m| !m.seen).map(|m| m.id).collect();
        if !unseen_at_open.is_empty() {
            let _: Value = self.client.post(&format!("/threads/{id}/seen"), json!({})).await?;
        }
        if was_unseen && self.split != Split::Off && !self.held.contains(&id) {
            self.held.push(id);
        }
        let read_now = !unseen_at_open.is_empty();
        self.open = Some(Open {
            thread,
            scroll: 0,
            all: false,
            unseen_at_open,
        });
        if read_now {
            // The list and the counts show it as seen.
            self.refresh().await?;
        }
        Ok(())
    }

    fn close(&mut self) {
        self.open = None;
        self.held.clear();
    }

    fn scroll(&mut self, by: isize) {
        let (page, lines) = (self.page, self.lines);
        if let Some(open) = &mut self.open {
            let last = lines.saturating_sub(page);
            open.scroll = (open.scroll as isize + by).clamp(0, last as isize) as usize;
        }
    }

    /// One key. Errors of the server end up in the status line, not here; only a lost
    /// session is passed on.
    pub async fn key(&mut self, key: KeyEvent) -> Result<()> {
        self.status.clear();
        if let Err(error) = self.act(key).await {
            if error.is::<crate::api::SignedOut>() {
                return Err(error);
            }
            self.status = format!("{error:#}");
        }
        Ok(())
    }

    async fn act(&mut self, key: KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.help {
            // Any key closes the list of keys.
            self.help = false;
            return Ok(());
        }
        let page = self.page.saturating_sub(2).max(1) as isize;
        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('l') if ctrl => self.refresh().await?,
            KeyCode::F(5) => self.refresh().await?,

            // The same letters as in the web client.
            KeyCode::Char('H') | KeyCode::Char('1') => self.go(Place::Home).await?,
            KeyCode::Char('I') => self.go(Place::Important).await?,
            KeyCode::Char('D') => self.go(Place::Delayed).await?,
            KeyCode::Char('N') | KeyCode::Char('3') => self.go(Place::Feed).await?,
            KeyCode::Char('2') => self.go(Place::Screener).await?,

            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Side {
                    Focus::List
                } else {
                    Focus::Side
                };
            }
            KeyCode::Char('s') => {
                self.split = match self.split {
                    Split::Off => Split::Beside,
                    Split::Beside => Split::Below,
                    Split::Below => Split::Off,
                };
                self.status = match self.split {
                    Split::Off => "Split view off: a mail opens in place of the list".into(),
                    Split::Beside => "Split view: mail beside the list".into(),
                    Split::Below => "Split view: mail below the list".into(),
                };
            }
            KeyCode::Esc => {
                if self.open.is_some() {
                    self.close();
                    self.load_rows().await?;
                } else {
                    self.focus = Focus::List;
                }
            }

            // Paging inside the mail.
            KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::SHIFT) => self.scroll(-page),
            KeyCode::Char(' ') | KeyCode::PageDown => self.scroll(page),
            KeyCode::Backspace | KeyCode::PageUp => self.scroll(-page),
            KeyCode::Down if ctrl => self.scroll(page),
            KeyCode::Up if ctrl => self.scroll(-page),
            KeyCode::Char('x') => {
                if let Some(open) = &mut self.open {
                    open.all = !open.all;
                    open.scroll = 0;
                }
            }
            KeyCode::Char('o') => {
                if let Some(address) = self.web_address() {
                    open_in_browser(&address);
                    self.status = format!("Opened in the browser: {address}");
                }
            }

            KeyCode::Down | KeyCode::Char('j') => self.arrow(1).await?,
            KeyCode::Up | KeyCode::Char('k') => self.arrow(-1).await?,
            KeyCode::Enter | KeyCode::Right => {
                if self.focus == Focus::Side {
                    self.go(self.place).await?;
                } else {
                    self.open_current().await?;
                }
            }
            KeyCode::Left => self.focus = Focus::Side,
            _ => {}
        }
        Ok(())
    }

    /// Arrow down or up: the next place in the side bar, or the next mail. With a mail open
    /// the arrows go on to the next mail and open it, as they do in the web client.
    async fn arrow(&mut self, by: isize) -> Result<()> {
        if self.focus == Focus::Side {
            let at = Place::ALL.iter().position(|p| *p == self.place).unwrap_or(0) as isize;
            let next = (at + by).clamp(0, Place::ALL.len() as isize - 1) as usize;
            if next as isize != at {
                self.place = Place::ALL[next];
                self.open = None;
                self.held.clear();
                self.cursor = 0;
                self.load_rows().await?;
            }
            return Ok(());
        }
        // With the split view on, the first arrow opens the mail the cursor is on.
        let opens = self.open.is_some() || self.split != Split::Off;
        let first = self.split != Split::Off && self.open.is_none();
        if (first || self.step(by)) && opens {
            self.open_current().await?;
        }
        Ok(())
    }

    /// Keys given as text, for scripts and tests: plain characters, and `<down>`, `<up>`,
    /// `<left>`, `<right>`, `<enter>`, `<esc>`, `<tab>`, `<space>`, `<bs>`, `<c-down>`, `<c-up>`.
    pub async fn script(&mut self, keys: &str) -> Result<()> {
        let mut rest = keys;
        while let Some(c) = rest.chars().next() {
            let (code, modifiers, used) = match rest.strip_prefix('<').and_then(|r| r.split_once('>')) {
                Some((name, _)) => {
                    let (code, modifiers) = match name {
                        "down" => (KeyCode::Down, KeyModifiers::NONE),
                        "up" => (KeyCode::Up, KeyModifiers::NONE),
                        "left" => (KeyCode::Left, KeyModifiers::NONE),
                        "right" => (KeyCode::Right, KeyModifiers::NONE),
                        "enter" => (KeyCode::Enter, KeyModifiers::NONE),
                        "esc" => (KeyCode::Esc, KeyModifiers::NONE),
                        "tab" => (KeyCode::Tab, KeyModifiers::NONE),
                        "space" => (KeyCode::Char(' '), KeyModifiers::NONE),
                        "bs" => (KeyCode::Backspace, KeyModifiers::NONE),
                        "c-down" => (KeyCode::Down, KeyModifiers::CONTROL),
                        "c-up" => (KeyCode::Up, KeyModifiers::CONTROL),
                        other => anyhow::bail!("unknown key <{other}>"),
                    };
                    (code, modifiers, name.len() + 2)
                }
                None => (KeyCode::Char(c), KeyModifiers::NONE, c.len_utf8()),
            };
            self.key(KeyEvent::new(code, modifiers)).await?;
            rest = &rest[used..];
        }
        Ok(())
    }
}

/// Hands an address to the system's browser. Failing to is no reason to stop.
fn open_in_browser(address: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program)
        .arg(address)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
