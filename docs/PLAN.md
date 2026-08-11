# PLAN — v0.9.0: Minimize to System Tray

> Feature branch: `feature/minimize-to-tray`.
> Implement after reading `docs/ARCHITECTURE.md` for project conventions.

---

## Goal

When the user toggles "Minimize to system tray" in the settings:

- Closing the window (X button) hides it instead of quitting.
- A tray icon appears in the system notification area.
- **Left-click** (Linux/macOS) or **double-click** (Windows) on the tray icon restores the window.
- **Right-click** opens a context menu: Restore, Start/Stop Watching, Delete All Files, Quit.
- "Quit" in the tray menu is the only way to truly exit when tray mode is active.
- Toggling the setting off removes the tray icon and restores normal close behavior.

---

## 1. Dependencies

### 1.1 Cargo.toml

```toml
[dependencies]
# ... existing ...
tray-icon = "0.19"
image = "0.25"   # PNG → RGBA for tray icon
```

### 1.2 Rationale

| Crate | Why |
|-------|-----|
| `tray-icon` 0.19 | Cross-platform tray (Windows, macOS, Linux). Maintained by Tauri team. Same `muda` menu system as the Tauri ecosystem. |
| `image` 0.25 | Converts `assets/icon.png` to raw RGBA bytes for `tray_icon::Icon::from_rgba()`. The `image` crate is the standard Rust imaging library. |

No async runtime needed. `tray-icon` uses blocking channels (`std::sync::mpsc`-style receivers) for events — same pattern as `notify` in `watcher.rs`.

---

## 2. Config — `src/config.rs`

### 2.1 New struct

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrayConfig {
    #[serde(default)]  // default: false
    pub enabled: bool,
}
```

### 2.2 Add to `Config`

```rust
pub struct Config {
    pub directories: Directories,
    pub filter: Filter,
    pub sound: Sound,
    #[serde(default = "default_locale")]
    pub locale: LocaleConfig,
    #[serde(default)]                       // <-- NEW
    pub tray: TrayConfig,                   // <-- NEW
}
```

`#[serde(default)]` on `TrayConfig` + `#[serde(default)]` on the field means any existing `config.toml` without `[tray]` keeps working (defaults to `enabled = false`).

### 2.3 Validation

No additional validation needed. `tray.enabled` is an optional boolean.

---

## 3. Locales — `locales/en.toml` and `locales/pt-BR.toml`

### 3.1 New keys (en.toml)

```toml
[tray]
setting = "Minimize to system tray"

[tray_menu]
restore = "Restore"
toggle_watch = "Start/Stop Watching"
delete_all = "Delete All Files"
quit = "Quit"
```

### 3.2 New keys (pt-BR.toml)

```toml
[tray]
setting = "Minimizar para a bandeja"

[tray_menu]
restore = "Restaurar"
toggle_watch = "Iniciar/Parar Monitoramento"
delete_all = "Limpar Todos os Arquivos"
quit = "Sair"
```

The menu item text for Start/Stop is static — a single toggle item. The user sees current watch state when they restore the window. No dynamic menu text needed.

---

## 4. New Message variants — `src/main.rs`

Add to the `Message` enum:

```rust
#[derive(Debug, Clone)]
enum Message {
    // === existing ===
    WatchToggled,
    DeleteAll,
    SourceDirChanged(String),
    DestDirChanged(String),
    FilterModeChanged(FilterMode),
    FilterPatternChanged(String),
    SoundEnabledChanged(bool),
    SoundFileChanged(String),
    LogLine(String),
    BrowseSourceDir,
    BrowseDestDir,
    BrowseSoundFile,
    SourceDirPicked(Option<std::path::PathBuf>),
    DestDirPicked(Option<std::path::PathBuf>),
    SoundFilePicked(Option<std::path::PathBuf>),
    ToggleLocale,

    // === new v0.9.0 ===
    TrayEnabledChanged(bool),
    TrayEvent(TrayAction),                    // received from tray subscription thread
    WindowCloseRequested(iced::window::Id),   // user clicked X button
}

#[derive(Debug, Clone)]
enum TrayAction {
    Click,        // left click (Linux/macOS) or double-click (Windows)
    MenuRestore,
    MenuToggleWatch,
    MenuDeleteAll,
    MenuQuit,
}
```

`TrayAction` is a plain enum — not a Message variant itself, but carried by `Message::TrayEvent`. This keeps the outer enum flat.

---

## 5. AppState changes — `src/main.rs`

Add one field to `AppState`:

```rust
struct AppState {
    // ... existing fields ...
    tray_enabled: bool,   // mirrors config.tray.enabled; persisted on change
}
```

Initialize from config in the `move ||` closure:

```rust
AppState {
    // ... existing ...
    tray_enabled: config.tray.enabled,
}
```

---

## 6. Tray subscription — `src/main.rs`

### 6.1 TrayConfig (for `Subscription::run_with`)

```rust
#[derive(Hash, Clone)]
struct TrayConfig {
    icon_path: PathBuf,
    watching: bool,
}
```

`PathBuf` is `Hash`. `watching` is passed so the tray thread can log when toggling watch state via the menu.

### 6.2 Tray stream builder

```rust
fn build_tray_stream(
    config: &TrayConfig,
) -> iced::futures::stream::BoxStream<'static, Message> {
    let running = Arc::new(AtomicBool::new(true));
    let guard = StopGuard(running.clone());

    let (event_tx, event_rx) = iced::futures::channel::mpsc::unbounded::<TrayAction>();

    tray_thread::start(
        config.icon_path.clone(),
        event_tx,
        running,
    );

    use iced::futures::StreamExt;
    event_rx
        .map(move |action| {
            let _hold = &guard;
            Message::TrayEvent(action)
        })
        .boxed()
}
```

Same three-part pattern as `build_watcher_stream`:
1. `Arc<AtomicBool>` + `StopGuard` for lifecycle.
2. `mpsc::unbounded` channel to bridge thread → GUI.
3. Thread spawn, map channel to `Message`.

### 6.3 Tray thread module — `src/tray.rs` (new file)

```rust
// src/tray.rs
// System tray icon + context menu. Runs in a background thread.
// Communicates with the GUI via UnboundedSender<TrayAction>.

use crate::config::resolve_path;
use image::ImageReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tray_icon::{
    TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuItem, MenuEvent},
};

use super::TrayAction;

pub fn start(
    icon_path: PathBuf,
    action_sender: iced::futures::channel::mpsc::UnboundedSender<TrayAction>,
    running: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        // --- Load icon ---
        let icon = match load_icon(&icon_path) {
            Ok(ic) => ic,
            Err(e) => {
                let _ = action_sender.unbounded_send(TrayAction::Quit); // fallback: bail
                eprintln!("tray: failed to load icon: {e}");
                return;
            }
        };

        // --- Build menu ---
        let menu = Menu::new();

        let restore_item = MenuItem::new("Restore", true, None);
        let toggle_item = MenuItem::new("Start/Stop Watching", true, None);
        let delete_item = MenuItem::new("Delete All Files", true, None);
        let quit_item   = MenuItem::new("Quit", true, None);

        // Store IDs to match events later.
        let id_restore = restore_item.id();
        let id_toggle  = toggle_item.id();
        let id_delete  = delete_item.id();
        let id_quit    = quit_item.id();

        menu.append_items(&[&restore_item, &toggle_item, &delete_item, &quit_item])
            .expect("tray: menu append");

        // --- Build tray icon ---
        let _tray = match TrayIconBuilder::new()
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_tooltip("DicomWatch")
            .with_menu_on_left_click(false) // menu only on right-click
            .build()
        {
            Ok(t) => t,
            Err(e) => {
                let _ = action_sender.unbounded_send(TrayAction::Quit);
                eprintln!("tray: failed to create tray icon: {e}");
                return;
            }
        };

        // --- Event loop ---
        while running.load(Ordering::Relaxed) {
            // Check tray icon clicks
            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                match event {
                    TrayIconEvent::Click { button, .. }
                        if button == tray_icon::MouseButton::Left =>
                    {
                        let _ = action_sender.unbounded_send(TrayAction::Click);
                    }
                    TrayIconEvent::DoubleClick { .. } => {
                        let _ = action_sender.unbounded_send(TrayAction::Click);
                    }
                    _ => {}
                }
            }

            // Check menu clicks
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                let action = match event.id {
                    id if id == id_restore => TrayAction::MenuRestore,
                    id if id == id_toggle  => TrayAction::MenuToggleWatch,
                    id if id == id_delete  => TrayAction::MenuDeleteAll,
                    id if id == id_quit    => TrayAction::MenuQuit,
                    _ => continue,
                };
                let _ = action_sender.unbounded_send(action);
            }

            thread::sleep(Duration::from_millis(200));
        }

        // The tray icon is dropped here when running becomes false
        // (StopGuard drop → AtomicBool = false → loop exits).
    });
}

fn load_icon(path: &std::path::Path) -> Result<tray_icon::Icon, String> {
    let img = ImageReader::open(path)
        .map_err(|e| format!("cannot open icon: {e}"))?
        .decode()
        .map_err(|e| format!("cannot decode icon: {e}"))?
        .to_rgba8();

    let (w, h) = img.dimensions();
    let rgba = img.into_raw();

    tray_icon::Icon::from_rgba(rgba, w, h)
        .map_err(|e| format!("invalid icon: {e}"))
}
```

### 6.4 `src/main.rs` — module declaration

At the top, add:

```rust
mod tray;
```

---

## 7. Update logic — `update()` in `src/main.rs`

### 7.1 New match arms

```rust
Message::TrayEnabledChanged(v) => {
    self.tray_enabled = v;
    save_config(self);
    Task::none()
}

Message::WindowCloseRequested(id) => {
    if self.tray_enabled {
        self.log.push(tr("log.tray_minimized"));
        window::set_mode(id, iced::window::Mode::Hidden)
    } else {
        window::close(id)
    }
}

Message::TrayEvent(action) => match action {
    TrayAction::Click | TrayAction::MenuRestore => {
        window::latest().and_then(|id| window::set_mode(id, iced::window::Mode::Windowed))
    }
    TrayAction::MenuToggleWatch => {
        // Reuse existing WatchToggled logic
        if self.watching {
            self.watching = false;
        } else {
            self.field_errors = validate_fields(self);
            if self.field_errors.is_empty() {
                self.watching = true;
                self.log.push(tr("log.watching_start"));
            }
        }
        Task::none()
    }
    TrayAction::MenuDeleteAll => {
        // Reuse existing DeleteAll logic — extract to helper or duplicate here.
        // See section 11 for the extract-helper approach.
        Self::delete_all_files(self)
    }
    TrayAction::MenuQuit => {
        window::latest().and_then(window::close)
    }
}
```

### 7.2 Import additions

```rust
use iced::window;
```

---

## 8. Close request subscription — `subscription()`

### 8.1 New subscription

```rust
fn close_request_subscription() -> Subscription<Message> {
    window::close_requests().map(|(id, ())| Message::WindowCloseRequested(id))
}
```

### 8.2 Combine subscriptions

Update `subscription()` to return a combined set:

```rust
fn subscription(&self) -> Subscription<Message> {
    let mut subs: Vec<Subscription<Message>> = Vec::new();

    // Watcher
    if self.watching {
        let config = WatcherConfig {
            source_dir: PathBuf::from(&self.source_dir),
            dest_dir: PathBuf::from(&self.dest_dir),
            filter_mode_str: self.filter_mode.to_string(),
            pattern: self.filter_pattern.clone(),
            sound_enabled: self.sound_enabled,
            sound_file: resolve_path(&self.sound_file, &self.exe_dir),
        };
        subs.push(Subscription::run_with(config, build_watcher_stream));
    }

    // Tray
    if self.tray_enabled {
        let tray_config = TrayConfig {
            icon_path: resolve_path("icon.png", &self.exe_dir),
            watching: self.watching,
        };
        subs.push(Subscription::run_with(tray_config, build_tray_stream));
    }

    // Close intercept (always active — the handler decides whether to hide or quit)
    subs.push(close_request_subscription());

    Subscription::batch(subs)
}
```

---

## 9. View — `view()` in `src/main.rs`

### 9.1 Tray toggle in settings card

Inside the settings card column, add a row between the sound row and the bottom, before `].spacing(0)`:

```rust
// After the sound row, before the closing ].spacing(0):
Space::new().height(6),
row![
    toggler(self.tray_enabled)
        .label(tr("tray.setting"))
        .text_size(13)
        .on_toggle(Message::TrayEnabledChanged),
]
```

No separate padding — the row follows the same spacing pattern as other settings rows.

---

## 10. Config save — `save_config()`

Add `tray` to the serialized config:

```rust
fn save_config(state: &AppState) {
    let config = Config {
        directories: config::Directories { ... },
        filter: config::Filter { ... },
        sound: config::Sound { ... },
        locale: config::LocaleConfig { ... },
        tray: config::TrayConfig {
            enabled: state.tray_enabled,
        },
    };
    // ... rest unchanged
}
```

---

## 11. Refactor: extract delete_all_files helper

`MenuDeleteAll` needs to run the same logic as `Message::DeleteAll`. Extract a helper to avoid duplication:

```rust
impl AppState {
    fn delete_all_files(&mut self) -> Task<Message> {
        let dest = std::path::Path::new(&self.dest_dir);
        match std::fs::read_dir(dest) {
            Ok(entries) => {
                let mut removed = 0;
                for entry in entries.flatten() {
                    let path = entry.path();
                    let result = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                    if result.is_ok() {
                        removed += 1;
                    } else if let Err(e) = result {
                        self.log.push(format!(
                            "[Delete] Failed to remove '{}': {}",
                            path.display(),
                            e
                        ));
                    }
                }
                self.log.push(format!(
                    "[Delete] Removed {} item(s) from '{}'.",
                    removed,
                    dest.display()
                ));
            }
            Err(e) => {
                self.log.push(format!(
                    "[Delete] Cannot read directory '{}': {}",
                    dest.display(),
                    e
                ));
            }
        }
        Task::none()
    }
}
```

Then in `update()`, replace the `Message::DeleteAll` body with:

```rust
Message::DeleteAll => self.delete_all_files(),
```

---

## 12. New locale key

Add one log entry key to both locale files:

```toml
# en.toml
[log]
# ... existing ...
tray_minimized = "Window hidden to system tray. Right-click tray icon to restore or quit."
```

```toml
# pt-BR.toml
[log]
# ... existing ...
tray_minimized = "Janela minimizada para a bandeja. Clique com botão direito no ícone para restaurar ou sair."
```

---

## 13. Platform-specific behavior

### 13.1 Linux

- `TrayIconEvent::Click(MouseButton::Left)` fires on left-click — this is the restore trigger.
- `DoubleClick` does NOT fire on Linux (unimplemented in `tray-icon` for Linux).
- `with_menu_on_left_click(false)` is ignored on Linux — the menu always shows on right-click only, which is the desired behavior.
- The tray icon uses the XDG StatusNotifierItem protocol (or libappindicator fallback, depending on `tray-icon` features).

### 13.2 Windows

- `TrayIconEvent::DoubleClick` fires on double-click — this is the restore trigger.
- A single left-click on Windows would also fire `TrayIconEvent::Click` — but we ignore it (no restore on single click, that's the platform convention).
- Wait: if we ignore single Click on Windows, we need the double-click for restore. The code above sends `TrayAction::Click` for both `Click(Left)` and `DoubleClick`. On Windows, this means both single-click AND double-click restore. That's acceptable — better too responsive than not enough. The implementer can refine with `#[cfg]` if needed.

### 13.3 macOS

Not a target, but `tray-icon` supports macOS via `TrayIconEvent::Click(MouseButton::Left)`.

---

## 14. Implementation order

1. **Add dependencies** — `tray-icon`, `image` to `Cargo.toml`. Run `cargo build` to sync `Cargo.lock`.
2. **Update config** — new `TrayConfig` struct, add `tray` field to `Config`. Update `save_config()`.
3. **Update locales** — add `[tray]`, `[tray_menu]`, and `log.tray_minimized` keys.
4. **Create `src/tray.rs`** — the tray thread module.
5. **Add `mod tray`** to `main.rs`.
6. **Update `AppState`** — add `tray_enabled` field, init from config.
7. **Update `Message`** — add `TrayEnabledChanged`, `TrayEvent(TrayAction)`, `WindowCloseRequested`.
8. **Add `TrayAction` enum** and `TrayConfig` struct.
9. **Add `build_tray_stream`** and `close_request_subscription` functions.
10. **Update `subscription()`** — combine watcher + tray + close_request.
11. **Refactor `delete_all_files`** — extract helper, use in both `DeleteAll` and `MenuDeleteAll`.
12. **Add `update()` arms** — `TrayEnabledChanged`, `WindowCloseRequested`, `TrayEvent`.
13. **Update `view()`** — add toggler for tray setting.
14. **`cargo fmt --check && cargo clippy -- -D warnings && cargo test`**
15. **Test manually** — run without tray, enable tray, close window, restore, menu actions, quit.
16. **Bump version** → `0.9.0`, commit, merge, tag, release.

---

## 15. File manifest (summary of changes)

| File | Action |
|------|--------|
| `Cargo.toml` | Add `tray-icon`, `image` |
| `Cargo.lock` | Auto-updated by `cargo build` |
| `src/config.rs` | Add `TrayConfig`, add `tray` field to `Config` |
| `src/tray.rs` | **New file** — tray thread module |
| `src/main.rs` | Add `mod tray`, `TrayAction`, `TrayConfig`, new Messages, new fields, new subscriptions, new `update()` arms, new `view()` row, extract `delete_all_files` helper |
| `locales/en.toml` | Add `[tray]`, `[tray_menu]`, `log.tray_minimized` |
| `locales/pt-BR.toml` | Add same keys translated |
| `config.toml.example` | Add `[tray]` section (commented example) |

---

## 16. What NOT to do

- **Do NOT** add a bidirectional channel to update menu item text dynamically. Static toggle label is simpler and sufficient.
- **Do NOT** add `#[cfg]` blocks to the tray thread for click behavior — the unified `Click | DoubleClick → TrayAction::Click` covers all platforms.
- **Do NOT** persist the window's Hidden/Visible state across restarts. Always start windowed.
- **Do NOT** show the tray icon when `tray_enabled` is false. The subscription check handles this.
- **Do NOT** create/destroy the tray icon on window hide/show — it stays alive while the subscription is active.
