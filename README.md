# emscreen

Self-hosted web mail client that screens mail by sender. Each new sender waits in the
Screener until you put them in one of three places:

- **Important**: conversations you want to see, split into new and previously seen
- **Nice to know**: newsletters and updates, shown open in one scrolling page
- **Junk**: hidden, and moved to the Junk folder of the mail account

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

Serve it behind HTTPS (reverse proxy) when it is reachable from other machines. Keep
`master.key` with your backups: without it the stored account passwords cannot be read.

## Development

    cargo run                       # API on :8080
    cd web && npm run dev           # UI with hot reload, proxies /api to :8080
    cargo test && cargo clippy --all-targets -- -D warnings
    cd web && npm run check

## Keyboard

`c` write, `/` search, `m` menu, `1` Important, `2` Screener, `3` Nice to know.
In the Screener `i` / `k` / `j` decide the first sender. In a conversation `r` reply,
`a` reply all, `f` forward.
