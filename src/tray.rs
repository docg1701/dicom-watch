// System tray icon event polling. Runs in a background thread.
// The tray icon itself is created on the main thread in main.rs
// because GTK requires it to be on the same thread as the event loop.
//
// This module only polls TrayIconEvent and MenuEvent receivers.
//
// ponytail: single thread for tray events, no channel backpressure needed
// because the rate of tray events is human-scale (1 per click).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tray_icon::{TrayIconEvent, menu::MenuEvent};

use super::TrayAction;

pub fn start(
    action_sender: iced::futures::channel::mpsc::UnboundedSender<TrayAction>,
    running: Arc<AtomicBool>,
    id_restore: String,
    id_toggle: String,
    id_delete: String,
    id_quit: String,
) {
    thread::spawn(move || {
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
                let action = if event.id.0 == id_restore {
                    TrayAction::MenuRestore
                } else if event.id.0 == id_toggle {
                    TrayAction::MenuToggleWatch
                } else if event.id.0 == id_delete {
                    TrayAction::MenuDeleteAll
                } else if event.id.0 == id_quit {
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

/// Decode the embedded PNG into a tray icon RGBA buffer.
pub fn icon_from_bytes(bytes: &[u8]) -> Result<tray_icon::Icon, String> {
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

/// Build the tray icon and return it along with the menu item IDs.
/// Called once on the main thread before the Iced event loop.
pub fn build_tray() -> Result<(tray_icon::TrayIcon, String, String, String, String), String> {
    let icon = icon_from_bytes(crate::ICON_PNG)?;

    let menu = tray_icon::menu::Menu::new();

    let restore = tray_icon::menu::MenuItem::new("Restore", true, None);
    let toggle = tray_icon::menu::MenuItem::new("Start/Stop Watching", true, None);
    let delete = tray_icon::menu::MenuItem::new("Delete All Files", true, None);
    let quit = tray_icon::menu::MenuItem::new("Quit", true, None);

    let id_restore = restore.id().0.clone();
    let id_toggle = toggle.id().0.clone();
    let id_delete = delete.id().0.clone();
    let id_quit = quit.id().0.clone();

    menu.append_items(&[&restore, &toggle, &delete, &quit])
        .map_err(|e| format!("menu: {e}"))?;

    let tray = tray_icon::TrayIconBuilder::new()
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_tooltip("DicomWatch")
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| format!("tray: {e}"))?;

    Ok((tray, id_restore, id_toggle, id_delete, id_quit))
}
