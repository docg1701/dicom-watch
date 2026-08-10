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

#### 3. Initialize i18n at crate root

In `main.rs`, at the top:

```rust
#[macro_use]
extern crate rust_i18n;

i18n!("src/i18n");
```

NOTE: The `i18n!` macro MUST be called at the crate root (`main.rs`), NOT in a separate module.
It generates the `t!` macro and translation lookup code at compile time.

Define locale constants in `src/i18n.rs`:

```rust
// src/i18n.rs
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
| `src/i18n.rs` | Create — locale constants (`LOCALES`, `DEFAULT_LOCALE`) only |
| `src/main.rs` | Modify — `#[macro_use] extern crate rust_i18n;`, `i18n!()`, add `locale` field, `ToggleLocale` msg, replace all strings |
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

Linux: `paplay` / `aplay` (already implemented in `src/watcher.rs`).

Windows options:

| Approach | Complexity | Notes |
|----------|-----------|-------|
| `winapi` + `PlaySoundW` | Low | Native Win32, no extra deps beyond `winapi` |
| Shell `cmd /c start` | Low | Opens default player, not ideal |

**Decision**: Keep `paplay`/`aplay` on Linux. Use `winapi::PlaySoundW` on Windows.
No new heavy dependencies — `winapi` is already pulled in by `notify` on Windows.

```toml
# Cargo.toml — add Windows-only dependency
[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = ["playsoundapi", "winuser"] }
```

```rust
// src/watcher.rs — cross-platform sound
#[cfg(unix)]
fn play_sound(path: &Path) {
    // existing paplay / aplay code
}

#[cfg(windows)]
fn play_sound(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::playsoundapi::PlaySoundW;
    use winapi::um::winuser::SND_FILENAME;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME) };
}
```

#### File watcher

`notify` crate works on both platforms (uses `ReadDirectoryChangesW` on Windows).
No code changes needed.

#### Path handling

`std::path::Path` and `PathBuf` handle both `/` and `\` correctly on their respective platforms.
No code changes needed.

### Implementation

#### 1. CI workflow — quality checks only

CI does NOT compile any binary. It only runs quality checks (fmt, clippy, tests).
Add a Windows job to verify the code at least compiles on Windows:

```yaml
# .github/workflows/ci.yml — add Windows quality check
test-windows:
  runs-on: windows-latest
  steps:
    - uses: actions/checkout@v5
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo fmt --check
    - run: cargo clippy -- -D warnings
    - run: cargo test
```

No release job on CI. All compilation is local, same as v0.4.0.
The user downloads the release zip, extracts, and runs.

#### 2. Local Windows build (documented for contributors)

```bash
# On Windows or cross-compiling from Linux:
cargo build --release
# Binary: target/release/dicom-watch.exe
```

Release zip for Windows is created locally and uploaded to the existing GitHub Release
(created by CI on tag push, same as Linux).

#### 3. Sound abstraction

Keep the existing `play_sound` function in `src/watcher.rs` but make it `pub`
and platform-aware via `cfg` attributes (as described in the Sound playback section above).

No new `src/sound.rs` module needed — the function stays in `watcher.rs`
but becomes `pub fn play_sound(path: &Path)` so it can be called from tests if needed.

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
| `.github/workflows/ci.yml` | Modify — add Windows quality-check job (fmt/clippy/test only, no build) |
| `src/watcher.rs` | Modify — `play_sound` becomes `pub` + `cfg` for Windows `PlaySoundW` |
| `scripts/release.sh` | Modify — platform-aware zip naming |
| `Cargo.toml` | Modify — add `[target.'cfg(windows)'.dependencies] winapi` |
| `README.md` | Modify — dual-platform instructions |
| `AGENTS.md` | Modify — dual-platform build instructions |

---

## v0.7.0 — Application icon

### Goal

DicomWatch has a professional, high-resolution icon embedded in the binary.
The icon is visible in:
- Linux: title bar, taskbar, alt-tab, app menu (Cinnamon, GNOME, KDE)
- Windows: title bar, taskbar, Start menu, Alt+Tab

Icon sourced from the Obsidian icon set (already on this machine).
The icon is embedded at compile time — no external files needed at runtime.
No installation: user downloads the release zip, extracts, and runs.

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

#### 3. Build script for Windows icon

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

The icon is embedded at compile time — no external files needed at runtime.
User downloads the release zip, extracts, and runs. No installation.

### Files to create/modify

| File | Action |
|------|--------|
| `assets/icon.png` | Create — 256×256 source icon from Obsidian set (repo asset only) |
| `assets/icon.ico` | Create — multi-resolution Windows icon (repo asset only) |
| `build.rs` | Create — compile Windows `.ico` into `.exe` via `winres` |
| `Cargo.toml` | Modify — add `build = "build.rs"`, add `winres` dev-dep |
| `src/main.rs` | Modify — embed icon via `include_bytes!`, pass to `window_settings` |
| `README.md` | Modify — document icon source (Obsidian, MIT) |
| `AGENTS.md` | Modify — document icon build steps |

---

## Summary

| Version | Theme | Key Dependencies | Key Deliverables |
|---------|-------|-----------------|-----------------|
| v0.5.0 | i18n | `rust-i18n` | Toggle EN↔pt-BR, `src/i18n/*.toml`, all strings externalized |
| v0.6.0 | Windows | `winapi` (Windows-only) | Local Windows build, CI quality-check only, `PlaySoundW` on Windows |
| v0.7.0 | Icon | `winres`, `imagemagick` | `.ico` embedded in `.exe`, `.png` embedded in Linux binary, no install needed |
