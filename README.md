# SlapWindows 🖐️💻

Slap your desk. Hear a sound. That's it.

SlapWindows listens to your microphone, detects desk slaps using frequency analysis, and plays satisfying sound effects in response. Built with Tauri, React, and Rust.

## Features

- 🎤 Real-time microphone monitoring with FFT-based slap detection
- 🔊 Multiple sound packs (classic, meme, cid, and more)
- 📦 Import your own custom sound packs
- 🎯 Adjustable sensitivity, slap filter, and cooldown
- 🔄 Cycle, random, or single playback modes
- 🖥️ System tray with slap counter
- 🚀 Auto-launch on startup
- 🎨 Terminal-themed hacker UI

## Download

Grab the latest release from the [Releases](https://github.com/ad1tyayadav/slapWindows/releases) page:

### Windows
- **`.msi`** — Standard installer (recommended)
- **`.exe`** — NSIS installer

### Linux
- **`.AppImage`** — Universal, runs on any distro (recommended)
- **`.deb`** — For Debian/Ubuntu (`sudo dpkg -i slapwindows_x.x.x_amd64.deb`)
- **`.rpm`** — For Fedora/RHEL (`sudo rpm -i slapwindows-x.x.x.x86_64.rpm`)

> **Linux Note:** Make sure your user has microphone permissions. On some distros you may need to add yourself to the `audio` group.

## Build from Source

### Prerequisites
- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/) (stable)
- [Tauri CLI](https://tauri.app/start/) (`npm install -g @tauri-apps/cli`)

### Windows
```bash
npm install
npm run tauri build
```

### Linux
Install system dependencies first:
```bash
# Debian/Ubuntu
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf \
  libasound2-dev \
  libgtk-3-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev

# Fedora
sudo dnf install -y \
  webkit2gtk4.1-devel \
  libappindicator-gtk3-devel \
  librsvg2-devel \
  patchelf \
  alsa-lib-devel \
  gtk3-devel \
  libsoup3-devel \
  javascriptcoregtk4.1-devel
```

Then build:
```bash
npm install
npm run tauri build
```

Build output will be in `src-tauri/target/release/bundle/`.

## Data Locations

| Platform | Config | Custom Sounds |
|----------|--------|---------------|
| Windows | `%APPDATA%\slapwindows\config.json` | `%APPDATA%\slapwindows\sounds\` |
| Linux | `~/.config/slapwindows/config.json` | `~/.local/share/slapwindows/sounds/` |

## Releasing a New Version

1. Update the version in `package.json` and `src-tauri/tauri.conf.json`
2. Commit your changes
3. Create and push a git tag:
   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```
4. GitHub Actions will automatically build for **both Windows and Linux** and create a Release with all installers attached

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
