# Portier

Self-hosted web mail client that screens mail by sender. Each new sender waits in the
Screener until you put them in one of three places:

- **Home**: conversations you want to see, split into new and previously seen
- **Nice to know**: newsletters and updates, kept out of the way of Home
- **Junk**: hidden, and moved to the Junk folder of the mail account

**Automatic archive** (Settings, off by default) archives seen conversations of Home once
their newest mail is older than a number of weeks you choose. Unseen, Important and delayed
conversations and the other lists are left alone. It is checked hourly.

**Trash** lists what you deleted: the newest 500 mails of the account's Trash folder. Mail in the
Trash is left out of the other lists and of search. "Move back to Home", on an opened
mail or for mails selected in the Trash list, brings it back: received mail to the inbox, your own to Sent.

**Nice to know folder** (optional, per mail account in Settings): with a folder name set, mail
from Nice to know senders is moved there on the mail server, out of the inbox, so other mail
programs see a tidy inbox too. The folder is created if missing. Filing a sender elsewhere moves
their mail back. Without it such mail stays in the inbox.

**Delayed folder** (optional, per mail account in Settings): with a folder name set, a delayed
conversation waits in that folder on the mail server and moves back to the inbox, unseen, when it
returns. Without it delayed mail stays where it is on the server and is only hidden in Portier.

**Important** is a list below Home for conversations you want set apart. Drag a row by its
handle onto Important in the side bar, use the button in the selection bar, or press `i` while
reading; the same ways lead back to Home. It is stored as the mail server's "flagged" mark,
so other mail programs show it as flag or star, and mail flagged there appears in Important.

**Undo**: every action on mail can be taken back for 4 seconds: archive, Trash and back, seen and
unseen, Important, Delay, moving to a folder, notes, and sending. The notice that confirms the
action has an Undo button (`Ctrl+Z`; in the terminal client `Ctrl+Z` too). Portier shows the
change at once, but only carries it out on the mail server when it can no longer be undone, so
an undone action never happened there. For the same reason a mail you send leaves about five
seconds after you sent it; if sending then fails, the mail is back in Drafts with the reason.

**Notes**: in an opened mail, "Add note" (key `t`) attaches a text of your own to the conversation,
e.g. "tax 2026" or "warranty until May". It shows on the mail, in the lists and in search results,
is kept in Portier only (not on the mail server), and the search finds its words: typed
plainly, or with `note:`.

**Delay** takes a conversation out of Home for 1, 2, 3 or 7 days. It waits in the Delayed
list and returns at 7:00 (server time) on that day, unseen and at the top. A new mail in the
conversation ends the delay early. Delays are kept in Portier only.

**Split view** (two buttons in the top bar) shows the opened mail together with the mail list:
beside it, or below it; while no mail is open the list has the whole page. It works on the
mail lists and on search results. Each part scrolls on its own; drag the line between them to resize
(double-click it for the usual size). Windows narrower than about 900 px have no
room for the two side by side and stack them. The choice is kept per browser.

**Archive** files a mail away from any list or from the mail itself: it moves to the account's
Archive folder on the server and shows in the Archive list.

The mail lists and the search results have "Read all on one page", which shows their mails
opened one below the other. Junk is the exception: its mail is only opened one by one.

Images that a mail loads from the internet are hidden until you choose "Show images". That choice
is remembered for the sender; "Hide images and ask again" undoes it.

The **Attachments** page shows every file received in the last four weeks from senders in
Home and Nice to know, as a picture of its content: images as thumbnails, PDFs by their
first page. Selecting one opens the document on the page, with a link to its email.

In Home, Nice to know, Junk, Sent and the search results, "Select" above the list shows a
checkbox on every mail (Escape or "Stop selecting" hides them again). Tick several mails and
mark them seen or unseen, move them to the account's Trash folder, or move them to any folder
on the mail server. Portier mirrors only Inbox, Sent, Junk, Archive and Trash, so mail moved elsewhere
leaves its views and stays on the server.

Rows in the mail lists and search results can also be slid left or right, by finger or mouse.
Each direction's actions (seen/unseen, archive, move to folder, move to Trash) are chosen per user under
Settings: one action is performed on release, several are offered as buttons.

Connects to existing mailboxes over IMAP and SMTP (password or app password). Several users
can share one installation; each has their own accounts and sender decisions.

## Build and run

    cd web && npm install && npm run build && cd ..
    cargo build --release
    ./target/release/portier

The frontend is embedded in the binary, so build `web/` before `cargo build`. Open
http://127.0.0.1:8080. The first user to register manages the installation and can add
further users under Settings.

## Installing it as an app

Served over HTTPS, the web client can be installed as an app: in Chrome and Edge with the install
button in the address bar, in Safari with "Add to Dock" (Mac) or "Add to Home Screen" (iPhone,
iPad). It then has its own icon and window, without the browser's bars.

The app itself (scripts, styles, fonts, icons) is stored on the device, so it opens at once and
also without the server. Mail is never stored there: it is always asked from the server. When the
server cannot be reached (no connection, VPN off) the app says so and starts by itself once it
can.

## Terminal client

`portier-tui` shows the same lists and mails in a terminal. It talks to a running Portier server
and needs nothing else.

    cargo build --release -p portier-tui
    ./target/release/portier-tui --url http://127.0.0.1:8080

The first start asks for the server address, email and password; after that the session is kept
in `~/.config/portier/session.json` (readable by you only; `--logout` forgets it).

It shows the side bar with its counts, every list (Home with its Unseen, Important and Seen
areas), the conversation view, split view beside or below the list, and live updates. Keys are
those of the web client where they exist (`?` lists them): arrows for the next and previous mail,
Enter, Space and Backspace to page, `Shift+H` / `I` / `D` / `N` for the lists, `s` for the split
view, `o` to open the mail in the web browser.

Mail is acted on with the same letters: `a` archive, `d` move to Trash, `u` unseen, `i` important,
`z` delay (then `1`, `2`, `3` or `7`), `t` note; also `b` to move mail back out of the Trash and
`m` to move it to a folder. They apply to the opened mail, to the one under the cursor, or, after
`v`, to the mails ticked with Space. In the Screener, Enter reads a waiting sender's mail and
`i`, `n` or `Shift+J` sends them to Home, Nice to know or Junk.

`/` searches with the same filters as the web client (`from:`, `note:`, `received:` …); each
result names the list it is found in. `Shift+F` lists all mail from the sender of a mail,
`Shift+S` saves the shown search in the side bar (with its count of unseen mail) and `Shift+X`
removes it. `Shift+U` narrows Nice to know, Junk and Archive to unseen mail, and `Shift+P` reads
all mails of a list or of the search results on one page.

`c` writes a new mail, `r` replies, `Shift+R` replies to all, `f` forwards, and Enter on a draft
takes it up again. Tab walks through the fields; known recipients are offered while typing in To
(arrows pick, Enter takes). `Ctrl+S` sends (`Ctrl+Return` too, where the terminal tells it from
Return), Esc saves the draft and leaves, `Ctrl+X` twice discards it. `Ctrl+A` attaches a file by
its path, `Ctrl+O` shows the mail being answered, and `Ctrl+E` hands the text to your own editor
(`$VISUAL`, `$EDITOR`, else vi). The text is plain and is sent as simple paragraphs; a draft that
was given formatting in the web client loses it when edited here.

`Shift+A` lists the attachments of the opened conversation: Enter opens one with the program
your computer uses for it, `s` saves it to Downloads (never over an existing file).

In terminals that draw pictures (kitty, iTerm2, WezTerm, Ghostty, and others with sixel) the
sender's picture is shown on the opened mail, and Enter on an image attachment shows the image
in the terminal. `PORTIER_TUI_IMAGES` changes that: `off` shows none, `on` tries in a terminal
the client does not know, `blocks` draws coarse pictures from block characters anywhere.

Mails written as HTML are shown as text with their links; designed layouts and pictures inside
the mail text are not. `o` opens such a mail in the browser. Mail accounts, users and the other
settings are managed in the web client.

## Configuration (environment)

The app was called emscreen before. Each variable is still read under its earlier name too
(`EMSCREEN_BIND` and so on) when the `PORTIER_` one is not set. The database file `emscreen.db`
and the session cookie keep the earlier name, so an existing installation goes on working
unchanged.

| Variable | Default | Meaning |
|---|---|---|
| `PORTIER_BIND` | `127.0.0.1:8080` | Listen address |
| `PORTIER_DATA_DIR` | `data` | SQLite database, raw mail, drafts, key file |
| `PORTIER_MASTER_KEY` | generated in `<data dir>/master.key` | Base64 of 32 bytes; encrypts mail account passwords |
| `PORTIER_OPEN_REGISTRATION` | off | `1` lets anyone create a user |
| `PORTIER_SYNC_MAX_PER_FOLDER` | `5000` | Newest messages mirrored per folder |
| `PORTIER_SOFFICE` | found automatically | Path to LibreOffice's `soffice`, or `off` |
| `PORTIER_AVATARS` | on | `off` stops looking up sender pictures |
| `PORTIER_TLS_CERT`, `PORTIER_TLS_KEY` | none | Certificate chain and private key (PEM files): the server then speaks HTTPS |

A user who is locked out gets a new password on the server itself:

    ./target/release/portier reset-password you@example.org

It prints a random password and ends that user's sessions; the user then chooses their own under
Settings.

### Reaching it from other machines

By default Portier listens on this machine only and speaks plain HTTP. For other machines it
needs HTTPS: your login and all your mail pass over the connection.

    PORTIER_BIND=0.0.0.0:8443 \
    PORTIER_TLS_CERT=/path/to/fullchain.pem \
    PORTIER_TLS_KEY=/path/to/privkey.pem \
    ./target/release/portier

- `PORTIER_BIND`: `0.0.0.0` listens on every network of the machine. To serve one network only,
  give that network's own address instead, e.g. the machine's VPN address.
- The certificate has to be issued for the name you type into the browser. Any source works:
  Let's Encrypt, your VPN (`tailscale cert machine.your-tailnet.ts.net` writes both files), or a
  certificate authority of your own.
- The files are read again twice a day, so a renewed certificate is picked up without a restart.
- With HTTPS on, the session cookie is marked Secure. Sign in again after switching.
- The terminal client connects with `--url https://name:8443` and trusts the certificate
  authorities your system trusts.

On macOS, `contrib/launchd/` has two launchd agents to copy into `~/Library/LaunchAgents` and
adapt: one keeps Portier running from login on, the other asks Tailscale for the certificate
at login and once a week, which renews it when it is due. Load each with
`launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/<file>`.

The agents run a copy of the binary in `~/Library/Application Support/Portier`, with the data
folder beside it, not the one in the build folder. `contrib/launchd/deploy.sh <label>` builds,
copies the new binary there and restarts the agent. (A service run straight from a build folder
on an external volume stops at a macOS permission dialog after every rebuild.)

A reverse proxy that adds HTTPS in front of the plain port works as well. Either way, keep
`master.key` with your backups: without it the stored account passwords cannot be read.

## Sender pictures

Instead of initials, a sender is shown with their Gravatar picture if they have one, otherwise
with the BIMI logo their mail domain publishes in DNS. What is found, and that nothing was found,
is cached in the database (pictures for 30 days, misses for 3), so each sender is looked up rarely.

These lookups leave your server: Gravatar receives a hash of the sender's address, your DNS
resolver sees the sender's domain, and the logo is fetched from the address the domain names.
Logo addresses must be https and resolve to public addresses only. Set `PORTIER_AVATARS=off`
to keep initials and make no such requests.

## Previews of Office documents

Word, Excel, PowerPoint and OpenDocument attachments are shown by converting them to PDF with
LibreOffice. It is optional: without it those files appear as plain tiles and can be downloaded.

    brew install --cask libreoffice      # macOS
    apt install libreoffice-core libreoffice-writer libreoffice-calc libreoffice-impress

Portier looks for `soffice` on the PATH and in `/Applications/LibreOffice.app` at start-up.
Conversion runs headless, one document at a time, on a copy of the file in a throwaway directory.
It still means LibreOffice opens files that strangers sent you. Set `PORTIER_SOFFICE=off` if
you do not want that.

## Development

    cargo run                       # API on :8080
    cd web && npm run dev           # UI with hot reload, proxies /api to :8080
    cargo test && cargo clippy --all-targets -- -D warnings
    cd web && npm run check

## Search

The field in the top bar searches subject, sender, recipients and text, and takes filters:

| Filter | Finds |
|---|---|
| `from:carsten` | sender name or address contains "carsten" |
| `to:anna` | a recipient contains "anna" |
| `subject:invoice`, `title:invoice` | subject contains "invoice" |
| `note:tax` | your note on the conversation contains "tax"; `note:` alone finds every conversation with a note |
| `attachment:true`, `attachment:false` | mail with, or without, attachments |
| `received:last month` | also `today`, `yesterday`, `this week`, `last week`, `this month`, `this year`, `last year` |
| `received:01.09.2026..01.10.2026` | from first to last day, both included; also `2026/09/01..2026/10/01`, a single day, or an open end |

Different filters and words narrow the search together: `hallo from:carsten received:last month`.
The same filter given several times means either: `from:anna from:carsten` finds mail from Anna or
from Carsten.
Values with spaces go in quotes: `from:"Carsten Meier"`. Days are counted in the server's time zone.

Nice to know, Junk and Archive have an "Unseen only" switch that narrows the list to conversations
with unseen mail; it is kept per browser and per list.

In a mail list, the person icon at the end of a row shows all mail from that row's sender, as a
`from:` search you can refine or save.

Each result names the list it is found in (Home, Important, Delayed, Nice to know, Screener,
Archive, Sent, Junk).

A search can be saved under a name with "Save this search" on the results page. Saved searches
are listed in the side bar with the number of unread mails they currently find, and are run afresh
each time, so `received:last month` keeps meaning the previous month.

## Sender pictures

Senders show a picture looked up for their address (Gravatar, or their domain's BIMI logo). With
"Picture", on an opened mail next to the sender or in Settings in the list of senders, you choose
one of your own instead: an image file from your computer, or the web address of a picture.
Portier keeps a small copy and shows it for that sender; "Remove your picture" goes back to
the looked-up one.

## Look

Settings has four colour themes (Harbour, Forest, Plum, Graphite), each light and dark; "Like the
system" follows the operating system. The choice is kept per browser.

## Keyboard

`?` shows all keyboard shortcuts. `c` write, `/` search field, `m` menu, `Shift+H` or `1` Home, `Shift+I` Important,
`Shift+D` Delayed, `Shift+N` or `3` Nice to know, `2` Screener, `4` Attachments.
In the composer `Ctrl+Return` (or `Cmd+Return`) sends the mail.
In the attachment viewer the left and right arrows step through files and Escape closes it.
In the Screener `i` / `k` / `j` decide the first sender. In a conversation `r` reply, `u` mark as unseen, `i` important, `z` delay (then `1`, `2`, `3` or `7`), `a` archive, `d` move to Trash,
`Shift+R` reply all, `f` forward.
Arrow down and up go to the next and previous mail: in a conversation, and on a list in split
view; on a list without split view, and in search results (also straight from the search field),
they move from row to row and Enter opens. Space pages down
in the mail, Backspace (or Shift+Space) pages up; Ctrl+Down and Ctrl+Up do the same.
