//! Writing mail: a new one, a reply, a forward, or a draft taken up again.
//!
//! The text is plain: it is sent as simple paragraphs. A draft that was given formatting in
//! the web client loses it when it is edited here.

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{Addr, Message, display_name},
    app::App,
};

/// A small text editor: lines of text and a cursor, wrapped by the width it is drawn in.
#[derive(Default)]
pub struct Editor {
    pub lines: Vec<String>,
    pub row: usize,
    /// Cursor position in the line, in characters.
    pub col: usize,
    /// First screen row shown.
    pub top: usize,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            lines,
            row: 0,
            col: 0,
            top: 0,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
    }

    /// Byte position of the cursor in its line.
    fn at(&self) -> usize {
        self.lines[self.row]
            .char_indices()
            .nth(self.col)
            .map_or(self.lines[self.row].len(), |(i, _)| i)
    }

    pub fn insert(&mut self, c: char) {
        let at = self.at();
        self.lines[self.row].insert(at, c);
        self.col += 1;
    }

    pub fn newline(&mut self) {
        let at = self.at();
        let rest = self.lines[self.row].split_off(at);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            let at = self.at();
            self.lines[self.row].remove(at);
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.len(self.row);
            self.lines[self.row].push_str(&line);
        }
    }

    pub fn delete(&mut self) {
        if self.col < self.len(self.row) {
            let at = self.at();
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let line = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&line);
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.len(self.row);
        }
    }

    pub fn right(&mut self) {
        if self.col < self.len(self.row) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    /// Up or down by one screen row of `width` characters. False at the first or last row,
    /// where the caller moves on to the neighbouring field.
    pub fn vertical(&mut self, by: isize, width: usize) -> bool {
        let width = width.max(1);
        if by < 0 {
            if self.col >= width {
                self.col -= width;
            } else if self.row > 0 {
                self.row -= 1;
                // The same column of the last screen row of the line above.
                let len = self.len(self.row);
                self.col = (Self::last_row(len, width) * width + self.col).min(len);
            } else {
                return false;
            }
        } else {
            let len = self.len(self.row);
            if self.col / width < Self::last_row(len, width) {
                self.col = (self.col + width).min(len);
            } else if self.row + 1 < self.lines.len() {
                self.col %= width;
                self.row += 1;
                self.col = self.col.min(self.len(self.row));
            } else {
                return false;
            }
        }
        true
    }

    /// Index of the last screen row a line of `len` characters takes.
    fn last_row(len: usize, width: usize) -> usize {
        len.saturating_sub(1) / width
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.len(self.row);
    }

    /// The text as screen rows of `width` characters, and the cursor's row and column there.
    pub fn screen(&self, width: usize) -> (Vec<String>, usize, usize) {
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut cursor = (0, 0);
        for (index, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            if index == self.row {
                // At the very end of a full row the cursor waits at its right edge.
                let on = if self.col == chars.len() && self.col > 0 && self.col.is_multiple_of(width) {
                    (self.col - 1) / width
                } else {
                    self.col / width
                };
                cursor = (rows.len() + on, self.col - on * width);
            }
            if chars.is_empty() {
                rows.push(String::new());
            }
            for chunk in chars.chunks(width) {
                rows.push(chunk.iter().collect());
            }
        }
        (rows, cursor.0, cursor.1)
    }
}

/// The parts of the form, in the order Tab walks through them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    From,
    To,
    Cc,
    Bcc,
    Subject,
    Body,
    /// "Include the message … below yours".
    Quote,
    /// "Forward its attachments".
    Files,
    /// An attached file, by its position.
    Attachment(usize),
}

#[derive(Deserialize, Clone)]
pub struct Account {
    pub id: i64,
    pub label: String,
    pub address: String,
}

#[derive(Deserialize, Clone)]
pub struct Attached {
    pub id: i64,
    pub filename: String,
    pub size: i64,
}

#[derive(Deserialize)]
struct Source {
    id: i64,
    from: Addr,
    attachments: i64,
}

#[derive(Deserialize)]
struct Detail {
    draft: Value,
    attachments: Vec<Attached>,
    source: Option<Source>,
}

pub struct Compose {
    pub id: i64,
    pub kind: String,
    pub accounts: Vec<Account>,
    /// Position in `accounts` of the one the mail is sent from.
    pub account: usize,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: Editor,
    /// Who wrote the mail being answered or forwarded, if any.
    pub source_from: Option<String>,
    pub source_files: i64,
    pub quote: bool,
    pub forward_files: bool,
    pub files: Vec<Attached>,
    /// The mail being answered or forwarded, to read while writing.
    pub original: Option<Message>,
    pub show_original: bool,
    pub original_scroll: usize,
    pub field: Field,
    pub suggestions: Vec<Addr>,
    /// Suggestion picked with the arrow keys.
    pub picked: Option<usize>,
    /// Typing the path of a file to attach.
    pub attaching: Option<String>,
    /// Ctrl+X was pressed once: again discards the draft.
    pub discarding: bool,
    /// Width of the text area as last drawn; the cursor moves by it.
    pub width: usize,
}

impl Compose {
    fn fields(&self) -> Vec<Field> {
        let mut fields = vec![
            Field::From,
            Field::To,
            Field::Cc,
            Field::Bcc,
            Field::Subject,
            Field::Body,
        ];
        if self.source_from.is_some() {
            fields.push(Field::Quote);
            if self.kind == "forward" && self.source_files > 0 {
                fields.push(Field::Files);
            }
        }
        fields.extend((0..self.files.len()).map(Field::Attachment));
        fields
    }

    fn step(&mut self, by: isize) {
        let fields = self.fields();
        let at = fields.iter().position(|field| *field == self.field).unwrap_or(0) as isize;
        self.field = fields[(at + by).rem_euclid(fields.len() as isize) as usize];
        self.suggestions.clear();
        self.picked = None;
    }

    fn line(&mut self) -> Option<&mut String> {
        match self.field {
            Field::To => Some(&mut self.to),
            Field::Cc => Some(&mut self.cc),
            Field::Bcc => Some(&mut self.bcc),
            Field::Subject => Some(&mut self.subject),
            _ => None,
        }
    }

    /// The draft as the server stores it.
    fn stored(&self) -> Value {
        json!({
            "account_id": self.accounts.get(self.account).map(|account| account.id),
            "to_addrs": self.to,
            "cc_addrs": self.cc,
            "bcc_addrs": self.bcc,
            "subject": self.subject,
            "body_html": to_html(&self.body.text()),
            "forward_attachments": self.forward_files,
            "include_quote": self.quote,
        })
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Plain text as the simple HTML a mail is sent with: a paragraph per block of lines.
pub fn to_html(text: &str) -> String {
    let mut html = String::new();
    let mut block: Vec<String> = Vec::new();
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if !block.is_empty() {
                html.push_str(&format!("<p>{}</p>", block.join("<br>")));
                block.clear();
            }
        } else {
            block.push(escape(line));
        }
    }
    html
}

/// The text of a draft's HTML body, for editing.
fn to_text(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    html2text::from_read(html.as_bytes(), 10_000)
        .map(|text| text.trim_end().to_string())
        .unwrap_or_default()
}

impl App {
    /// Starts a draft and opens it: `kind` is new, reply, reply_all or forward; the last
    /// three are about the newest mail of the conversation the keys are on.
    pub(crate) async fn start_draft(&mut self, kind: &str) -> Result<()> {
        let source = if kind == "new" {
            None
        } else {
            let thread = match (&self.open, self.current_thread_id()) {
                (Some(open), _) => Some(open.thread.clone()),
                (None, Some(id)) => Some(self.client.get(&format!("/threads/{id}")).await?),
                (None, None) => None,
            };
            match thread.and_then(|thread| thread.messages.last().map(|message| message.id)) {
                Some(id) => Some(id),
                None => return Ok(()),
            }
        };
        let created: Value = self
            .client
            .post("/drafts", json!({ "kind": kind, "source_message": source }))
            .await?;
        let id = created["id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("the server did not create a draft"))?;
        self.open_draft(id).await
    }

    pub(crate) async fn open_draft(&mut self, id: i64) -> Result<()> {
        let detail: Detail = self.client.get(&format!("/drafts/{id}")).await?;
        let accounts: Vec<Account> = self.client.get("/accounts").await?;
        let text = |key: &str| detail.draft[key].as_str().unwrap_or_default().to_string();
        let account = accounts
            .iter()
            .position(|account| Some(account.id) == detail.draft["account_id"].as_i64())
            .unwrap_or(0);
        let kind = text("kind");
        let original = match &detail.source {
            Some(source) => self
                .client
                .get::<Message>(&format!("/messages/{}", source.id))
                .await
                .ok(),
            None => None,
        };
        // A reply has its recipients and subject already: writing starts in the text.
        let field = if kind == "reply" || kind == "reply_all" {
            Field::Body
        } else {
            Field::To
        };
        self.compose = Some(Compose {
            id,
            accounts,
            account,
            to: text("to_addrs"),
            cc: text("cc_addrs"),
            bcc: text("bcc_addrs"),
            subject: text("subject"),
            body: Editor::new(&to_text(&text("body_html"))),
            source_from: detail
                .source
                .as_ref()
                .map(|s| display_name(Some(&s.from.name), Some(&s.from.address))),
            source_files: detail.source.as_ref().map_or(0, |s| s.attachments),
            quote: detail.draft["include_quote"].as_bool().unwrap_or(true),
            forward_files: detail.draft["forward_attachments"].as_bool().unwrap_or(true),
            files: detail.attachments,
            original,
            show_original: false,
            original_scroll: 0,
            field,
            suggestions: Vec::new(),
            picked: None,
            attaching: None,
            discarding: false,
            width: 72,
            kind,
        });
        Ok(())
    }

    async fn save_draft(&mut self) -> Result<()> {
        if let Some(compose) = &self.compose {
            let _: Value = self
                .client
                .put(&format!("/drafts/{}", compose.id), compose.stored())
                .await?;
        }
        Ok(())
    }

    /// Recipients the server knows that match what is being typed after the last comma.
    async fn suggest(&mut self) -> Result<()> {
        let Some(compose) = &mut self.compose else {
            return Ok(());
        };
        compose.picked = None;
        let term = compose.to.rsplit(',').next().unwrap_or_default().trim().to_string();
        compose.suggestions = if compose.field == Field::To && term.chars().count() >= 2 {
            self.client
                .get(&format!("/contacts?q={}", crate::app::encode(&term)))
                .await?
        } else {
            Vec::new()
        };
        Ok(())
    }

    /// One key while writing. Returns to the lists when the mail is sent, left or discarded.
    pub(crate) async fn compose_key(&mut self, key: KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(compose) = &mut self.compose else {
            return Ok(());
        };
        let discarding = std::mem::take(&mut compose.discarding);

        // Typing the path of a file to attach.
        if let Some(mut path) = compose.attaching.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let id = compose.id;
                    let files: Vec<Attached> = self
                        .client
                        .upload(&format!("/drafts/{id}/attachments"), path.trim())
                        .await?;
                    if let Some(compose) = &mut self.compose {
                        compose.files = files;
                    }
                    self.status = "File attached".into();
                }
                KeyCode::Backspace => {
                    path.pop();
                    compose.attaching = Some(path);
                }
                KeyCode::Char(c) if !ctrl => {
                    path.push(c);
                    compose.attaching = Some(path);
                }
                _ => compose.attaching = Some(path),
            }
            return Ok(());
        }
        // Reading the original mail.
        if compose.show_original {
            let page = self.page.saturating_sub(2).max(1);
            match key.code {
                KeyCode::Char(' ') | KeyCode::PageDown | KeyCode::Down => {
                    let by = if key.code == KeyCode::Down { 3 } else { page };
                    compose.original_scroll = (compose.original_scroll + by).min(self.lines.saturating_sub(self.page));
                }
                KeyCode::Backspace | KeyCode::PageUp | KeyCode::Up => {
                    let by = if key.code == KeyCode::Up { 3 } else { page };
                    compose.original_scroll = compose.original_scroll.saturating_sub(by);
                }
                _ => compose.show_original = false,
            }
            return Ok(());
        }

        match key.code {
            // Send: Ctrl+S everywhere; Ctrl+Return where the terminal tells it from Return.
            KeyCode::Char('s') if ctrl => return self.send_draft().await,
            KeyCode::Enter if ctrl => return self.send_draft().await,
            KeyCode::Esc => {
                if !compose.suggestions.is_empty() {
                    compose.suggestions.clear();
                    compose.picked = None;
                } else {
                    self.save_draft().await?;
                    self.compose = None;
                    self.refresh().await?;
                    self.status = "Draft saved".into();
                }
            }
            KeyCode::Char('c') if ctrl => {
                // Leaving the program keeps what was written.
                self.save_draft().await?;
                self.quit = true;
            }
            KeyCode::Char('x') if ctrl => {
                if discarding {
                    let id = compose.id;
                    let _: Value = self.client.delete(&format!("/drafts/{id}")).await?;
                    self.compose = None;
                    self.refresh().await?;
                    self.status = "Draft discarded".into();
                } else {
                    compose.discarding = true;
                    self.status = "Discard this draft? Ctrl+X again discards it; any other key keeps it".into();
                }
            }
            KeyCode::Char('o') if ctrl && compose.original.is_some() => {
                compose.show_original = true;
                compose.original_scroll = 0;
            }
            KeyCode::Char('a') if ctrl => compose.attaching = Some(String::new()),
            KeyCode::Char('e') if ctrl => self.wants_editor = true,

            KeyCode::Tab if compose.picked.is_some() => self.take_suggestion(),
            KeyCode::Tab => compose.step(1),
            KeyCode::BackTab => compose.step(-1),

            _ => match compose.field {
                Field::Body => match key.code {
                    KeyCode::Enter => compose.body.newline(),
                    KeyCode::Backspace => compose.body.backspace(),
                    KeyCode::Delete => compose.body.delete(),
                    KeyCode::Left => compose.body.left(),
                    KeyCode::Right => compose.body.right(),
                    KeyCode::Home => compose.body.home(),
                    KeyCode::End => compose.body.end(),
                    KeyCode::Up => {
                        let width = compose.width;
                        if !compose.body.vertical(-1, width) {
                            compose.step(-1);
                        }
                    }
                    KeyCode::Down => {
                        let width = compose.width;
                        if !compose.body.vertical(1, width) {
                            compose.step(1);
                        }
                    }
                    KeyCode::Char(c) if !ctrl => compose.body.insert(c),
                    _ => {}
                },
                Field::From => match key.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if !compose.accounts.is_empty() => {
                        let by = if key.code == KeyCode::Left {
                            compose.accounts.len() - 1
                        } else {
                            1
                        };
                        compose.account = (compose.account + by) % compose.accounts.len();
                    }
                    KeyCode::Down | KeyCode::Enter => compose.step(1),
                    KeyCode::Up => compose.step(-1),
                    _ => {}
                },
                Field::Quote | Field::Files => match key.code {
                    KeyCode::Char(' ') | KeyCode::Enter => {
                        if compose.field == Field::Quote {
                            compose.quote = !compose.quote;
                        } else {
                            compose.forward_files = !compose.forward_files;
                        }
                    }
                    KeyCode::Down => compose.step(1),
                    KeyCode::Up => compose.step(-1),
                    _ => {}
                },
                Field::Attachment(index) => match key.code {
                    KeyCode::Backspace | KeyCode::Delete => {
                        let (draft, file) = (compose.id, compose.files[index].id);
                        let files: Vec<Attached> = self
                            .client
                            .delete(&format!("/drafts/{draft}/attachments/{file}"))
                            .await?;
                        if let Some(compose) = &mut self.compose {
                            compose.files = files;
                            compose.field = Field::Body;
                        }
                        self.status = "File removed".into();
                    }
                    KeyCode::Down => compose.step(1),
                    KeyCode::Up => compose.step(-1),
                    _ => {}
                },
                // The one-line fields.
                Field::To | Field::Cc | Field::Bcc | Field::Subject => {
                    let recipients = compose.field == Field::To;
                    match key.code {
                        KeyCode::Down if recipients && !compose.suggestions.is_empty() => {
                            let count = compose.suggestions.len();
                            compose.picked = Some(compose.picked.map_or(0, |at| (at + 1) % count));
                        }
                        KeyCode::Up if recipients && compose.picked.is_some() => {
                            let count = compose.suggestions.len();
                            compose.picked = compose.picked.map(|at| (at + count - 1) % count);
                        }
                        KeyCode::Enter if compose.picked.is_some() => self.take_suggestion(),
                        KeyCode::Down | KeyCode::Enter => compose.step(1),
                        KeyCode::Up => compose.step(-1),
                        KeyCode::Backspace => {
                            if let Some(line) = compose.line() {
                                line.pop();
                            }
                            self.suggest().await?;
                        }
                        KeyCode::Char('u') if ctrl => {
                            if let Some(line) = compose.line() {
                                line.clear();
                            }
                            self.suggest().await?;
                        }
                        KeyCode::Char(c) if !ctrl => {
                            if let Some(line) = compose.line() {
                                line.push(c);
                            }
                            self.suggest().await?;
                        }
                        _ => {}
                    }
                }
            },
        }
        Ok(())
    }

    /// Puts the picked suggestion in place of what was typed after the last comma.
    fn take_suggestion(&mut self) {
        let Some(compose) = &mut self.compose else { return };
        if let Some(contact) = compose.picked.and_then(|at| compose.suggestions.get(at)) {
            let mut parts: Vec<String> = compose.to.split(',').map(|part| part.trim().to_string()).collect();
            parts.pop();
            parts.push(contact.address.clone());
            compose.to = format!("{}, ", parts.join(", "));
        }
        compose.suggestions.clear();
        compose.picked = None;
    }

    async fn send_draft(&mut self) -> Result<()> {
        let Some(id) = self.compose.as_ref().map(|compose| compose.id) else {
            return Ok(());
        };
        self.save_draft().await?;
        let answer: Value = self.client.post(&format!("/drafts/{id}/send"), json!({})).await?;
        self.compose = None;
        self.refresh().await?;
        // It goes out when it can no longer be undone.
        self.status = "Sending the mail".into();
        self.undoable(&answer);
        Ok(())
    }

    /// The text of the draft for an outside editor, and its way back.
    pub fn draft_text(&self) -> Option<String> {
        self.compose.as_ref().map(|compose| compose.body.text())
    }

    pub fn set_draft_text(&mut self, text: &str) {
        if let Some(compose) = &mut self.compose {
            compose.body = Editor::new(text.trim_end());
            compose.field = Field::Body;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_becomes_paragraphs_and_stays_text() {
        assert_eq!(
            to_html("Hello\nthere\n\n\nSecond <b> & more"),
            "<p>Hello<br>there</p><p>Second &lt;b&gt; &amp; more</p>"
        );
        assert_eq!(to_html(""), "");
        assert_eq!(to_text("<p>Hello<br>there</p><p>Second</p>"), "Hello\nthere\n\nSecond");
    }

    #[test]
    fn the_editor_edits_across_lines_and_wrapped_rows() {
        let mut editor = Editor::new("");
        for c in "abcdefgh".chars() {
            editor.insert(c);
        }
        editor.newline();
        editor.insert('x');
        assert_eq!(editor.text(), "abcdefgh\nx");
        // Drawn four wide: "abcd", "efgh", "x"; the cursor is behind the x.
        assert_eq!(
            editor.screen(4),
            (vec!["abcd".to_string(), "efgh".into(), "x".into()], 2, 1)
        );
        assert!(editor.vertical(-1, 4));
        assert_eq!((editor.row, editor.col), (0, 5));
        assert!(editor.vertical(-1, 4));
        assert_eq!((editor.row, editor.col), (0, 1));
        assert!(!editor.vertical(-1, 4), "nothing above the first row");
        editor.backspace();
        assert_eq!(editor.text(), "bcdefgh\nx");
        editor.end();
        editor.delete();
        assert_eq!(editor.text(), "bcdefghx");
        editor.right();
        editor.left();
        editor.left();
        editor.insert('é');
        assert_eq!(editor.text(), "bcdefgéhx");
    }
}
