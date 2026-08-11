// System tray icon + context menu. Runs in a background thread.
// Communicates with the GUI via UnboundedSender<TrayAction>.
//
// ponytail: single thread for tray events, no channel backpressure needed
// because the rate of tray events is human-scale (1 per click).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tray_icon::{
    TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

use super::TrayAction;

// Icon embedded at compile time — same bytes as the main window icon.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

pub fn start(
    action_sender: iced::futures::channel::mpsc::UnboundedSender<TrayAction>,
    running: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        #[cfg(target_os = "linux")]
        if let Err(e) = gtk::init() {
            eprintln!("tray: gtk::init failed: {e}");
            return;
        }

        // Decode embedded PNG → RGBA for tray_icon.
        let icon = match load_icon_from_bytes(ICON_PNG) {
            Ok(ic) => ic,
            Err(e) => {
                eprintln!("tray: failed to decode embedded icon: {e}");
                return;
            }
        };

        // --- Build context menu ---
        let menu = Menu::new();

        let restore = MenuItem::new("Restore", true, None);
        let toggle = MenuItem::new("Start/Stop Watching", true, None);
        let delete = MenuItem::new("Delete All Files", true, None);
        let quit = MenuItem::new("Quit", true, None);

        let id_restore = restore.id();
        let id_toggle = toggle.id();
        let id_delete = delete.id();
        let id_quit = quit.id();

        if let Err(e) = menu.append_items(&[&restore, &toggle, &delete, &quit]) {
            eprintln!("tray: failed to build menu: {e}");
            return;
        }

        // --- Build tray icon ---
        let _tray = match TrayIconBuilder::new()
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_tooltip("DicomWatch")
            .with_menu_on_left_click(false)
            .build()
        {
            Ok(t) => t,
            Err(e) => {
                eprintln!("tray: failed to create tray icon: {e}");
                return;
            }
        };

        // --- Event loop ---
        while running.load(Ordering::Relaxed) {
            if let Ok(event) = TrayIconEvent::receiver().try_recv() {
                match event {
                    TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        ..
                    } => {
                        let _ = action_sender.unbounded_send(TrayAction::Click);
                    }
                    TrayIconEvent::DoubleClick { .. } => {
                        let _ = action_sender.unbounded_send(TrayAction::Click);
                    }
                    _ => {}
                }
            }

            if let Ok(event) = MenuEvent::receiver().try_recv() {
                let action = if event.id == id_restore {
                    TrayAction::MenuRestore
                } else if event.id == id_toggle {
                    TrayAction::MenuToggleWatch
                } else if event.id == id_delete {
                    TrayAction::MenuDeleteAll
                } else if event.id == id_quit {
                    TrayAction::MenuQuit
                } else {
                    continue;
                };
                let _ = action_sender.unbounded_send(action);
            }

            thread::sleep(Duration::from_millis(200));
        }
    });
}

fn load_icon_from_bytes(bytes: &[u8]) -> Result<tray_icon::Icon, String> {
    let img = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("cannot guess image format: {e}"))?
        .decode()
        .map_err(|e| format!("cannot decode icon: {e}"))?
        .to_rgba8();

    let (w, h) = img.dimensions();
    let rgba = img.into_raw();

    tray_icon::Icon::from_rgba(rgba, w, h).map_err(|e| format!("invalid icon: {e}"))
}
