---
title: Getting Started
sidebar_label: Getting Started
sidebar_position: 1
---

# Getting Started

Venus is a native Rust API client for Insomnia users who want to use their cloud
collections without running Insomnia. It is an independent fork of
[Mercury](https://github.com/Harry-kp/mercury), retaining its MIT license.

- **Your existing collections.** Browser sign-in, collection discovery, request
  and environment sync, and cloud commit previews.
- **Local files.** Requests remain editable JSON files. Cloud sync is optional;
  ordinary dotenv files stay local.
- **Native app.** The desktop client is Rust and egui. The optional docs website
  has its own JavaScript build tools.

## Installation

Build the [Venus repository](https://github.com/alvezgr/venus) from source. The
upstream Mercury installers do not include Venus's Insomnia features.

```bash
git clone https://github.com/alvezgr/venus.git
cd venus
cargo build --release
```

Run `./target/release/venus` on Linux or macOS, or `target\release\venus.exe` on
Windows. The Insomnia bridge has been verified on Linux; other platforms still
need verification.

### Linux: add to the app launcher

Run from the repository root:

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

Close older windows, then search **Venus** in your app menu. The launcher uses
X11 through XWayland to avoid the Glutin Wayland window-creation error observed
on this setup. It installs under your home directory without `sudo`.

After code changes, close Venus and update the installed binary:

```bash
cargo build --release
install -m 755 target/release/venus ~/.local/bin/venus
```

### Build dependencies

OpenSSL development libraries and `pkg-config` are needed for the Rust cloud
bridge. The Linux GUI development packages used in CI are listed in
`.github/workflows/ci.yml`.

### Linux: runtime libraries

The Linux binary links against the system GUI libraries. On a minimal install you may need:

```bash
# Debian / Ubuntu
sudo apt install libgtk-3-0 libxkbcommon0 libxcb-render0 libxcb-shape0 libxcb-xfixes0 libssl3

# Fedora
sudo dnf install gtk3 libxkbcommon libxcb openssl-libs
```

If Venus exits immediately with an error mentioning a missing `.so` file, that library is the one to install.


## Connect to Insomnia

Open **Sync → Browse cloud collections…**. Sign in through the browser, paste the
authentication code into Venus, and select a collection and empty local folder.
No collection export or running Insomnia app is needed. An installed Insomnia
login can also be used when available.

Pull from master to download updates. Push opens a preview and message box;
confirm the push to share saved request and environment changes with your team.
See [Cloud Sync](/docs/features/import-export#local-insomnia-cloud-bridge-linux).

## Existing Mercury data

Venus uses `~/.venus/` for state, history and saved login. Missing files are copied
from `~/.mercury/` when first read. Existing Venus files win, including signed-out
login markers; Mercury originals and workspace folders remain intact.
`VENUS_HOME` selects an isolated profile. `MERCURY_HOME` remains a compatibility
alias, with `VENUS_HOME` taking priority.

## Next steps

- [Quick Start](/docs/quickstart): send, save and organize requests
- [Requests](/docs/features/requests): editor, params, headers, body and responses
- [Environments](/docs/features/environments): local dotenv and cloud JSON environments
- [Keyboard Shortcuts](/docs/reference/keyboard-shortcuts)
