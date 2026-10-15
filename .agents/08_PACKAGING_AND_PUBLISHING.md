# Packaging & Open-Source Publishing

This document covers how to package, release, and distribute **EVE Trade Scout**
as a polished open-source application ready for others to install and use.

---

## Distribution Options (Overview)

| Method | Who installs it | Command to install | Notes |
|---|---|---|---|
| **crates.io** | Any Rust user | `cargo install eve-trade-scout` | Easiest, builds from source |
| **GitHub Releases** (Linux) | Any Linux user | Download `.tar.gz` & extract | Standalone binary + launcher |
| **GitHub Releases** (Windows) | Any Windows user | Download `.zip` & extract | Standalone `.exe` binary |
| **AUR** (Arch) | Arch / Manjaro users | `yay -S eve-trade-scout` | Community maintained |
| **RPM / DNF** | Fedora users | `dnf install eve-trade-scout` | Native Fedora package |

**Recommended minimum:** crates.io + GitHub Releases binary + GitHub Actions CI
**Nice to have:** AppImage (broadest Linux reach, zero friction)

---

## 1. crates.io — `cargo install`

The Rust ecosystem's official package registry. Users with Rust installed can do:

```bash
cargo install eve-trade-scout
# Compiles from source, installs to ~/.cargo/bin/eve-merchant
```

### What's needed

1. Create a free account at [crates.io](https://crates.io)
2. Authenticate locally:
   ```bash
   cargo login <your-api-token>
   ```
3. Ensure `Cargo.toml` has all required metadata:

```toml
[package]
name        = "eve-trade-scout"
version     = "0.1.0"
edition     = "2021"
description = "EVE Online location-centric arbitrage route planner & multi-item cargo knapsack optimizer"
license     = "GPL-3.0-or-later"
repository  = "https://github.com/Flambello/eve-traveling-merchant"
homepage    = "https://github.com/Flambello/eve-traveling-merchant"
readme      = "README.md"
keywords    = ["eve-online", "trading", "arbitrage", "route-planner", "flathub"]
categories  = ["games", "gui", "command-line-utilities"]
exclude     = [".agents/", "data/", "cache/", "*.sqlite*", "*.bz2", ".github/"]
```

4. Publish:
   ```bash
   cargo publish
   ```

> **Note:** The SDE SQLite database (~650 MB) is **excluded** from the crate.
> It is downloaded automatically on first run — this is the correct approach.

---

## 2. GitHub Releases — Pre-compiled Binaries

Users without Rust can download a ready-to-run binary.

### Automated with GitHub Actions

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags:
      - "v*"   # triggers on any tag like v1.0.0

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: eve-merchant
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu    # ARM64 (Raspberry Pi, servers)
            artifact: eve-merchant
            cross: true                          # use cross-rs for cross-compilation

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross (if needed)
        if: matrix.cross
        run: cargo install cross

      - name: Build release binary
        run: |
          if [ "${{ matrix.cross }}" = "true" ]; then
            cross build --release --target ${{ matrix.target }}
          else
            cargo build --release --target ${{ matrix.target }}
          fi

      - name: Package
        run: |
          TARGET="${{ matrix.target }}"
          BINARY="target/$TARGET/release/${{ matrix.artifact }}"
          ARCHIVE="eve-traveling-merchant-$TARGET.tar.gz"
          tar -czf "$ARCHIVE" -C "target/$TARGET/release" "${{ matrix.artifact }}"
          echo "ARCHIVE=$ARCHIVE" >> $GITHUB_ENV

      - name: Upload to GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: ${{ env.ARCHIVE }}
```

### How to trigger a release

```bash
git tag v1.0.0
git push origin v1.0.0
```

GitHub Actions will:
1. Build binaries for x86_64 Linux and ARM64 Linux
2. Create a GitHub Release with the tag
3. Attach the `.tar.gz` archives automatically

Users can then:
```bash
# Download
wget https://github.com/Flambello/eve-traveling-merchant/releases/latest/download/eve-traveling-merchant-x86_64-unknown-linux-gnu.tar.gz
tar -xzf eve-traveling-merchant-x86_64-unknown-linux-gnu.tar.gz
chmod +x eve-merchant
./eve-merchant
```

---

## 3. AppImage — Universal Linux Bundle

An AppImage is a self-contained single-file application that runs on **any** Linux distro
without installation. Perfect for users who don't want to use a package manager.

### Build with `cargo-appimage`

```bash
# Install the tool
cargo install cargo-appimage

# Build the AppImage
cargo appimage
# → produces: target/appimage/eve-merchant.AppImage
```

Or use **`appimagetool`** manually for more control:

```bash
# Install appimagetool
wget https://github.com/AppImage/AppImageKit/releases/latest/download/appimagetool-x86_64.AppImage
chmod +x appimagetool-x86_64.AppImage

# Create AppDir structure
mkdir -p AppDir/usr/bin
cp target/release/eve-merchant AppDir/usr/bin/

# Create .desktop file
cat > AppDir/eve-merchant.desktop << EOF
[Desktop Entry]
Name=EVE Traveling Merchant
Exec=eve-merchant
Icon=eve-merchant
Type=Application
Categories=Game;Utility;
EOF

# Add an icon (128x128 PNG)
cp assets/icon.png AppDir/eve-merchant.png

# Package
./appimagetool-x86_64.AppImage AppDir EVETravelingMerchant-x86_64.AppImage
```

Add the AppImage build to the GitHub Actions release workflow to attach it to every release.

---

## 4. Multi-Platform Standalone Bundles (Linux & Windows)

The project ships zero-dependency standalone release archives for Linux and Windows:

### Linux Bundle (`eve-traveling-merchant-linux-x86_64.tar.gz`)
- Pre-compiled optimized release binary (`eve-merchant`) — launches Desktop GUI directly by default
- Native desktop entry (`eve-trade.desktop`) and Freedesktop icons
- Shell launcher script (`start-gui.sh`)
- Full documentation and license files

### Windows Bundle (`eve-traveling-merchant-windows-x86_64.zip`)
- Pre-compiled optimized release executable (`eve-merchant.exe`) compiled with Windows GUI subsystem (`windows_subsystem = "windows"`), launching the native GUI window directly without background console prompts
- Batch launcher helper (`start-gui.bat`)
- Embedded assets, application icons, and documentation
- Portable zero-install execution directly on Windows 10/11

---

## 5. README.md — Required for Open Source

The README is the storefront. It should include:

```markdown
# EVE Trade Scout 🚀

> Find the most profitable arbitrage trades in EVE Online, route-planned
> by profit per jump.

## Features
- 🏷️ Live market data via official ESI API
- 🗺️ Multi-stop route planner (maximise profit per jump)
- 🚢 Big Hauler (60 000 m³ / 500 M ISK) and Small Hauler (30 000 m³ / 50 M ISK) profiles
- 🔒 Safe-route mode (highsec only)
- 💻 Single compiled binary — no dependencies, just run it
- 🐧 Linux native (Fedora, Ubuntu, Arch, etc.)

## Install

### Option 1 — Download binary (no Rust required)
[Download latest release](https://github.com/YOUR_USERNAME/eve-trade-scout/releases/latest)

### Option 2 — Install via cargo
\`\`\`bash
cargo install eve-trade-scout
\`\`\`

### Option 3 — Build from source
\`\`\`bash
git clone https://github.com/YOUR_USERNAME/eve-trade-scout
cd eve-trade-scout
cargo build --release
./target/release/eve-trade
\`\`\`

## Usage
\`\`\`bash
eve-trade                    # interactive TUI
eve-trade --ship big --system Jita --safe
eve-trade --route --top 10
\`\`\`

## License
GPL-3.0 — see [LICENSE](LICENSE)

## Disclaimer
This tool is a read-only market analysis aid. It does not automate in-game actions
and complies with the EVE Online EULA and ESI Developer Licence.
```

---

## 6. License File

For open source, add a `LICENSE` file to the repo root. **GNU General Public License v3.0 (GPL-3.0)** provides
strong copyleft protection:

```
MIT License

Copyright (c) 2026 YOUR_NAME

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 7. CI — Continuous Integration (GitHub Actions)

Add a second workflow for pull request validation:

```yaml
# .github/workflows/ci.yml
name: CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test
      - run: cargo clippy -- -D warnings
      - run: cargo fmt --check
```

---

## 8. Recommended File Structure for Publishing

```
eve-trade-scout/          ← GitHub repo root
├── .github/
│   └── workflows/
│       ├── ci.yml        ← runs on every push/PR
│       └── release.yml   ← runs on version tags
├── .agents/              ← your planning docs
├── src/                  ← Rust source
├── tests/
├── assets/
│   └── icon.png          ← 128x128 app icon
├── Cargo.toml
├── Cargo.lock            ← ALWAYS commit this for binaries
├── LICENSE               ← GPL-3.0
├── README.md
└── .gitignore
```

> **Important:** For binary crates, always commit `Cargo.lock`.
> This ensures reproducible builds — anyone who clones the repo
> builds the exact same dependency versions.

---

## Summary: Recommended Publishing Checklist

- [ ] Push to GitHub (public repo)
- [ ] Add `LICENSE` (GPL-3.0)
- [ ] Write `README.md` with install instructions + screenshot/demo
- [ ] Add `.github/workflows/ci.yml` (auto-test on push)
- [ ] Add `.github/workflows/release.yml` (auto-build binaries on tag)
- [ ] Fill in all `Cargo.toml` metadata fields
- [ ] `cargo publish` to crates.io
- [ ] `git tag v1.0.0 && git push origin v1.0.0` to trigger first release
- [ ] (Optional) Build AppImage and attach to release
- [ ] (Optional) Submit to Flathub
