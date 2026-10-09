<p align="center">
  <img src="assets/icons/icon.png" alt="Venus" width="80" height="80">
</p>

<h1 align="center">Venus</h1>

<p align="center">
  <strong>Your Insomnia collections, in a native Rust app.</strong>
</p>

Venus is a lightweight, native API client for Insomnia users who want to work
with their existing cloud collections without running Insomnia. Built in Rust,
with browser sign-in, request and environment sync, and commit previews.

- Sign in to Insomnia in your browser and pick a collection; no export needed.
- Pull requests and environments from master, including base inheritance.
- Review saved changes, write a commit message, and push to share with teammates.
- Keep working with local JSON requests and dotenv files when you do not need sync.

[Installation](#installation) · [Cloud sync guide](INSOMNIA_SYNC.md) ·
[Documentation](website/docs/getting-started.md) ·
[Source](https://github.com/alvezgr/venus)

## Installation

Build this checkout to use Venus's Insomnia features. Mercury's installers and
Homebrew cask install the upstream app, not Venus.

From the repository root:

```bash
cargo build --release
./target/release/venus
```

On Linux, if Wayland cannot create a window, run:

```bash
env -u WAYLAND_DISPLAY WINIT_UNIX_BACKEND=x11 ./target/release/venus
```

The desktop launcher below applies this X11 workaround. The native app is Rust;
the optional documentation website has a separate JavaScript toolchain.
OpenSSL development libraries and `pkg-config` are required for the cloud bridge.
Linux has been verified; the Insomnia bridge has not yet been verified on macOS
or Windows.

<details>
<summary><strong>Linux: build this checkout and install as an app</strong></summary>

Run these commands from the repository root to install your local version,
including this fork's Insomnia Cloud Sync features:

```bash
cargo build --release

mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons
install -m 755 target/release/venus ~/.local/bin/venus
install -m 644 assets/icons/icon.png ~/.local/share/icons/venus.png

cat > ~/.local/share/applications/venus.desktop <<EOF
[Desktop Entry]
Type=Application
Name=Venus
Comment=API client with Insomnia Cloud Sync
Exec=/usr/bin/env -u WAYLAND_DISPLAY WINIT_UNIX_BACKEND=x11 $HOME/.local/bin/venus
Icon=$HOME/.local/share/icons/venus.png
Terminal=false
Categories=Development;Network;
EOF
```

Close older Venus windows, then search **Venus** in your applications menu.
The launcher uses X11 through XWayland on a Wayland desktop, to avoid the
Glutin window-creation error encountered with Wayland on this setup.

After code changes, close Venus and update the installed binary:

```bash
cargo build --release
install -m 755 target/release/venus ~/.local/bin/venus
```

The launcher and icon only need to be installed once. These commands install
under your home directory and do not require `sudo`.

</details>

## Existing Mercury data

Venus stores app state, history and its Insomnia login in `~/.venus/`. On first
read, missing files are copied from `~/.mercury/`; existing Venus files always
win, and the Mercury originals remain intact. Your workspace folders and
`.insomnia-sync` baselines keep working without changes.

Set `VENUS_HOME` for an isolated profile. `MERCURY_HOME` remains a compatibility
alias; `VENUS_HOME` takes priority. Neither override imports your normal profile.
Venus's executable, desktop launcher and application identifier are separate
from Mercury's.

## Your requests are files

A workspace is just a folder. Subfolders are collections, and every `*.json` file is one request:

```
api/
├── .env.dev
├── .env.production
├── users/
│   ├── list-users.json
│   └── create-user.json
└── health.json
```

```json
{
  "method": "POST",
  "url": "{{BASE_URL}}/users",
  "headers": {
    "Authorization": "Bearer {{TOKEN}}",
    "Content-Type": "application/json"
  },
  "body": "{\"name\": \"Ada\"}"
}
```

Grep it, diff it, commit it, edit it in VS Code — Venus picks up outside edits live. `headers` and `body` are optional, and headers are written in sorted order so git diffs stay clean.

**Environments** are `.env` files in the workspace root. Pick one from the top-right menu or cycle with `⌘ E`; `{{NAME}}` is substituted into the URL, headers and body when you send. Undefined variables are flagged on the tab that holds them. Production shows in red, staging in amber.

```bash
# .env.dev
BASE_URL=http://localhost:3000
TOKEN="dev token"
```

---

## Why Venus

- **Native Rust app.** Rust and egui render the interface without Electron.
- **Your Insomnia collections.** Sign in through your browser, select a cloud collection, and sync requests and environments without running Insomnia.
- **Review before sharing.** Preview changes and write a cloud commit message before pushing to master.
- **Local workspaces.** Requests stay in editable JSON files. Ordinary `.env` files remain local; cloud sync runs when you choose it.
- **Keyboard first.** `⌘ K` opens a palette over every request and command, `⌘ F` finds anything in a response, `?` lists the rest.
- **No setup tax.** Type `localhost:3000/api`, pick JSON or Form, hit send. Venus fills in the scheme, the `Content-Type` and the decoding so the first request works.
- **Light or dark.** Follows your system theme, or `⌘ D` pins one. Vector icons and bundled fonts, so it looks the same on every OS.

### What's in it

- **Collections** — create, rename, duplicate and delete from the sidebar; outside edits sync live and changes auto-save.
- **Auth** — Basic, Bearer or a custom `Authorization` value, always in sync with the Headers tab.
- **Query params** — a table that stays in sync with the URL, both ways.
- **cURL** — paste a `curl …` command into the URL bar to import it; `⌘ ⇧ C` copies the request back out.
- **Import** — Postman v2.1 and Insomnia (JSON or YAML), with their variables, auth and bodies.
- **Responses** — pretty-printed and highlighted JSON, XML and HTML; headers and cookies; `⌘ F` to search; save images, binaries or large bodies to a file.
- **History** — your last 50 requests from the past 7 days, with full responses. **Recent** keeps unsaved requests you've sent.
- **Cookies** — kept for the session, so login flows just work.

---

## Shortcuts

`⌘` is `Ctrl` on Windows and Linux. Press `?` in the app for this list.

| Shortcut | Action |
|----------|--------|
| `⌘ ⏎` | Send request |
| `⌘ K` | Command palette |
| `⌘ N` | New request |
| `⌘ S` | Save request |
| `⌘ O` | Open folder |
| `⌘ L` | Focus URL bar |
| `⌘ E` | Next environment |
| `⌘ H` | Toggle history |
| `⌘ R` | Toggle raw response |
| `⌘ D` | Toggle light / dark |
| `⌘ F` | Find in response |
| `⌘ ⇧ C` | Copy as cURL |
| `⌘ ⇧ F` | Focus mode |
| `?` | Keyboard shortcuts |
| `Esc` | Close find / cancel request / close dialog / clear filter |

`Tab` moves between controls and `Space` activates the one you land on, so everything is reachable without a mouse.

---

## Defaults

There's no settings screen. These are fixed:

| | |
|---|---|
| Timeout | 30 seconds |
| Redirects | Followed (up to 10) |
| Largest response downloaded | 10 MB |
| Largest body shown inline | 148 KB (bigger ones offer **Save**) |
| App data | `~/.venus/` (session, recent, history). Set `VENUS_HOME` to use another folder. |

---

## Insomnia Cloud Sync

Venus connects to your existing Insomnia Cloud collections using a Rust bridge.
Venus's code stays local; request and environment changes sync with Insomnia's
**master**. Choose **Sync → Browse cloud collections…** to sign in through your
browser or use the installed Insomnia login, pick a collection, and download
master into an empty folder. No export is needed. In a fetched workspace,
click the sync arrows and **master** in the bottom bar to pull or push directly.
For the initial fetch or terminal commands, close Venus first:

```bash
cargo build
./target/debug/venus --insomnia-fetch <cloud-project-id> <workspace-folder>
./target/debug/venus --insomnia-collections
./target/debug/venus --insomnia-plan <cloud-project-id> <workspace-folder>
./target/debug/venus --insomnia-push <cloud-project-id> <workspace-folder>
```

Open the fetched folder in Venus to edit. Fetch refuses unpushed edits; push
checks cloud conflicts. Cloud environments are downloaded and their edits are
synced, including base inheritance. Request files include
an optional `insomnia_id` that Venus preserves on save and clears on duplicate.
Build dependencies include OpenSSL development libraries. See
[INSOMNIA_SYNC.md](INSOMNIA_SYNC.md) for this collection's commands, verification,
recovery and execution limits.

---

## Contributing

PRs are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). The repo is set up for Claude Code: [CLAUDE.md](CLAUDE.md) holds the conventions, and `/fix-issue <number>` runs the whole fix-to-merge loop.

```bash
cargo run      # run the app
cargo test     # unit tests + a headless UI smoke test
```

## Origin and license

Venus is an independent fork of [Mercury](https://github.com/Harry-kp/mercury),
created by Harshit Chaudhary (@Harry-kp). It adds an Insomnia-focused workflow.
The original MIT license and copyright are retained in [LICENSE](LICENSE).

---

<p align="center">
  Based on Mercury by<br>
  <a href="https://github.com/Harry-kp">@Harry-kp</a>
</p>
