//! What the terminal client shows and what its keys do. Drawing is in `ui.rs`.

use std::collections::HashSet;

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::api::{
    Client, Counts, Draft, Message, SavedSearch, ScreenerEntry, SearchHit, Thread, ThreadSummary, display_name,
};

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
    /// The results of a search: reached with `/` or from a saved search, not a fixed entry
    /// of the side bar.
    Search,
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
            Place::Search => "Search",
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
            Place::Screener | Place::Drafts | Place::Search => return None,
        })
    }

    /// A list of mails, on which the mail keys work (the Screener and Drafts are not).
    pub fn has_mail(self) -> bool {
        self.mailbox().is_some() || self == Place::Search
    }

    /// The lists that can be narrowed to unseen mail, as in the web client.
    pub fn can_narrow(self) -> bool {
        matches!(self, Place::Feed | Place::Junk | Place::Archive)
    }

    /// The lists whose mails can be read on one page: all but Junk, as in the web client.
    fn one_page(self) -> bool {
        self.has_mail() && self != Place::Junk
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
            Place::Search => (0, false),
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
    /// A sender waiting in the Screener (with the sender's id and newest conversation), or a
    /// draft (with neither: drafts are written in the web client for now).
    Entry {
        who: String,
        text: String,
        date: i64,
        sender: Option<i64>,
        thread: Option<i64>,
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

/// What the keys are for at the moment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    /// `z` was pressed: a digit says for how many days.
    Delay,
    /// Writing the note of a conversation: its id and the text so far.
    Note {
        thread: i64,
        text: String,
    },
    /// Choosing the folder to move mail to.
    Folders {
        account: i64,
        names: Vec<String>,
        cursor: usize,
    },
    /// Typing what to search for.
    Search {
        text: String,
    },
    /// Typing the name a search is saved under.
    Name {
        text: String,
    },
}

/// The mails of a list or of a search, opened one below the other.
pub struct Reading {
    pub messages: Vec<Message>,
    pub scroll: usize,
    /// Everything there is has been loaded.
    done: bool,
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
    pub mode: Mode,
    pub help: bool,
    /// Ticking mails for an action on several at once.
    pub selecting: bool,
    pub selected: HashSet<i64>,
    /// One line for the bottom of the screen: what just happened, or what went wrong.
    pub status: String,
    pub quit: bool,
    /// What the search results answer.
    pub query: String,
    /// The mails the search found, for reading them on one page.
    hits: Vec<i64>,
    pub saved: Vec<SavedSearch>,
    /// Lists narrowed to unseen mail.
    narrowed: HashSet<&'static str>,
    pub reading: Option<Reading>,
    /// Conversations of the current list that are in Important.
    flagged: HashSet<i64>,
    /// Mails read beside the list keep their place in Home's Unseen area while the mail
    /// area is open (as in the web client), so the arrow keys do not skip the ones after.
    held: Vec<i64>,
    /// Height of the mail area and number of its lines, as last drawn; paging needs both.
    pub page: usize,
    pub lines: usize,
}

/// "1 conversation", "3 conversations".
fn conversations(count: usize) -> String {
    format!("{count} conversation{}", if count == 1 { "" } else { "s" })
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
            mode: Mode::Normal,
            help: false,
            selecting: false,
            selected: HashSet::new(),
            status: String::new(),
            quit: false,
            query: String::new(),
            hits: Vec::new(),
            saved: Vec::new(),
            narrowed: HashSet::new(),
            reading: None,
            flagged: HashSet::new(),
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
        self.saved = self.client.get("/searches").await?;
        self.load_rows().await?;
        if let Some(open) = &self.open {
            match self.client.get::<Thread>(&format!("/threads/{}", open.thread.id)).await {
                Ok(thread) => {
                    if let Some(open) = &mut self.open {
                        open.thread = thread;
                    }
                }
                // Gone: moved away by another client, or emptied from the Trash.
                Err(_) => self.open = None,
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
        self.flagged.clear();
        match self.place {
            Place::Home => {
                let inbox = self.threads("important").await?;
                let flagged = self.threads("flagged").await?;
                self.flagged = flagged.iter().map(|t| t.id).collect();
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
                    who: display_name(Some(&entry.display_name), Some(&entry.address)),
                    text: format!(
                        "{}  ({} {})",
                        entry.subject,
                        entry.count,
                        if entry.count == 1 { "mail" } else { "mails" }
                    ),
                    date: entry.date,
                    sender: Some(entry.id),
                    thread: Some(entry.thread_id),
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
                    sender: None,
                    thread: None,
                }));
                if rows.is_empty() {
                    rows.push(Row::Empty("No drafts.".into()));
                }
            }
            Place::Search => {
                let hits: Vec<SearchHit> = self.client.get(&format!("/search?q={}", encode(&self.query))).await?;
                self.hits = hits.iter().map(|hit| hit.id).collect();
                self.flagged = hits
                    .iter()
                    .filter(|hit| hit.place == "flagged")
                    .map(|hit| hit.thread_id)
                    .collect();
                rows.extend(hits.into_iter().map(|hit| Row::Mail(hit.row())));
                if rows.is_empty() {
                    rows.push(Row::Empty("No mail matches. Try fewer or different words.".into()));
                }
            }
            place => {
                let mailbox = place.mailbox().expect("mail list");
                let mut list = self.threads(mailbox).await?;
                if place == Place::Important {
                    self.flagged = list.iter().map(|t| t.id).collect();
                }
                let all = list.len();
                if self.narrowed.contains(place.name()) {
                    // The opened mail stays listed although reading it made it seen.
                    let open = self.open.as_ref().map(|open| open.thread.id);
                    list.retain(|t| t.unread > 0 || Some(t.id) == open);
                }
                let narrowed_away = all > 0 && list.is_empty();
                rows.extend(list.into_iter().map(Row::Mail));
                if narrowed_away {
                    rows.push(Row::Empty("No unseen mail here. U shows all mail again.".into()));
                } else if rows.is_empty() {
                    rows.push(Row::Empty(format!("{} is empty.", place.name())));
                }
            }
        }
        self.rows = rows;
        // Ticks on mails that left the list mean nothing any more.
        let present: HashSet<i64> = self.mails().map(|t| t.id).collect();
        self.selected.retain(|id| present.contains(id));
        // Stay on the mail the cursor was on; otherwise on the nearest row that can be chosen.
        self.cursor = at
            .and_then(|id| self.index_of(id))
            .or_else(|| self.nearest(self.cursor))
            .unwrap_or(0);
        Ok(())
    }

    fn mails(&self) -> impl Iterator<Item = &ThreadSummary> {
        self.rows.iter().filter_map(|row| match row {
            Row::Mail(thread) => Some(thread),
            _ => None,
        })
    }

    fn current(&self) -> Option<&ThreadSummary> {
        match self.rows.get(self.cursor) {
            Some(Row::Mail(thread)) => Some(thread),
            _ => None,
        }
    }

    fn current_id(&self) -> Option<i64> {
        self.current().map(|thread| thread.id)
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

    /// Whether the current list shows unseen mail only.
    pub fn is_narrowed(&self) -> bool {
        self.narrowed.contains(self.place.name())
    }

    /// The saved search whose results are shown, if the query is one of the saved ones.
    pub fn saved_here(&self) -> Option<&SavedSearch> {
        (self.place == Place::Search)
            .then(|| self.saved.iter().find(|saved| saved.query == self.query.trim()))
            .flatten()
    }

    /// Runs a search and shows what it finds.
    async fn search(&mut self, query: String) -> Result<()> {
        let before = (self.place, std::mem::replace(&mut self.query, query));
        self.place = Place::Search;
        self.open = None;
        self.reading = None;
        self.held.clear();
        self.selecting = false;
        self.selected.clear();
        self.cursor = 0;
        self.focus = Focus::List;
        if let Err(error) = self.load_rows().await {
            // A query the server cannot read: stay where the reader was.
            (self.place, self.query) = before;
            self.load_rows().await?;
            return Err(error);
        }
        let found = self.mails().count();
        self.status = match found {
            0 => String::new(),
            200 => "The newest 200 mails are shown".into(),
            1 => "1 mail found".into(),
            n => format!("{n} mails found"),
        };
        Ok(())
    }

    /// Opens every mail of the current list, or every mail found, one below the other.
    async fn read_all(&mut self) -> Result<()> {
        if !self.place.one_page() || self.mails().next().is_none() {
            return Ok(());
        }
        self.open = None;
        self.reading = Some(Reading {
            messages: Vec::new(),
            scroll: 0,
            done: false,
        });
        self.read_more().await
    }

    /// The next twenty mails of the page.
    async fn read_more(&mut self) -> Result<()> {
        const BATCH: usize = 20;
        let Some(have) = self
            .reading
            .as_ref()
            .filter(|reading| !reading.done)
            .map(|reading| reading.messages.len())
        else {
            return Ok(());
        };
        let (batch, done): (Vec<Message>, bool) = match self.place.mailbox() {
            Some(mailbox) => {
                let batch: Vec<Message> = self
                    .client
                    .get(&format!("/feed?box={mailbox}&offset={have}&limit={BATCH}"))
                    .await?;
                let done = batch.len() < BATCH;
                (batch, done)
            }
            None => {
                let mut batch = Vec::new();
                for id in self.hits.iter().skip(have).take(BATCH) {
                    // A mail that is gone by now is left out.
                    if let Ok(message) = self.client.get::<Message>(&format!("/messages/{id}")).await {
                        batch.push(message);
                    }
                }
                (batch, have + BATCH >= self.hits.len())
            }
        };
        if let Some(reading) = &mut self.reading {
            reading.messages.extend(batch);
            reading.done = done;
        }
        Ok(())
    }

    async fn go(&mut self, place: Place) -> Result<()> {
        self.place = place;
        self.reading = None;
        self.open = None;
        self.held.clear();
        self.selecting = false;
        self.selected.clear();
        self.cursor = 0;
        self.focus = Focus::List;
        self.load_rows().await
    }

    /// Opens the mail under the cursor. Reading it marks it as seen, as in the web client.
    async fn open_current(&mut self) -> Result<()> {
        let (id, was_unseen) = match self.rows.get(self.cursor) {
            Some(Row::Mail(summary)) => (summary.id, summary.unread > 0),
            // A waiting sender's newest mail, to read before deciding.
            Some(Row::Entry { thread: Some(id), .. }) => (*id, false),
            _ => return Ok(()),
        };
        let thread: Thread = self.client.get(&format!("/threads/{id}")).await?;
        let unseen_at_open: HashSet<i64> = thread.messages.iter().filter(|m| !m.seen).map(|m| m.id).collect();
        // Mail of a sender not let in yet stays unseen until the decision is made.
        let read_now = !unseen_at_open.is_empty() && self.place != Place::Screener;
        if read_now {
            let _: Value = self.client.post(&format!("/threads/{id}/seen"), json!({})).await?;
        }
        if was_unseen && self.split != Split::Off && !self.held.contains(&id) {
            self.held.push(id);
        }
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

    // ---- Actions on mail ------------------------------------------------------------------

    /// The conversations an action is for: the ticked ones while selecting, else the opened
    /// mail, else the one under the cursor.
    fn targets(&self) -> Vec<i64> {
        if self.selecting && !self.selected.is_empty() {
            return self
                .mails()
                .map(|t| t.id)
                .filter(|id| self.selected.contains(id))
                .collect();
        }
        match (&self.open, self.current_id()) {
            (Some(open), _) => vec![open.thread.id],
            (None, Some(id)) => vec![id],
            (None, None) => Vec::new(),
        }
    }

    /// Runs one of the server's mail actions on the targets and says what was done.
    /// With `leaves`, the mails go away from this list: the cursor (and the mail area) then
    /// moves on to the mail that followed.
    async fn act_on_mail(&mut self, action: &str, done: &str, extra: Value, leaves: bool) -> Result<()> {
        let ids = self.targets();
        if ids.is_empty() {
            return Ok(());
        }
        let mut body = json!({ "action": action, "thread_ids": ids });
        if let (Some(body), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            body.extend(extra.clone());
        }
        // Where to go afterwards: the first mail after the affected ones, else the one before.
        let order: Vec<i64> = self.mails().map(|t| t.id).collect();
        let first = order.iter().position(|id| ids.contains(id)).unwrap_or(0);
        let onward = order[first..]
            .iter()
            .chain(order[..first].iter().rev())
            .find(|id| !ids.contains(id))
            .copied();
        let was_open = self.open.is_some();

        let _: Value = self.client.post("/mail/actions", body).await?;
        self.status = format!("{done} {}", conversations(ids.len()));
        self.selected.clear();
        self.selecting = false;
        if leaves {
            self.open = None;
        }
        self.refresh().await?;
        if leaves {
            if let Some(index) = onward.and_then(|id| self.index_of(id)) {
                self.cursor = index;
                if was_open {
                    let said = std::mem::take(&mut self.status);
                    self.open_current().await?;
                    self.status = said;
                }
            } else {
                self.held.clear();
            }
        }
        Ok(())
    }

    /// Whether the mail the keys are on is in Important.
    fn is_important(&self) -> bool {
        match (&self.open, self.current_id()) {
            (Some(open), _) => open.thread.important,
            (None, Some(id)) => self.flagged.contains(&id),
            (None, None) => false,
        }
    }

    fn is_delayed(&self) -> bool {
        match (&self.open, self.current()) {
            (Some(open), _) => open.thread.snoozed_until.is_some(),
            (None, Some(thread)) => thread.snoozed_until.is_some(),
            (None, None) => false,
        }
    }

    /// Moves the sender of the Screener row under the cursor to a list.
    async fn decide(&mut self, category: &str, place: &str) -> Result<()> {
        let Some(Row::Entry {
            sender: Some(id), who, ..
        }) = self.rows.get(self.cursor)
        else {
            return Ok(());
        };
        let (id, who) = (*id, who.clone());
        let _: Value = self
            .client
            .post(&format!("/senders/{id}/category"), json!({ "category": category }))
            .await?;
        self.open = None;
        self.refresh().await?;
        self.status = format!("{who} goes to {place}");
        Ok(())
    }

    /// One key. Errors of the server end up in the status line, not here; only a lost
    /// session is passed on.
    pub async fn key(&mut self, key: KeyEvent) -> Result<()> {
        self.status.clear();
        if let Err(error) = self.act(key).await {
            if error.is::<crate::api::SignedOut>() {
                return Err(error);
            }
            self.mode = Mode::Normal;
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
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => {}
            Mode::Delay => return self.delay_key(key).await,
            Mode::Note { thread, text } => return self.note_key(key, thread, text).await,
            Mode::Folders { account, names, cursor } => return self.folder_key(key, account, names, cursor).await,
            Mode::Search { mut text } => {
                return match edit(key, &mut text) {
                    Typed::Done if text.trim().is_empty() => Ok(()),
                    Typed::Done => self.search(text.trim().to_string()).await,
                    Typed::More => {
                        self.mode = Mode::Search { text };
                        Ok(())
                    }
                    Typed::Cancelled => Ok(()),
                };
            }
            Mode::Name { mut text } => {
                return match edit(key, &mut text) {
                    Typed::Done => self.save_search(text.trim()).await,
                    Typed::More => {
                        self.mode = Mode::Name { text };
                        Ok(())
                    }
                    Typed::Cancelled => Ok(()),
                };
            }
        }
        if self.reading.is_some() {
            return self.reading_key(key).await;
        }

        let page = self.page.saturating_sub(2).max(1) as isize;
        let screener = self.place == Place::Screener;
        match key.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('l') if ctrl => self.refresh().await?,
            KeyCode::F(5) => self.refresh().await?,

            // The Screener: where the sender under the cursor goes.
            KeyCode::Char('i') if screener => self.decide("important", "Home").await?,
            KeyCode::Char('n') if screener => self.decide("feed", "Nice to know").await?,
            KeyCode::Char('J') if screener => self.decide("junk", "Junk").await?,

            // The same letters as in the web client.
            KeyCode::Char('H') | KeyCode::Char('1') => self.go(Place::Home).await?,
            KeyCode::Char('I') => self.go(Place::Important).await?,
            KeyCode::Char('D') => self.go(Place::Delayed).await?,
            KeyCode::Char('N') | KeyCode::Char('3') => self.go(Place::Feed).await?,
            KeyCode::Char('2') => self.go(Place::Screener).await?,

            // Searching, as in the web client's search field.
            KeyCode::Char('/') => {
                let text = if self.place == Place::Search {
                    self.query.clone()
                } else {
                    String::new()
                };
                self.mode = Mode::Search { text };
            }
            KeyCode::Char('F') => {
                // All mail from the sender of the mail the keys are on.
                let address = match (&self.open, self.current()) {
                    (Some(open), _) => open.thread.sender.as_ref().map(|sender| sender.address.clone()),
                    (None, Some(thread)) => Some(thread.from_addr.clone()),
                    (None, None) => None,
                };
                if let Some(address) = address.filter(|address| !address.is_empty()) {
                    self.search(format!("from:{address}")).await?;
                }
            }
            KeyCode::Char('S') if self.place == Place::Search => {
                let text = self.saved_here().map(|saved| saved.name.clone()).unwrap_or_default();
                self.mode = Mode::Name { text };
            }
            KeyCode::Char('X') if self.place == Place::Search => {
                if let Some((id, name)) = self.saved_here().map(|saved| (saved.id, saved.name.clone())) {
                    let _: Value = self.client.delete(&format!("/searches/{id}")).await?;
                    self.saved = self.client.get("/searches").await?;
                    self.status = format!("Removed {name} from the side bar");
                }
            }
            KeyCode::Char('U') if self.place.can_narrow() => {
                let name = self.place.name();
                if !self.narrowed.remove(name) {
                    self.narrowed.insert(name);
                }
                self.status = if self.is_narrowed() {
                    "Unseen mail only".into()
                } else {
                    "All mail".into()
                };
                self.load_rows().await?;
            }
            KeyCode::Char('P') => self.read_all().await?,

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
                } else if self.selecting {
                    self.selecting = false;
                    self.selected.clear();
                } else {
                    self.focus = Focus::List;
                }
            }

            // Selecting several mails.
            KeyCode::Char('v') if self.focus == Focus::List && self.place.has_mail() => {
                self.selecting = !self.selecting;
                self.selected.clear();
                if self.selecting {
                    self.status = "Selecting: Space ticks, * ticks all, then a d u i z m; Esc stops".into();
                }
            }
            KeyCode::Char(' ') if self.selecting => {
                if let Some(id) = self.current_id() {
                    if !self.selected.remove(&id) {
                        self.selected.insert(id);
                    }
                    self.step(1);
                }
            }
            KeyCode::Char('*') if self.selecting => {
                let all: HashSet<i64> = self.mails().map(|t| t.id).collect();
                self.selected = if self.selected.len() == all.len() {
                    HashSet::new()
                } else {
                    all
                };
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

            // What the web client's buttons do, on the same letters.
            KeyCode::Char('a') if self.archivable() => {
                self.act_on_mail("archive", "Archived:", json!({}), true).await?
            }
            KeyCode::Char('d') if self.trashable() => {
                self.act_on_mail("trash", "Moved to Trash:", json!({}), true).await?
            }
            KeyCode::Char('b')
                if self.place == Place::Trash || self.open.as_ref().is_some_and(|o| o.thread.can_restore) =>
            {
                self.act_on_mail("untrash", "Moved back to Home:", json!({}), true)
                    .await?
            }
            KeyCode::Char('u') if !screener => {
                // Unseen again; a mail that is unseen already is marked as seen instead.
                let unseen = self.open.is_none() && self.current().is_some_and(|t| t.unread > 0);
                if unseen {
                    self.act_on_mail("read", "Marked as seen:", json!({}), false).await?;
                } else {
                    let was_open = self.open.is_some();
                    self.act_on_mail("unread", "Marked as unseen:", json!({}), false)
                        .await?;
                    if was_open {
                        // Back to the list, where it shows as new (as in the web client).
                        let said = std::mem::take(&mut self.status);
                        self.close();
                        self.load_rows().await?;
                        self.status = said;
                    }
                }
            }
            KeyCode::Char('i') if self.archivable() => {
                if self.is_important() {
                    self.act_on_mail(
                        "unimportant",
                        "Moved to Home:",
                        json!({}),
                        self.place == Place::Important,
                    )
                    .await?
                } else {
                    self.act_on_mail("important", "Moved to Important:", json!({}), self.place != Place::Home)
                        .await?
                }
            }
            KeyCode::Char('z') if self.archivable() => {
                if self.is_delayed() {
                    self.act_on_mail("undelay", "Back in Home:", json!({}), true).await?
                } else if !self.targets().is_empty() {
                    self.mode = Mode::Delay;
                    self.status = "Delay for how many days? 1, 2, 3 or 7 (Esc cancels)".into();
                }
            }
            KeyCode::Char('t') if !screener => {
                let note = match (&self.open, self.current()) {
                    (Some(open), _) => Some((open.thread.id, open.thread.note.clone())),
                    (None, Some(thread)) => Some((thread.id, thread.note.clone())),
                    (None, None) => None,
                };
                if let Some((thread, text)) = note {
                    // Edited on one line; a note written on several in the web client is joined.
                    self.mode = Mode::Note {
                        thread,
                        text: text.replace('\n', " "),
                    };
                }
            }
            KeyCode::Char('m') if !screener => self.choose_folder().await?,

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

    /// Trash is for mail that is not there yet, and not for mail of senders still waiting.
    fn trashable(&self) -> bool {
        match &self.open {
            Some(open) => open.thread.can_trash && self.place != Place::Screener,
            None => !matches!(self.place, Place::Trash | Place::Screener | Place::Drafts),
        }
    }

    /// Archive, Important and Delay are for mail that is still in a list of received mail.
    fn archivable(&self) -> bool {
        match &self.open {
            Some(open) => open.thread.can_archive && self.place != Place::Screener,
            None => matches!(
                self.place,
                Place::Home | Place::Important | Place::Delayed | Place::Feed | Place::Junk | Place::Search
            ),
        }
    }

    async fn delay_key(&mut self, key: KeyEvent) -> Result<()> {
        let days = match key.code {
            KeyCode::Char('1') => 1,
            KeyCode::Char('2') => 2,
            KeyCode::Char('3') => 3,
            KeyCode::Char('7') => 7,
            // Anything else calls it off.
            _ => return Ok(()),
        };
        let done = format!("Delayed for {days} {}:", if days == 1 { "day" } else { "days" });
        self.act_on_mail("delay", &done, json!({ "days": days }), true).await
    }

    async fn note_key(&mut self, key: KeyEvent, thread: i64, mut text: String) -> Result<()> {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
                let saved: Value = self
                    .client
                    .put(&format!("/threads/{thread}/note"), json!({ "note": text }))
                    .await?;
                self.refresh().await?;
                self.status = if saved["note"].as_str().unwrap_or_default().is_empty() {
                    "Note removed".into()
                } else {
                    "Note saved".into()
                };
            }
            KeyCode::Backspace => {
                text.pop();
                self.mode = Mode::Note { thread, text };
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.mode = Mode::Note {
                    thread,
                    text: String::new(),
                };
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                text.push(c);
                self.mode = Mode::Note { thread, text };
            }
            _ => self.mode = Mode::Note { thread, text },
        }
        Ok(())
    }

    /// Asks the mail server for the account's folders and offers them.
    async fn choose_folder(&mut self) -> Result<()> {
        let ids = self.targets();
        let accounts: HashSet<i64> = self
            .mails()
            .filter(|t| ids.contains(&t.id))
            .map(|t| t.account_id)
            .collect();
        let mut accounts = accounts.into_iter();
        let (Some(account), None) = (accounts.next(), accounts.next()) else {
            if !ids.is_empty() {
                self.status = "The mails belong to different mail accounts; move them one account at a time".into();
            }
            return Ok(());
        };
        let answer: Value = self.client.get(&format!("/accounts/{account}/folders")).await?;
        let names: Vec<String> = answer["folders"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|name| name.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if names.is_empty() {
            self.status = "The mail server lists no folders".into();
        } else {
            self.mode = Mode::Folders {
                account,
                names,
                cursor: 0,
            };
        }
        Ok(())
    }

    async fn folder_key(&mut self, key: KeyEvent, account: i64, names: Vec<String>, cursor: usize) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {}
            KeyCode::Down | KeyCode::Char('j') => {
                self.mode = Mode::Folders {
                    account,
                    cursor: (cursor + 1).min(names.len() - 1),
                    names,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.mode = Mode::Folders {
                    account,
                    cursor: cursor.saturating_sub(1),
                    names,
                };
            }
            KeyCode::Enter => {
                let folder = names[cursor].clone();
                let done = format!("Moved to {folder}:");
                self.act_on_mail("move", &done, json!({ "account_id": account, "folder": folder }), true)
                    .await?;
            }
            _ => self.mode = Mode::Folders { account, names, cursor },
        }
        Ok(())
    }

    /// Keys while the mails of a list are read on one page.
    async fn reading_key(&mut self, key: KeyEvent) -> Result<()> {
        let page = self.page.saturating_sub(2).max(1) as isize;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let by = match key.code {
            KeyCode::Esc | KeyCode::Char('P') => {
                self.reading = None;
                return Ok(());
            }
            KeyCode::Char('q') => {
                self.quit = true;
                return Ok(());
            }
            KeyCode::Char('c') if ctrl => {
                self.quit = true;
                return Ok(());
            }
            KeyCode::Char('?') => {
                self.help = true;
                return Ok(());
            }
            KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::SHIFT) => -page,
            KeyCode::Char(' ') | KeyCode::PageDown => page,
            KeyCode::Backspace | KeyCode::PageUp => -page,
            KeyCode::Down if ctrl => page,
            KeyCode::Up if ctrl => -page,
            KeyCode::Down | KeyCode::Char('j') => 3,
            KeyCode::Up | KeyCode::Char('k') => -3,
            _ => return Ok(()),
        };
        let (height, lines) = (self.page, self.lines);
        let Some(reading) = &mut self.reading else {
            return Ok(());
        };
        let last = lines.saturating_sub(height);
        reading.scroll = (reading.scroll as isize + by).clamp(0, last as isize) as usize;
        // Near the end of what is loaded, the next mails are fetched.
        if by > 0 && reading.scroll + height >= lines {
            self.read_more().await?;
        }
        Ok(())
    }

    /// Saves the shown search under a name, or renames it when it is saved already.
    async fn save_search(&mut self, name: &str) -> Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        let body = json!({ "name": name, "query": self.query });
        let renamed = match self.saved_here().map(|saved| saved.id) {
            Some(id) => {
                let _: Value = self.client.put(&format!("/searches/{id}"), body).await?;
                true
            }
            None => {
                let _: Value = self.client.post("/searches", body).await?;
                false
            }
        };
        self.saved = self.client.get("/searches").await?;
        self.status = if renamed {
            format!("Renamed to {name}")
        } else {
            format!("Saved as {name}")
        };
        Ok(())
    }

    /// Arrow down or up: the next place in the side bar, or the next mail. With a mail open
    /// the arrows go on to the next mail and open it, as they do in the web client.
    async fn arrow(&mut self, by: isize) -> Result<()> {
        if self.focus == Focus::Side {
            // The side bar: the lists, then the saved searches.
            let lists = Place::ALL.len();
            let at = match self.saved_here() {
                Some(here) => lists + self.saved.iter().position(|saved| saved.id == here.id).unwrap_or(0),
                None => Place::ALL.iter().position(|p| *p == self.place).unwrap_or(0),
            } as isize;
            let next = (at + by).clamp(0, (lists + self.saved.len()) as isize - 1) as usize;
            if next as isize != at {
                if next < lists {
                    self.go(Place::ALL[next]).await?;
                } else {
                    let query = self.saved[next - lists].query.clone();
                    self.search(query).await?;
                }
                self.focus = Focus::Side;
            }
            return Ok(());
        }
        // While ticking mails the arrows only move.
        if self.selecting {
            self.step(by);
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

/// What a key did to a line of text being typed.
enum Typed {
    More,
    Done,
    Cancelled,
}

/// One key on a line of text: Enter ends it, Esc gives up, Ctrl+U empties it.
fn edit(key: KeyEvent, text: &mut String) -> Typed {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Enter => return Typed::Done,
        KeyCode::Esc => return Typed::Cancelled,
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char('u') if ctrl => text.clear(),
        KeyCode::Char(c) if !ctrl => text.push(c),
        _ => {}
    }
    Typed::More
}

/// A query as part of a web address.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn queries_are_encoded_for_the_address() {
        assert_eq!(encode("hallo from:carsten"), "hallo%20from%3Acarsten");
        assert_eq!(encode("größe & 100%"), "gr%C3%B6%C3%9Fe%20%26%20100%25");
    }
}
