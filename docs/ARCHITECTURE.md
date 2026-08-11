# Architecture — dicom-watch

> Blueprint for building lightweight, cross-platform desktop apps with Rust.
> Use this as a template for any file-watching, background-processing, or
> system-tray-less desktop utility.

---

## 1. Philosophy

- **No framework, just libraries.** The app is held together by ~20 lines of
  glue. There is no ORM, no dependency injection, no router, no service layer.
  Each module does one thing and exposes one public function.
- **State machine GUI.** The GUI is a pure function of state (`view`), a pure
  state reducer (`update`), and a side-effect manager (`subscription`). This is
  the Elm Architecture, implemented by `iced`.
- **Channels, never shared mutable state.** The watcher thread talks to the GUI
  through a single `mpsc::unbounded` channel. No `Arc<Mutex<T>>` anywhere.
- **Crash early, crash loud.** Config errors abort the process before the GUI
  opens, with human-readable messages in both `stderr` and a native OS dialog.
  No silent defaults, no fallback values.
- **Portable by construction.** The same source compiles for Linux and Windows
  with zero `#[cfg]` branching in business logic. Platform differences are
  isolated to sound playback (two 5-line functions) and the build script.

---

## 2. Project structure

```
src/
  main.rs      — State machine: AppState, Message, update(), view(), subscription()
  watcher.rs   — Background thread: notify watcher, zip extraction, sound playback
  config.rs    — Config load/validate/save, FilterMode enum, path resolution
locales/
  en.toml      — English translations (all UI strings)
  pt-BR.toml   — Brazilian Portuguese translations
scripts/
  release.sh          — Build release binary, package Linux zip, upload to GitHub
  release-windows.sh  — Cross-compile Windows .exe, package zip, upload
  install.sh          — Linux: create .desktop entry + copy icon
  uninstall.sh        — Linux: remove .desktop entry + icon
build.rs              — Windows: embed .ico into .exe via winres (no-op on Linux)
assets/
  icon.png            — 256×256 PNG (embedded at compile time via include_bytes!)
  icon.ico            — Multi-res Windows icon (16/32/48/256)
  alarm.wav       — Default notification sound
config.toml.example   — Documented template; user copies to config.toml
```

### Why 3 source files?

| File | Lines | Responsibility |
|------|-------|---------------|
| `main.rs` | ~360 | GUI state, layout, message routing, startup |
| `watcher.rs` | ~200 | Thread spawn, fs events, zip extraction, sound |
| `config.rs` | ~120 | TOML parsing, validation, path resolution |

Each file is self-contained and under 500 lines. A developer reading
`watcher.rs` never needs to open `main.rs` to understand what the watcher does.
The only coupling is: `watcher::start()` takes an `UnboundedSender<String>` and
sends log lines — it has no idea those lines end up in an Iced widget.

---

## 3. State machine (Elm Architecture via Iced)

### 3.1 The loop

```
User action → Message → update(&mut AppState, Message) → Task<Message>
              ↑                                              │
              └──── view(&AppState) → Element<Message>       │
                                │                            │
                                └── subscription(&AppState) ──┘
                                   (background streams)
```

Every user interaction (click, text input, file dialog result) produces a
`Message` enum variant. `update()` pattern-matches it, mutates `AppState`, and
optionally returns a `Task` for async work (file dialogs). `view()` takes a
read-only `&AppState` and produces a widget tree — it never mutates state.

### 3.2 AppState — flat struct, no nesting

```rust
struct AppState {
    exe_dir: PathBuf,       // directory containing the binary
    config_path: PathBuf,   // path to config.toml (next to binary)
    source_dir: String,     // user-editable
    dest_dir: String,
    filter_mode: FilterMode,
    filter_pattern: String,
    sound_enabled: bool,
    sound_file: String,
    locale: String,         // "en" or "pt-BR"
    watching: bool,         // whether the watcher thread is active
    log: Vec<String>,       // ring buffer, max ~200 lines
    field_errors: Vec<String>, // validation errors shown in red
}
```

All fields are owned `String`s — no lifetimes, no borrowed references from
config. This keeps the struct `'static` and avoids fighting the borrow checker
during state transitions.

### 3.3 Message — flat enum, one variant per action

```rust
enum Message {
    WatchToggled,
    DeleteAll,
    SourceDirChanged(String),
    DestDirChanged(String),
    FilterModeChanged(FilterMode),
    FilterPatternChanged(String),
    SoundEnabledChanged(bool),
    SoundFileChanged(String),
    LogLine(String),                    // from watcher thread
    BrowseSourceDir,                    // triggers Task::perform
    BrowseDestDir,
    BrowseSoundFile,
    SourceDirPicked(Option<PathBuf>),   // result of Task::perform
    DestDirPicked(Option<PathBuf>),
    SoundFilePicked(Option<PathBuf>),
    ToggleLocale,
}
```

Three categories:
- **User actions** → mutate state, re-validate, save config.
- **Async results** (`*Picked`) → receive `PathBuf` from `rfd::AsyncFileDialog`.
- **Thread messages** (`LogLine`) → push to log ring buffer.

### 3.4 Subscription — thread ↔ GUI bridge

```rust
fn subscription(&self) -> Subscription<Message> {
    if !self.watching { return Subscription::none(); }
    iced::Subscription::run_with(config, build_watcher_stream)
}
```

`subscription()` is called by Iced whenever state changes. When `watching` is
`true`, it spawns the watcher thread. When `watching` becomes `false`, Iced
drops the old subscription, which drops the stream, which drops the
`StopGuard`, which sets `AtomicBool` to `false` — the watcher loop exits.

**This is the key pattern for thread lifecycle management.** No `thread::JoinHandle`,
no `abort()`, no signal handling. The thread simply polls `AtomicBool` and the
guard sets it on drop.

---

## 4. Thread communication

### 4.1 The channel

```rust
// In build_watcher_stream():
let (log_tx, log_rx) = iced::futures::channel::mpsc::unbounded::<String>();

// Start the background thread
watcher::start(src, dst, mode, pat, sound, file, log_tx, running_clone);

// Map the rx stream into Iced Messages
log_rx
    .map(move |s| { let _hold = &guard; Message::LogLine(s) })
    .boxed()
```

`watcher::start()` spawns `std::thread` and moves `log_tx` into it. The watcher
never knows about Iced, Messages, or the GUI. It just calls
`log_tx.unbounded_send(line)`.

### 4.2 Stop signal

```rust
struct StopGuard(Arc<AtomicBool>);
impl Drop for StopGuard { ... }  // sets to false
```

The `StopGuard` is moved into the stream's closure. When Iced drops the
subscription, the stream is dropped, the guard's `Drop` fires, and the
`AtomicBool` flips to `false`. The watcher's `while running.load(...)` loop
exits on next iteration.

### 4.3 Why not async?

The watcher uses `std::thread` + `std::sync::mpsc::channel` for filesystem
events (`notify` crate is sync). Sound playback spawns a short-lived thread.
This keeps the call stack simple: no `tokio`, no `async fn main`, no runtime
choice. For a single-background-thread app, async adds complexity with zero
benefit.

---

## 5. Cross-platform strategy

### 5.1 Platform isolation

Only two places in the entire codebase have `#[cfg]`:

```rust
// watcher.rs — sound playback
#[cfg(unix)]
fn play_sound(path: &Path) { /* try paplay, fallback to aplay */ }

#[cfg(windows)]
fn play_sound(path: &Path) { /* PlaySoundW via winapi */ }
```

```rust
// watcher.rs — Unix file permissions on extracted files
#[cfg(unix)]
{ use std::os::unix::fs::PermissionsExt; ... }
```

```rust
// build.rs — icon embedding
if cfg!(windows) { winres::WindowsResource::new().set_icon(...).compile(); }
```

Everything else — GUI layout, config parsing, file watching, zip extraction,
i18n — is identical on both platforms. The `iced` and `rfd` crates handle
platform-native rendering and dialogs internally.

### 5.2 Sound playback (Linux)

Tries `paplay` (PulseAudio) first, falls back to `aplay` (ALSA). Both are
ubiquitous on Linux desktops. The thread is detached — if playback fails,
nothing crashes, nothing blocks.

### 5.3 Release packaging

| Platform | Zip contents |
|----------|-------------|
| Linux | `dicom-watch` + `install.sh` + `icon.png` + `config.toml.example` + `alarm.wav` |
| Windows | `dicom-watch.exe` + `config.toml.example` + `alarm.wav` |

The Linux binary is dynamically linked to system libs (glibc, X11, Wayland).
The Windows binary is statically linked via `x86_64-pc-windows-gnu` — it's
a single `.exe` with no DLL dependencies.

---

## 6. Internationalization (i18n)

### 6.1 rust-i18n — compile-time embedded

```rust
#[macro_use] extern crate rust_i18n;
i18n!("locales");
```

Translation files live in `locales/{en,pt-BR}.toml`. At compile time, the
macro embeds them into the binary as static strings. No runtime file loading,
no missing translation files at runtime.

### 6.2 Usage

```rust
t!("button.start")          // returns &str at current locale
t!("error.source_missing", path = "/foo")  // interpolation
```

A convenience wrapper `fn tr(key: &str) -> String` converts `&str` to owned
`String` for widget text.

### 6.3 Switching at runtime

```rust
Message::ToggleLocale => {
    let new = if self.locale == "en" { "pt-BR" } else { "en" };
    rust_i18n::set_locale(new);
    self.locale = new.into();
}
```

`rust_i18n::set_locale()` switches the global locale. The next `view()` call
re-renders all text. The choice is persisted to `config.toml`.

### 6.4 Locale file structure

```toml
[status]       # status.watching, status.idle
[button]       # button.start, button.stop, button.clear_all, button.browse
[field]        # field.source_dir, field.dest_dir, ...
[placeholder]  # placeholder values for text inputs
[section]      # section.settings, section.activity_log
[log]          # log.waiting, log.watching_start
[error]        # error.source_missing, error.dest_missing, ...
```

Flat namespacing by section. No nesting — `t!("section.field")` is as deep as
it gets.

---

## 7. Configuration system

### 7.1 TOML format — user-editable

```toml
[directories]
source = "/home/user/Downloads"
destination = "/home/user/dicom/studies"

[filter]
mode = "glob"
pattern = "*.zip"

[sound]
enabled = true
file = "alarm.wav"

[locale]
language = "en"
```

### 7.2 Load → deserialize → validate → fail or ready

```rust
pub fn load_config(exe_dir: &Path) -> Result<Config, String> {
    let config_path = exe_dir.join("config.toml");
    // 1. Check file exists → human-readable error
    // 2. Read to string → error with path
    // 3. toml::from_str → error with syntax location
    // 4. validate_config → check dirs exist, regex compiles, sound file found
    Ok(config)
}
```

Validation is exhaustive: every assumption is checked before the GUI opens. The
user never sees a blank window followed by a crash — they get a native OS
dialog explaining exactly what's wrong.

### 7.3 Config changes from GUI — save immediately

Every field change in the GUI calls `save_config(self)`, which serializes the
current `AppState` back to `config.toml` via `toml::to_string_pretty`. The file
is always in sync with the UI. No separate "Save" button.

### 7.4 Path resolution

```rust
pub fn resolve_path(path_str: &str, exe_dir: &Path) -> PathBuf {
    if Path::new(path_str).is_absolute() {
        path_str.into()
    } else {
        exe_dir.join(path_str)
    }
}
```

Relative paths are resolved against the directory containing the `.exe`. This
allows the config to reference bundled files (like `alarm.wav`) without
hardcoding install paths.

---

## 8. Release pipeline

### 8.1 CI (GitHub Actions)

| Trigger | Action |
|---------|--------|
| Push to `master` | `cargo fmt --check` |
| Push tag `v*.*.*` | `cargo fmt --check`, then auto-create GitHub Release with changelog |

CI never compiles, never runs clippy, never runs tests. All of that is local.

### 8.2 Local release script

```bash
# Linux
cargo build --release
./scripts/release.sh v0.8.3
  # → zip containing binary + install.sh + icon + config.example + sound
  # → uploaded to GitHub Release via gh CLI

# Windows (cross-compile from Linux)
cargo build --release --target x86_64-pc-windows-gnu
./scripts/release-windows.sh v0.8.3
  # → zip containing .exe + config.example + sound
  # → uploaded to GitHub Release via gh CLI
```

The release scripts use `zip -j` to flatten the archive (no directory
structure inside) and `gh release upload --clobber` to attach to the
CI-created release.

### 8.3 Version workflow

```
bump Cargo.toml → cargo build (sync Cargo.lock) → commit → PR → merge
→ git tag vX.Y.Z <sha> → git push origin vX.Y.Z
→ CI creates release → build locally → upload both zips
```

Tag is **lightweight** (`git tag v0.8.3 <sha>`, never `-a`/`-m`). Annotated
tags produce duplicated release titles in the CI integration.

---

## 9. Error handling patterns

### 9.1 Startup errors — crash with dialog

```rust
let config = match config::load_config(&exe_dir) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("DicomWatch: {}", e);
        rfd::MessageDialog::new()
            .set_title("DicomWatch")
            .set_description(&e)
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
};
```

Same message goes to `stderr` and a native OS dialog. The dialog blocks until
OK is clicked, then the process exits. No GUI is created.

### 9.2 Runtime field errors — red text, block Start

```rust
fn validate_fields(state: &AppState) -> Vec<String> {
    // Returns list of human-readable errors
    // Errors are shown as red text in the GUI
    // If non-empty, WatchToggled refuses to start
}
```

Fields re-validate on every keystroke. The user sees errors immediately, not on
submit.

### 9.3 Watcher errors — log and continue

```rust
// watcher.rs
match extract_zip(&path, &dest_dir) {
    Ok(count) => { log("Extracted..."); }
    Err(e) => { log(&format!("Failed to extract: {}", e)); }
}
```

The watcher never crashes. Extraction errors are logged and visible in the
activity panel. The watcher continues watching for new files.

### 9.4 Sound errors — silent fallback

```rust
if result.is_err() {
    let _ = std::process::Command::new("aplay")...;  // fallback
}
```

Sound playback failure is silent. A missing or broken audio system shouldn't
block the app from doing its primary job.

---

## 10. Dependency rationale

| Crate | Why |
|-------|-----|
| `iced` 0.14 | Native GUI, Elm architecture, cross-platform, GPU-accelerated |
| `notify` 8 | Cross-platform filesystem events (inotify / ReadDirectoryChangesW) |
| `toml` + `serde` | Config parsing with derive macros |
| `zip` 2 | Zip extraction (no external `unzip` binary) |
| `regex` | Pattern matching for `filter.mode = "regex"` |
| `glob` | Pattern matching for `filter.mode = "glob"` |
| `chrono` | Timestamps in log lines |
| `rfd` 0.17 | Native file/folder picker dialogs + message dialogs |
| `rust-i18n` 4 | Compile-time i18n, `set_locale` at runtime |
| `winapi` (Win only) | `PlaySoundW` for audio, `MessageBoxW` for error dialog |
| `winres` (build only) | Embed `.ico` in Windows `.exe` |

No async runtime, no HTTP client, no database. The app does exactly one thing
and the dependency list reflects that.

---

## 11. Replication recipe

To build a new app following this same architecture:

### 11.1 Skeleton

```
src/
  main.rs    — AppState, Message, update(), view(), subscription()
  worker.rs  — thread::spawn + channel send
  config.rs  — load/validate/save TOML
locales/
  en.toml
build.rs    — Windows icon embedding
assets/
  icon.png
  icon.ico
config.toml.example
```

### 11.2 Rules

1. **AppState is flat and owned.** No lifetimes, no borrowed config fields.
2. **Message is a flat enum.** One variant per action. Async results get `*Picked` variants.
3. **Thread → GUI via `mpsc::unbounded`.** The thread knows nothing about the GUI.
4. **Thread lifecycle via `AtomicBool` + `StopGuard`.** No `JoinHandle::abort()`.
5. **Config is validated before the GUI opens.** Use `rfd::MessageDialog` for the error.
6. **Platform differences are two `#[cfg]` blocks max.**
7. **No async runtime unless you have 10+ concurrent IO streams.**
8. **All strings in locale files.** Never hardcode user-visible text.
9. **Save config on every field change.** No separate save button.
10. **Release CI does format-check only.** Build + package is local.
11. **Functions ≤ 30 lines.** Files ≤ 500 lines.

### 11.3 Cargo.toml template

```toml
[package]
name = "your-app"
version = "0.1.0"
edition = "2024"
build = "build.rs"

[dependencies]
iced = { version = "0.14", default-features = false,
         features = ["wgpu", "image", "svg", "x11", "wayland", "thread-pool"] }
notify = "8"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
rfd = "0.17"
rust-i18n = "4"
chrono = "0.4"

[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = [...] }

[build-dependencies]
winres = "0.1"

[profile.release]
opt-level = "s"
lto = true
codegen-units = 1
strip = true
```

The release profile is tuned for binary size (`opt-level = "s"`) and single
binary distribution (`lto`, `strip`).

---

## 12. What this architecture avoids

- **No `Arc<Mutex<T>>` in production code.** The `AtomicBool` stop flag is the
  only shared mutable state.
- **No async runtime.** `std::thread` for background work, `iced::Task` only
  for file dialogs.
- **No event bus, no pub/sub.** One channel, one direction.
- **No widget library wrapping.** Styles are defined as two ~10-line functions.
- **No CI build matrix.** Local-only compilation means no CI minutes consumed.
- **No installer.** Linux: `.desktop` file script. Windows: portable `.exe`.
- **No auto-updater.** The user copies a zip. KISS.
