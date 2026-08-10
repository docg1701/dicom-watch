# PRD — DicomWatch

Product Requirements Document.
Covers implementation roadmap for v0.5.0, v0.6.0, and v0.7.0.

---

## v0.5.0 — i18n: English / Português (Brasil)

### Goal

User can switch between English and Brazilian Portuguese at runtime via a UI toggle.
No restart, no config file edit — instant switch.

### Research

The Rust i18n ecosystem offers several approaches:

| Crate | Approach | Best for |
|-------|----------|----------|
| [`rust-i18n`](https://docs.rs/rust-i18n) | Compile-time codegen, `t!` macro, YAML/JSON/TOML | Simple key-value, small projects |
| [`fluent`](https://crates.io/crates/fluent) / [`fluent-bundle`](https://crates.io/crates/fluent-bundle) | Mozilla's Fluent system | Complex pluralization, gender, references |
| [`i18n-embed`](https://crates.io/crates/i18n-embed) | Runtime locale loading via `fluent` | Hot-reload, multiple backends |

For DicomWatch, `rust-i18n` is the right fit:
- Small set of UI strings (~50 keys)
- Two languages only
- Compile-time = no runtime file loading = simpler distribution
- `t!` macro integrates cleanly with Iced's `text()` widget

### Implementation

#### 1. Add dependency

```toml
[dependencies]
rust-i18n = "3"
```

#### 2. Create locale files

```
src/i18n/
├── en.toml
└── pt-BR.toml
```

Example `en.toml`:

```toml
[watch]
start = "Start"
stop = "Stop"
watching = "● Watching"
idle = "○ Idle"
clear_all = "Clear All Files"
settings = "Settings"
source_dir = "Source directory"
dest_dir = "Destination directory"
filter_mode = "Filter mode"
pattern = "Pattern"
sound_alert = "Sound alert"
activity_log = "Activity Log"
waiting = "Waiting for files..."
browse = "Browse"
invalid_regex = "Invalid regex: {error}"
source_missing = "Source directory does not exist: {path}"
dest_missing = "Destination directory does not exist: {path}"
sound_missing = "Sound file not found: {path}"
log_new_file = "New file detected: {name}"
log_extracted = "Extracted {count} file(s) to '{path}'"
log_removed = "Original archive removed."
log_done = "Done."
log_failed = "Failed to extract '{path}': {error}"
log_vanished = "File vanished before processing: {name}"
log_watcher_stopped = "Watcher stopped."
log_watcher_error = "Watch error: {error}"
```

`pt-BR.toml` — same keys, translated values.

#### 3. Define i18n module

```rust
// src/i18n.rs
rust_i18n::i18n!("src/i18n");

pub use rust_i18n::t;
pub const LOCALES: &[&str] = &["en", "pt-BR"];
pub const DEFAULT_LOCALE: &str = "en";
```

#### 4. Add locale to AppState

```rust
struct AppState {
    ...
    locale: String,  // "en" | "pt-BR"
}
```

#### 5. Add message variant

```rust
enum Message {
    ...
    ToggleLocale,
    SetLocale(String),
}
```

#### 6. Add locale toggle button

Place in actions row or as a small gear/flag icon.
On click: cycle between available locales.

```rust
Message::ToggleLocale => {
    let new = if self.locale == "en" { "pt-BR" } else { "en" };
    rust_i18n::set_locale(new);
    self.locale = new.into();
    Task::none()
}
```

#### 7. Replace all hardcoded strings with `t!()` macro

Before:
```rust
text("Settings").size(14)
```

After:
```rust
text(t!("settings")).size(14).font(BOLD)
```

With interpolation:
```rust
text(t!("log_new_file", name = file_name))
```

#### 8. Log messages must also use `t!()`

The `watcher.rs` thread sends raw strings. It must also call `t!()`.
Since `t!` is a global macro, it works from any thread.
Pass the locale as a parameter to `watcher::start` or use `rust_i18n::locale()`.

```rust
// In watcher thread:
log(&t!("log_new_file", name = file_name));
```

#### 9. Persist locale choice

Store `locale` in `config.toml` so the choice survives restart:

```toml
[locale]
language = "pt-BR"
```

Load on startup via `rust_i18n::set_locale()` before UI renders.

### Files to create/modify

| File | Action |
|------|--------|
| `src/i18n/en.toml` | Create — all English strings |
| `src/i18n/pt-BR.toml` | Create — all pt-BR translations |
| `src/i18n.rs` | Create — module + re-exports |
| `src/main.rs` | Modify — add `locale` field, `ToggleLocale` msg, replace all strings |
| `src/watcher.rs` | Modify — replace raw strings with `t!()` calls |
| `src/config.rs` | Modify — add `Locale { language }` struct, load/save |
| `Cargo.toml` | Modify — add `rust-i18n` dependency |

---

## v0.6.0 — Windows support

### Goal

App compiles and runs on Windows 7, 10, and 11.
CI produces a `.exe` alongside the Linux binary.
Both are included in the release.
Documentation covers both platforms.

### Research

#### Iced + Windows

Iced 0.14 uses `wgpu` for rendering. `wgpu` supports Windows via:
- **DX12** (native, Windows 10+)
- **Vulkan** (via VK_KHR_portability or native)
- **OpenGL** (via ANGLE or native)

For Windows 7 support: `wgpu` does NOT support Windows 7.
`v0.20+` dropped Windows 7 support entirely.
Iced 0.14's `wgpu` backend requires Windows 10 minimum for DX12.
However, OpenGL fallback may work on Windows 7 with appropriate drivers.

**Decision**: Target Windows 10 and 11 as primary.
Windows 7 is best-effort (OpenGL backend only, may not work).

#### Cross-compilation from Linux

Two paths:

| Approach | Pros | Cons |
|----------|------|------|
| `x86_64-pc-windows-gnu` | No MSVC license, native cross-compile from Linux | Iced white canvas bug (issue #987), requires extra DLLs |
| `x86_64-pc-windows-msvc` via `cross` | Better Iced compatibility | Requires Docker, larger setup |
| GitHub Actions `windows-latest` | Native build, simplest | CI-only, can't test locally |

**Decision**: Use GitHub Actions `windows-latest` runner for CI builds.
Document local Windows build for contributors.

#### Cross-compilation setup (for reference)

```bash
# Install target
rustup target add x86_64-pc-windows-gnu

# Install mingw-w64 linker
sudo apt install gcc-mingw-w64-x86-64

# Build
cargo build --target x86_64-pc-windows-gnu --release
```

Required DLLs for redistribution (GNU builds):
- `libstdc++-6.dll`
- `libgcc_s_seh-1.dll`
- `libwinpthread-1.dll`

These must be bundled with the `.exe` in the release zip.

#### Sound playback

Linux: `paplay` / `aplay` (already implemented).

Windows options:

| Approach | Complexity | Notes |
|----------|-----------|-------|
| `winapi` + `PlaySound` | Medium | `winapi::um::playsoundapi::PlaySoundW` |
| `rodio` | Low | Already in dependencies, cross-platform |
| Shell `start` | Low | Opens default player, not ideal |

**Decision**: Use `rodio` for cross-platform sound. It's already in `Cargo.toml`.
Replace the current `paplay`/`aplay` calls with `rodio::source::Source::new`.
If `rodio` causes issues on Windows, fall back to `winapi::PlaySoundW` via `cfg!(windows)`.

```rust
// src/watcher.rs — cross-platform sound
#[cfg(unix)]
fn play_sound(path: &Path) {
    // paplay / aplay fallback (existing code)
}

#[cfg(windows)]
fn play_sound(path: &Path) {
    use rodio::{Decoder, OutputStream, Sink};
    // or: winapi::PlaySoundW
}
```

#### File watcher

`notify` crate works on both platforms (uses `ReadDirectoryChangesW` on Windows).
No code changes needed.

#### Path handling

`std::path::Path` and `PathBuf` handle both `/` and `\` correctly on their respective platforms.
No code changes needed.

### Implementation

#### 1. CI workflow for Windows

```yaml
# .github/workflows/ci.yml — add Windows job
test-windows:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v5
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo fmt --check
    - run: cargo clippy -- -D warnings
    - run: cargo test
```

#### 2. Release workflow for both platforms

```yaml
release-linux:
  runs-on: ubuntu-latest
  # ... existing release job

release-windows:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v5
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo build --release
    - run: cargo run --release --package scripts -- vX.Y.Z  # zip + upload
```

#### 3. Sound abstraction

Create `src/sound.rs`:

```rust
use std::path::Path;

pub fn play(path: &Path) {
    #[cfg(unix)]
    play_unix(path);
    #[cfg(windows)]
    play_windows(path);
}

#[cfg(unix)]
fn play_unix(path: &Path) { /* existing paplay/aplay code */ }

#[cfg(windows)]
fn play_windows(path: &Path) { /* rodio or winapi */ }
```

#### 4. Release script

Update `scripts/release.sh` to produce platform-specific zip names:
- `dicom-watch-vX.Y.Z-linux-x86_64.zip`
- `dicom-watch-vX.Y.Z-windows-x86_64.zip`

#### 5. Documentation

Update all docs:

- `README.md` — separate install for Linux and Windows
- `AGENTS.md` — build instructions for both targets
- `regex-guide.md` — no change needed (regex is cross-platform)

#### 6. Windows 7 compatibility (best-effort)

- Use `iced::Settings::backend` to force OpenGL if available
- Document that Windows 7 is not officially supported
- If `wgpu` drops OpenGL entirely, document the limitation

### Files to create/modify

| File | Action |
|------|--------|
| `.github/workflows/ci.yml` | Modify — add Windows test + release jobs |
| `src/sound.rs` | Create — platform-agnostic sound module |
| `src/watcher.rs` | Modify — use `crate::sound::play` instead of inline |
| `scripts/release.sh` | Modify — platform-aware zip naming |
| `Cargo.toml` | Modify — add `rodio` feature if needed |
| `README.md` | Modify — dual-platform install |
| `AGENTS.md` | Modify — dual-platform build instructions |

---

## v0.7.0 — Application icon

### Goal

DicomWatch has a professional, high-resolution icon embedded in the binary.
The icon is visible in:
- Linux: title bar, taskbar, alt-tab, app menu (Cinnamon, GNOME, KDE)
- Windows: title bar, taskbar, Start menu, Alt+Tab
- `.desktop` file for Linux launchers

Icon sourced from the Obsidian icon set (already on this machine).

### Research

#### Iced window icon

Iced 0.14 provides two APIs:

**At window creation** (in `Settings`):
```rust
iced::window::Settings {
    icon: Some(iced::window::icon::from_file_data(
        include_bytes!("../assets/icon.png"),
        None,
    )?),
    ..Default::default()
}
```

**At runtime** (returns `Task`):
```rust
Command::batch([
    iced::window::set_icon(iced::window::Id::MAIN, icon),
])
```

Requires `image` feature enabled in Cargo.toml (already enabled).

#### Icon format

| Format | Linux | Windows | Notes |
|--------|-------|---------|-------|
| PNG | ✅ | ❌ | Must be installed to hicolor theme |
| ICO | ❌ | ✅ | Native Windows icon format |
| Both | ✅ | ✅ | Best approach — embed PNG for Linux, ICO for Windows |

#### Windows icon embedding

`winres` crate:
```toml
[build-dependencies]
winres = "0.1"
```

`build.rs`:
```rust
fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().unwrap();
    }
}
```

`.ico` file should contain multiple resolutions: 16, 32, 48, 256.

#### Linux icon installation

`.desktop` file:
```ini
[Desktop Entry]
Name=DicomWatch
Exec=/usr/bin/dicom-watch
Icon=dicom-watch
Type=Application
Categories=Utility;Medical;
```

Icon must be installed to freedesktop icon theme:
```
/usr/share/icons/hicolor/16x16/apps/dicom-watch.png
/usr/share/icons/hicolor/32x32/apps/dicom-watch.png
/usr/share/icons/hicolor/48x48/apps/dicom-watch.png
/usr/share/icons/hicolor/256x256/apps/dicom-watch.png
```

Or user-local:
```
~/.local/share/icons/hicolor/...
```

#### Icon source — Obsidian icon set

The Obsidian icon set uses MIT license.
Icons are available as SVG and PNG.
Extract a high-resolution PNG (256×256 minimum) for the app icon.
Convert to ICO for Windows using `imagemagick`:
```bash
convert icon-256x256.png -define icon-resize=16,32,48,256 icon.ico
```

### Implementation

#### 1. Asset structure

```
assets/
├── icon.png              # 256×256 source, used for Linux + Iced runtime
├── icon.ico              # Multi-resolution, used for Windows .exe
└── hicolor/              # Linux freedesktop icon theme
    ├── 16x16/apps/dicom-watch.png
    ├── 32x32/apps/dicom-watch.png
    ├── 48x48/apps/dicom-watch.png
    └── 256x256/apps/dicom-watch.png
```

#### 2. Extract and convert icon

```bash
# Source: Obsidian icon set on this machine
# Pick appropriate icon, export/convert to 256×256 PNG
cp <obsidian-icon>.png assets/icon.png

# Generate ICO for Windows
convert assets/icon.png -define icon-resize=16,32,48,256 assets/icon.ico

# Generate hicolor PNGs
for size in 16 32 48 256; do
    convert assets/icon.png -resize ${size}x${size} \
        assets/hicolor/${size}x${size}/apps/dicom-watch.png
done
```

#### 3. Linux `.desktop` file

Create `assets/dicom-watch.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=DicomWatch
Comment=Watch directory for DICOM zip files
Exec=dicom-watch
Icon=dicom-watch
Categories=Utility;Medical;GTK;
Terminal=false
StartupNotify=true
```

#### 4. Build script for Windows icon

Create `build.rs`:
```rust
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("Failed to compile Windows resources");
    }
}
```

Add to `Cargo.toml`:
```toml
[build-dependencies]
winres = "0.1"
```

#### 5. Iced window icon (Linux + runtime)

In `main.rs`, load icon at startup and set it:

```rust
fn main() -> iced::Result {
    // Load icon from embedded bytes
    let icon = iced::window::icon::from_file_data(
        include_bytes!("../assets/icon.png"),
        None,
    )
    .ok();

    iced::application(...)
        .window_settings(iced::window::Settings {
            icon,
            ..Default::default()
        })
        .run()
}
```

Or set it via Task after window creation if compile-time fails.

#### 6. Install script for Linux

Create `scripts/install-linux.sh`:
```bash
#!/usr/bin/env bash
# Installs binary, .desktop file, and icons

PREFIX="${1:-/usr/local}"
BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/dicom-watch"
ICON_BASE="$PREFIX/share/icons/hicolor"

install -Dm755 target/release/dicom-watch "$BIN_DIR/dicom-watch"
install -Dm644 assets/dicom-watch.desktop "$PREFIX/share/applications/dicom-watch.desktop"
install -Dm644 assets/icon.png "$APP_DIR/icon.png"

for size in 16 32 48 256; do
    install -Dm644 \
        "assets/hicolor/${size}x${size}/apps/dicom-watch.png" \
        "${ICON_BASE}/${size}x${size}/apps/dicom-watch.png"
done

update-icon-caches "$ICON_BASE"
```

#### 7. Uninstall script

`scripts/uninstall-linux.sh` — removes all installed files.

### Files to create/modify

| File | Action |
|------|--------|
| `assets/icon.png` | Create — 256×256 source icon from Obsidian set |
| `assets/icon.ico` | Create — multi-resolution Windows icon |
| `assets/hicolor/` | Create — PNGs at 16/32/48/256 for Linux |
| `assets/dicom-watch.desktop` | Create — freedesktop launcher entry |
| `build.rs` | Create — compile Windows `.ico` into `.exe` |
| `scripts/install-linux.sh` | Create — install binary + icons + desktop file |
| `scripts/uninstall-linux.sh` | Create — remove installed files |
| `Cargo.toml` | Modify — add `build = "build.rs"`, add `winres` dev-dep |
| `src/main.rs` | Modify — load icon, pass to window settings |
| `README.md` | Modify — document icon installation |
| `AGENTS.md` | Modify — document icon build steps |

---

## Summary

| Version | Theme | Key Dependencies | Key Deliverables |
|---------|-------|-----------------|-----------------|
| v0.5.0 | i18n | `rust-i18n` | Toggle EN↔pt-BR, `src/i18n/*.toml`, all strings externalized |
| v0.6.0 | Windows | `rodio`, `winapi` (optional) | Cross-compile `.exe`, CI dual-platform, sound on both |
| v0.7.0 | Icon | `winres`, `imagemagick` | `.ico` + `.png` icon set, `.desktop` file, install script |
