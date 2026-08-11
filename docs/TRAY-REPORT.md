# Tray Implementation Report — v0.8.3 → v0.9.6

> Written for the next agent. Everything attempted, everything broken.

---

## Summary

The "minimize to system tray" feature was requested for v0.9.0. After 6 patch versions
(v0.9.1 through v0.9.6), the tray icon **still does not appear**. The app compiles and
runs without panics, but no tray icon is visible on Linux Mint Cinnamon.

---

## What was implemented

### Dependencies added

```toml
tray-icon = { version = "0.19", default-features = false }  # libxdo disabled
image = "0.25"   # PNG → RGBA conversion
gtk = "0.18"     # Linux only, for gtk::init()
```

`tray-icon` 0.19 pulls `muda` 0.15 (menus) and `libappindicator` 0.9 (Linux tray backend).
`libappindicator-sys` uses `libloading` — loads `libayatana-appindicator3.so.1` dynamically
at runtime. No compile-time dependency on `libappindicator3-dev`. The `libxdo` default
feature was disabled because the tray menu has no keyboard shortcuts.

### New files

- **`src/tray.rs`** — tray icon creation + event polling (~130 lines)
- **`docs/PLAN.md`** — implementation plan for the feature
- **`docs/ARCHITECTURE.md`** — project architecture document
- **`assets/alarm.wav`** — cross-platform notification sound (converted from `.ogg`)
- **`assets/delete.wav`** — cross-platform delete sound (converted from `.oga`)

### Config changes (`src/config.rs`)

```rust
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub tray: TrayConfig,                    // NEW
    #[serde(default)]
    pub delete_sound: DeleteSound,           // NEW (separate request)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TrayConfig {
    #[serde(default)]
    pub enabled: bool,  // default: false (opt-in)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DeleteSound {
    #[serde(default)]
    pub enabled: bool,  // default: false
    pub file: String,   // validated at startup if enabled
}
```

Both `tray` and `delete_sound` use `#[serde(default)]` on the Config field — absent in
config.toml = disabled/empty, no crash.

### Main state machine changes (`src/main.rs`)

**AppState** — 3 new fields:
```rust
tray_enabled: bool,
delete_sound_enabled: bool,
delete_sound_file: String,
```

**Message** — 6 new variants:
```rust
TrayEnabledChanged(bool),
TrayEvent(TrayAction),
WindowCloseRequested(iced::window::Id),
DeleteSoundEnabledChanged(bool),
DeleteSoundFileChanged(String),
BrowseDeleteSoundFile,
DeleteSoundFilePicked(Option<PathBuf>),
```

**TrayAction** — internal enum for tray-to-GUI communication:
```rust
enum TrayAction {
    Click,           // tray icon left-click (Linux) or double-click (Windows)
    MenuRestore,
    MenuToggleWatch,
    MenuDeleteAll,
    MenuQuit,
}
```

### Close interception

```rust
// subscription.rs
fn close_request_subscription() -> Subscription<Message> {
    window::close_requests().map(Message::WindowCloseRequested)
}

// update()
Message::WindowCloseRequested(id) => {
    if self.tray_enabled {
        self.log.push(tr("log.tray_minimized"));
        window::set_mode(id, iced::window::Mode::Hidden)  // hide, don't quit
    } else {
        window::close(id)  // real quit
    }
}
```

### Tray icon creation — 3 architecture attempts

#### Attempt 1 (v0.9.0): Background thread

Created tray in the subscription's spawned thread (`build_tray_stream` → `thread::spawn`).
Loaded icon from disk file `icon.png` next to executable.

**Result**: Panic — `GTK has not been initialized`. GTK wasn't initialized in the background
thread. Also, `icon.png` doesn't exist next to the binary at runtime (the window icon uses
`include_bytes!`, the tray tried to read from disk).

#### Attempt 2 (v0.9.1–v0.9.4): Background thread with embedded icon + gtk::init

Changed tray icon loading from disk file → `include_bytes!("../assets/icon.png")` embedded
at compile time. Added `gtk::init()` call at the top of the tray thread.

**Result**: No panic, no error — but tray icon never appears. GTK requires the tray icon
to be created on a thread with a running GTK event loop. Our background thread calls
`gtk::init()` but has no event loop. The libappindicator creation likely fails silently
or creates an icon that never shows because no GTK main loop pumps events on that thread.

#### Attempt 3 (v0.9.5–v0.9.6): Main thread in Iced init closure

Moved tray creation to the `iced::application()` init closure (the `move || AppState {...}`
function). This runs on the main thread. Added `gtk::init()` before tray creation. The
tray handle is leaked via `std::mem::forget()` for process-lifetime persistence. Menu
item IDs are stored in a `static OnceLock` and read by the event-polling subscription.

```rust
// main()
iced::application(
    move || {
        if config.tray.enabled {
            #[cfg(target_os = "linux")]
            if let Err(e) = gtk::init() {
                eprintln!("...");
            }
            match tray::build_tray() {
                Ok((tray, ids...)) => {
                    std::mem::forget(tray);  // leak for lifetime
                    TRAY_IDS.set(ids).ok();  // store menu IDs
                }
                Err(e) => eprintln!("..."),
            }
        }
        AppState { ... }
    },
    ...
)
```

**Result**: No panic, no stderr output — but tray icon still doesn't appear. The init
closure runs during `iced::application().run()`, but it may run BEFORE the winit/GTK
event loop actually starts pumping events. On Linux, `gtk::init()` initializes the
library but doesn't start the main loop. The `libappindicator` backend creates the
indicator via D-Bus, which might need the GTK main loop to be pumping to actually
register and display.

### Tray menu

Menu items (hardcoded English text, not i18n):
- "Restore"
- "Start/Stop Watching"
- "Delete All Files"
- "Quit"

Menu is shown on right-click only (`with_menu_on_left_click(false)`).

Restore behavior:
- Linux: left-click on tray icon
- Windows: double-click on tray icon

### Tray event polling (subscription thread)

A separate thread spawned via `Subscription::run_with(TrayPollConfig, build_tray_stream)`
polls `TrayIconEvent::receiver()` and `MenuEvent::receiver()` every 200ms. Received
events are forwarded to the main loop via `UnboundedSender<TrayAction>` mapped to
`Message::TrayEvent(action)`. This thread does NOT create or own the tray icon — it
only polls the global receivers.

### Sound changes (separate request, same cycle)

- Converted `alarm-001.ogg` → `alarm.wav` (WAV, cross-platform)
- Converted `/usr/share/mint-artwork/sounds/unmaximize.oga` → `delete.wav`
- Both bundled in Linux and Windows release zips
- `[delete_sound]` config section with independent toggle
- Delete sound only plays if `removed > 0`
- Validation: if `delete_sound.enabled = true` but `file` is missing/invalid → crash with dialog

### UI changes

- Two sound toggle rows: "Alert on processed" + "Alert on delete" with browse buttons
- "Minimize to system tray" toggler below sound settings

### Locales

- `en.toml` + `pt-BR.toml` updated with `[tray]`, `[tray_menu]`, `[delete_sound]` keys
- Tray menu text is hardcoded English (not using locale keys — the menu is created before
  `rust_i18n::set_locale()` is called)

---

## Known problems

### 1. Tray icon doesn't appear (UNRESOLVED)

The core issue: `tray-icon` requires the tray icon to be created on a thread with a
running GTK event loop. In our architecture, the Iced/winit event loop runs GTK on the
main thread, but we don't know exactly when the event loop starts pumping relative to
the init closure.

Possible root causes:
- The init closure fires before GTK main loop iteration starts — `libappindicator` can't
  register the D-Bus service without the loop pumping.
- The `iced::application()` init closure may not be the right place — need a `Task` that
  runs after the first frame or after a specific event.
- The tray creation succeeds but the icon never shows because the GTK event loop doesn't
  process the indicator's D-Bus registration until later.

Potential fixes for the next agent:
- Use Iced's `Task::done()` or a startup `Message` to defer tray creation to the first
  `update()` call (which runs during the event loop).
- Or use winit's `Event::NewEvents(StartCause::Init)` via a custom subscription.
- Or use `gtk::main_iteration()` to pump GTK events once after creation.
- Or try a different tray backend for Linux: `ksni` (pure Rust D-Bus, no GTK).

### 2. Menu text is hardcoded English

The tray menu items are created with hardcoded strings ("Restore", "Quit", etc.) because
the tray is created before `rust_i18n::set_locale()` is called (the locale is set from
config, which happens before the init closure). But the menu text could still use `tr()`
if we moved locale setup earlier.

### 3. Windows cross-compilation not tested

The Windows binary compiles successfully (cross-compiled from Linux with
`x86_64-pc-windows-gnu`), but has never been tested on a real Windows machine.
The `gtk::init()` call is `#[cfg(target_os = "linux")]` so it won't affect Windows.
On Windows, `tray-icon` uses `Shell_NotifyIcon` via `windows-sys` — this should work
if the Win32 event loop is running. The init closure approach might actually work on
Windows where event loop behavior differs from GTK.

### 4. `libxdo` disabled but not needed

Disabled `default-features` on `tray-icon` to avoid `libxdo` runtime dep. This is
safe — we don't use keyboard shortcuts in menu items. But it's one more divergence
from the default configuration.

---

## Release artifacts (all on GitHub)

| Version | Linux zip | Windows zip |
|---------|-----------|-------------|
| v0.9.0 | 5.8M | — |
| v0.9.1 | 5.8M | 5.0M |
| v0.9.2 | 5.8M | 5.0M |
| v0.9.3 | 5.9M | 5.1M |
| v0.9.4 | 5.9M | 5.1M |
| v0.9.5 | 5.9M | 5.1M |
| v0.9.6 | 5.9M | 5.1M |

Each zip contains: binary, `config.toml.example`, `alarm.wav`, `delete.wav` (+ `install.sh` + `icon.png` on Linux).

---

## Files changed (cumulative from v0.8.3)

```
Cargo.toml          — deps: tray-icon, image, gtk(Linux)
Cargo.lock          — auto
src/main.rs         — +~200 lines (tray, delete sound, UI, subscriptions)
src/tray.rs         — NEW, ~120 lines
src/config.rs       — +TrayConfig, +DeleteSound
locales/en.toml     — +tray, +tray_menu, +delete_sound, +log.tray_minimized
locales/pt-BR.toml  — same, translated
config.toml.example — +[tray], +[delete_sound]
assets/alarm.wav    — NEW (converted from alarm-001.ogg)
assets/delete.wav   — NEW (converted from unmaximize.oga)
scripts/release.sh       — +alarm.wav, +delete.wav
scripts/release-windows.sh — +alarm.wav, +delete.wav
docs/ARCHITECTURE.md — NEW
docs/PLAN.md         — NEW
AGENTS.md            — +find-docs rules
```

---

## What works correctly

- App compiles and runs without panics/crashes
- Window hides on close when `tray.enabled = true` (`iced::window::Mode::Hidden`)
- Close interception via `window::close_requests()` works
- Delete sound plays only when files were actually removed (`removed > 0`)
- Both `.wav` sound files bundled in release zips
- Config backward-compatible (absent `[tray]`/`[delete_sound]` = disabled)
- Linux and Windows cross-compilation successful
- fmt, clippy, test: all pass

---

## What doesn't work

- **Tray icon never appears** on Linux Mint Cinnamon (GTK/libappindicator/ayatana)
- Tray has never been tested on Windows
- Tray menu item text is hardcoded English, not i18n-aware
