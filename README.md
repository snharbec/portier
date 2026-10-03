# emscreen

Self-hosted web mail client that screens mail by sender. Each new sender waits in the
Screener until you put them in one of three places:

- **Important**: conversations you want to see, split into new and previously seen
- **Nice to know**: newsletters and updates, shown open in one scrolling page
- **Junk**: hidden, and moved to the Junk folder of the mail account

The **Attachments** page shows every file received in the last four weeks from senders in
Important and Nice to know, as a picture of its content: images as thumbnails, PDFs by their
first page. Selecting one opens the document on the page, with a link to its email.

In Important, Nice to know, Junk, Sent and the search results you can tick several mails and
mark them read or unread, move them to the account's Trash folder, or move them to any folder
on the mail server. Email Screen mirrors only Inbox, Sent and Junk, so mail moved elsewhere
leaves its views and stays on the server.

Rows in the mail lists and search results can also be slid left or right, by finger or mouse.
Each direction's actions (read/unread, move to folder, move to Trash) are chosen per user under
Settings: one action is performed on release, several are offered as buttons.

Connects to existing mailboxes over IMAP and SMTP (password or app password). Several users
can share one installation; each has their own accounts and sender decisions.

## Build and run

    cd web && npm install && npm run build && cd ..
    cargo build --release
    ./target/release/emscreen

The frontend is embedded in the binary, so build `web/` before `cargo build`. Open
http://127.0.0.1:8080. The first user to register manages the installation and can add
further users under Settings.

## Configuration (environment)

| Variable | Default | Meaning |
|---|---|---|
| `EMSCREEN_BIND` | `127.0.0.1:8080` | Listen address |
| `EMSCREEN_DATA_DIR` | `data` | SQLite database, raw mail, drafts, key file |
| `EMSCREEN_MASTER_KEY` | generated in `<data dir>/master.key` | Base64 of 32 bytes; encrypts mail account passwords |
| `EMSCREEN_OPEN_REGISTRATION` | off | `1` lets anyone create a user |
| `EMSCREEN_SYNC_MAX_PER_FOLDER` | `5000` | Newest messages mirrored per folder |
| `EMSCREEN_SOFFICE` | found automatically | Path to LibreOffice's `soffice`, or `off` |

Serve it behind HTTPS (reverse proxy) when it is reachable from other machines. Keep
`master.key` with your backups: without it the stored account passwords cannot be read.

## Previews of Office documents

Word, Excel, PowerPoint and OpenDocument attachments are shown by converting them to PDF with
LibreOffice. It is optional: without it those files appear as plain tiles and can be downloaded.

    brew install --cask libreoffice      # macOS
    apt install libreoffice-core libreoffice-writer libreoffice-calc libreoffice-impress

emscreen looks for `soffice` on the PATH and in `/Applications/LibreOffice.app` at start-up.
Conversion runs headless, one document at a time, on a copy of the file in a throwaway directory.
It still means LibreOffice opens files that strangers sent you. Set `EMSCREEN_SOFFICE=off` if
you do not want that.

## Development

    cargo run                       # API on :8080
    cd web && npm run dev           # UI with hot reload, proxies /api to :8080
    cargo test && cargo clippy --all-targets -- -D warnings
    cd web && npm run check

## Keyboard

`c` write, `/` search, `m` menu, `1` Important, `2` Screener, `3` Nice to know, `4` Attachments.
In the attachment viewer the left and right arrows step through files and Escape closes it.
In the Screener `i` / `k` / `j` decide the first sender. In a conversation `r` reply,
`a` reply all, `f` forward.
