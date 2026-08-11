# Architecture — dicom-watch

> Blueprint for building lightweight, cross-platform desktop apps with Rust.
> Use this as a template for any file-watching, background-processing, or
> desktop utility.

---

## 1. Philosophy

- **No framework, just libraries.** The app is held together by ~20 lines of
  glue. There is no ORM, no dependency injection, no router, no service layer.
  Each module does one thing and exposes one public function.
- **State machine GUI.** The GUI is a pure function of state (`view`), a pure
  state reducer (`update`), and a side-effect manager (`subscription`). This is
  the Elm Architecture, implemented by `iced`.
- **Channels, never shared mutable state.** Background threads talk to the GUI
  through `mpsc::unbounded` channels. No `Arc<Mutex<T>>` anywhere.
- **Crash early, crash loud.** Config errors abort the process before the GUI
  opens, with human-readable messages in both `stderr` and a native OS dialog.
  No silent defaults for filesystem paths.
- **Portable by construction.** The same source compiles for Linux and Windows
  with minimal `#[cfg]` branching. Platform differences are isolated to sound
  playback and the build script.

---

## 2. Project structure

```
src/
  main.rs      — State machine: AppState, Message, update(), view(), subscription()
  watcher.rs   — Background thread: notify watcher, zip extraction, sound playback
  config.rs    — Config load/validate/save, FilterMode enum, path resolution
  tray.rs      — System tray: platform bootstrap (GTK thread on Linux), icon + menu creation, event polling (bg thread)
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
  alarm.wav           — Notification sound (WAV, cross-platform)
  delete.wav          — Delete-all sound (WAV, cross-platform)
config.toml.example   — Documented template; user copies to config.toml
docs/
  architecture.md     — This file
  regex-guide.md      — User-facing regex documentation
```

### Module responsibilities

| File | Responsibility |
|------|---------------|
| `main.rs` | GUI state, layout, message routing, startup, subscription orchestration |
| `watcher.rs` | Background thread: fs events, zip extraction, sound playback |
| `config.rs` | TOML parsing, validation, path resolution, struct definitions |
| `tray.rs` | System tray: platform bootstrap, icon + menu creation, event polling (background thread) |

Each file is self-contained. `watcher.rs` has no idea about Iced widgets.
`tray.rs` has no idea about the watcher. The only coupling is through
`UnboundedSender` channels carrying plain data.

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

### 3.2 AppState — flat struct, no nesting

```rust
struct AppState {
    exe_dir: PathBuf,
    config_path: PathBuf,
    source_dir: String,
    dest_dir: String,
    filter_mode: FilterMode,
    filter_pattern: String,
    sound_enabled: bool,
    sound_file: String,
    delete_sound_enabled: bool,
    delete_sound_file: String,
    locale: String,           // "en" or "pt-BR"
    watching: bool,
    tray_enabled: bool,       // minimize-to-tray toggle
    log: Vec<String>,         // ring buffer, max ~200 lines
    field_errors: Vec<String>,
}
```

All fields are owned `String`s — no lifetimes, no borrowed references.

### 3.3 Message — flat enum

```rust
enum Message {
    WatchToggled, DeleteAll,
    SourceDirChanged(String), DestDirChanged(String),
    FilterModeChanged(FilterMode), FilterPatternChanged(String),
    SoundEnabledChanged(bool), SoundFileChanged(String),
    DeleteSoundEnabledChanged(bool), DeleteSoundFileChanged(String),
    LogLine(String),
    BrowseSourceDir, BrowseDestDir, BrowseSoundFile, BrowseDeleteSoundFile,
    SourceDirPicked(Option<PathBuf>), DestDirPicked(Option<PathBuf>),
    SoundFilePicked(Option<PathBuf>), DeleteSoundFilePicked(Option<PathBuf>),
    ToggleLocale,
    TrayEnabledChanged(bool),
    TrayEvent(TrayAction),
    WindowCloseRequested(iced::window::Id),
}
```

Three categories:
- **User actions** → mutate state, re-validate, save config.
- **Async results** (`*Picked`) → receive `PathBuf` from `rfd::AsyncFileDialog`.
- **Thread messages** (`LogLine`, `TrayEvent`) → pushed from background subscriptions.

### 3.4 Subscription — composed via `Subscription::batch`

```rust
fn subscription(&self) -> Subscription<Message> {
    let mut subs = Vec::new();
    // Watcher (only when watching)
    if self.watching { subs.push(run_with(config, build_watcher_stream)); }
    // Tray polling (only when tray enabled + icon created)
    if self.tray_enabled && let Some(ids) = TRAY_IDS.get() { ... }
    // Close interception (always)
    subs.push(close_request_subscription());
    Subscription::batch(subs)
}
```

Iced diffs subscriptions on every state change. Dropping a subscription drops
its `StopGuard`, which signals the background thread to exit. No manual thread
management.

---

## 4. Thread communication

### 4.1 Watcher → GUI

```rust
let (log_tx, log_rx) = mpsc::unbounded::<String>();
watcher::start(src, dst, mode, pat, sound, file, log_tx, running);
log_rx.map(|s| Message::LogLine(s)).boxed()
```

### 4.2 Tray → GUI

```rust
let (event_tx, event_rx) = mpsc::unbounded::<TrayAction>();
tray::start(event_tx, running, ids...);
event_rx.map(|action| Message::TrayEvent(action)).boxed()
```

### 4.3 Stop signal

```rust
struct StopGuard(Arc<AtomicBool>);
impl Drop for StopGuard {
    fn drop(&mut self) { self.0.store(false, Ordering::Relaxed); }
}
```

The `StopGuard` is moved into the stream's closure. When Iced drops the
subscription, the stream drops, the guard fires, the thread's `while running`
loop exits. Zero `JoinHandle::abort()`.

---

## 5. System tray architecture (v0.9.9)

The tray has **three threads** on Linux, two on Windows:

### Phase 1: Platform bootstrap (`init_tray`)

Called from the `iced::application()` init closure:

```rust
iced::application(move || {
    if config.tray.enabled {
        match tray::init_tray() {
            Ok(ids) => { let _ = TRAY_IDS.set(ids); }
            Err(e) => eprintln!("DicomWatch: tray creation failed: {e}");
        }
    }
    AppState { ... }
})
```

**Linux** (`#[cfg(target_os = "linux")]`): `init_tray()` spawns a dedicated
GTK thread. `tray-icon` requires a GTK event loop on the creation thread —
without it, D-Bus messages never dispatch and the icon never appears. The
thread calls `gtk::init()`, creates the tray + menu via `build_tray()`, leaks
the tray handle, sends menu item IDs back through a `std::sync::mpsc::channel`,
then blocks on `gtk::main()` for the process lifetime.

**Windows** (`#[cfg(not(target_os = "linux"))]`): creates the tray directly
on the calling thread. Win32 tray messages are pumped by winit's event loop.

### Phase 2: Icon + menu creation (`build_tray`, private)

Decodes the embedded `icon.png` (via `include_bytes!`) to RGBA, builds the
context menu using localized strings from `t!("tray_menu.*")`, and creates
the tray icon via `tray_icon::TrayIconBuilder`:

- **Restore** — `t!("tray_menu.restore")`
- **Start/Stop Watching** — `t!("tray_menu.toggle_watch")`
- **Delete All Files** — `t!("tray_menu.delete_all")`
- **Quit** — `t!("tray_menu.quit")`

Menu item IDs are stored in a `static OnceLock` for the event polling thread.

### Phase 3: Event polling (`start`, background thread)

A subscription spawns a thread that polls `TrayIconEvent::receiver()` and
`MenuEvent::receiver()` every 200ms. Events are forwarded as `TrayAction`
variants to the GUI via `UnboundedSender`.

### Close interception

`window::Settings::exit_on_close_request` is set to `false`. The
`window::close_requests()` subscription fires on X button click:
- `tray_enabled == true` → `window::set_mode(id, Mode::Hidden)` — hide, don't quit.
- `tray_enabled == false` → `window::close(id)` — quit.

The "Quit" tray menu item calls `window::close(id)` directly, which exits
the app regardless of the setting.

---

## 6. Cross-platform strategy

### 6.1 Platform isolation

`#[cfg]` blocks exist only in:

- `watcher.rs` — sound playback (`paplay`/`aplay` on Unix, `PlaySoundW` on Windows)
- `watcher.rs` — Unix file permissions on extracted entries
- `tray.rs` — `gtk::init()` + `gtk::main()` on Linux in a dedicated thread
- `build.rs` — Windows `.ico` embedding

### 6.2 Sound playback

All bundled sounds use **WAV format** — the only format that works on both
Linux (`paplay`/`aplay`) and Windows (`PlaySoundW`).

- `alarm.wav` — notification when a ZIP is extracted
- `delete.wav` — notification when Delete All removes files (only if items were actually deleted)

### 6.3 Release packaging

| Platform | Zip contents |
|----------|-------------|
| Linux | `dicom-watch` + `install.sh` + `icon.png` + `config.toml.example` + `alarm.wav` + `delete.wav` |
| Windows | `dicom-watch.exe` + `config.toml.example` + `alarm.wav` + `delete.wav` |

Linux binary is dynamically linked (glibc, X11, Wayland, GTK3).
Windows binary is statically linked via `x86_64-pc-windows-gnu`.

---

## 7. Internationalization (i18n)

### 7.1 rust-i18n — compile-time embedded

```rust
#[macro_use] extern crate rust_i18n;
i18n!("locales");
```

Translation files in `locales/{en,pt-BR}.toml` are embedded into the binary at
compile time. No runtime file loading. Switch at runtime:

```rust
rust_i18n::set_locale("pt-BR");  // instant, no recompilation
```

### 7.2 Locale file structure

```toml
[status]       # status.watching, status.idle
[button]       # button.start, button.stop, button.clear_all, button.browse
[field]        # field.source_dir, field.dest_dir, sound_alert, delete_sound_alert, ...
[placeholder]  # placeholder values for text inputs
[section]      # section.settings, section.activity_log
[log]          # log.waiting, log.watching_start, log.tray_minimized
[error]        # error.source_missing, dest_missing, regex_invalid, sound_missing, delete_sound_missing
[tray]         # tray.setting
[tray_menu]    # tray_menu.restore, toggle_watch, delete_all, quit
```

---

## 8. Configuration system

### 8.1 TOML format

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

[delete_sound]
enabled = true
file = "delete.wav"

[locale]
language = "en"

[tray]
enabled = false
```

### 8.2 Validation on startup

- **Fail loud**: directories must exist, regex must compile, sound files must
  exist if enabled.
- **Opt-in with defaults**: `[tray]`, `[delete_sound]` use `#[serde(default)]`.
  Absent = disabled, no crash. `[sound]` and `[directories]` are always required.
- All errors go to `stderr` AND a native `rfd::MessageDialog` popup.

### 8.3 Save on every change

No separate "Save" button. Every field change in the GUI immediately serializes
`AppState` back to `config.toml` via `toml::to_string_pretty`.

### 8.4 Path resolution

```rust
pub fn resolve_path(path_str: &str, exe_dir: &Path) -> PathBuf {
    if Path::new(path_str).is_absolute() { path_str.into() }
    else { exe_dir.join(path_str) }
}
```

Relative paths (like `alarm.wav`) resolve against the directory containing the
binary. No hardcoded install paths.

---

## 9. Release pipeline

### 9.1 CI (GitHub Actions)

| Trigger | Action |
|---------|--------|
| Push to `master` | `cargo fmt --check` |
| Push tag `v*.*.*` | `cargo fmt --check`, then auto-create GitHub Release |

CI never compiles. All building, linting, and testing is local.

### 9.2 Version workflow

```
bump Cargo.toml → cargo build (sync Cargo.lock) → commit → merge
→ git tag vX.Y.Z <sha> → git push origin vX.Y.Z
→ CI creates release → cargo build --release → release.sh → release-windows.sh
```

Tags are **lightweight** (`git tag v0.9.6 <sha>`, never `-a`/`-m`).

---

## 10. Error handling patterns

| Layer | Strategy |
|-------|----------|
| **Startup config** | Crash with `stderr` message + native OS dialog (`rfd::MessageDialog`) |
| **Field validation** | Red error text in GUI on every keystroke; `Start` refuses if errors exist |
| **Watcher** | Log errors and continue; extraction failure doesn't stop watching |
| **Sound** | Fail silently — spawn and forget; missing audio system doesn't block the app |
| **Tray** | Log error and continue without tray icon; app remains fully functional |

---

## 11. Dependency rationale

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
| `tray-icon` 0.19 | Cross-platform system tray (GTK/AppIndicator on Linux, Win32 on Windows) |
| `image` 0.25 | PNG → RGBA conversion for tray icon |
| `gtk` 0.18 (Linux) | `gtk::init()` for tray on Linux |
| `winapi` (Win) | `PlaySoundW` for audio |
| `winres` (build) | Embed `.ico` in Windows `.exe` |

No async runtime, no HTTP client, no database.

---

## 12. Replication recipe

### 12.1 Skeleton

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

### 12.2 Rules

1. **AppState is flat and owned.** No lifetimes, no borrowed config fields.
2. **Message is a flat enum.** One variant per action.
3. **Thread → GUI via `mpsc::unbounded`.** The thread knows nothing about the GUI.
4. **Thread lifecycle via `AtomicBool` + `StopGuard`.** No `JoinHandle::abort()`.
5. **Config is validated before the GUI opens.** Crash with `rfd::MessageDialog`.
6. **Platform differences in `#[cfg]` blocks, not in business logic.**
7. **No async runtime** unless you have 10+ concurrent IO streams.
8. **All strings in locale files.** Never hardcode user-visible text.
9. **Save config on every field change.** No separate save button.
10. **Release CI does format-check only.** Build + package is local.
11. **Functions ≤ 30 lines. Files ≤ 500 lines.**
12. **All bundled audio files use WAV** for cross-platform compatibility.

### 12.3 Cargo.toml template

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

---

## 13. What this architecture avoids

- **No `Arc<Mutex<T>>` in production code.**
- **No async runtime.**
- **No event bus, no pub/sub.**
- **No widget library wrapping.**
- **No CI build matrix.**
- **No installer.**
- **No auto-updater.**
