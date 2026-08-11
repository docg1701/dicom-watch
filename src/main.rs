mod config;
mod tray;
mod watcher;

#[macro_use]
extern crate rust_i18n;

i18n!("locales");

use config::{Config, FilterMode, resolve_path};
use iced::widget::container as container_mod;
use iced::widget::text::Shaping;
use iced::widget::{
    Space, button, column, container, pick_list, row, scrollable, text, text_input, toggler,
};
use iced::window;
use iced::{Alignment, Element, Font, Length, Subscription, Task};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

// Tray menu item IDs set once at startup, read by subscription.
static TRAY_IDS: OnceLock<(String, String, String, String)> = OnceLock::new();

const TITLE: &str = concat!("DicomWatch v", env!("CARGO_PKG_VERSION"));

// Icon embedded at compile time for both window and tray.
pub(crate) const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

const BOLD: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::DEFAULT
};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

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

    locale: String,

    watching: bool,
    tray_enabled: bool,

    log: Vec<String>,
    field_errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    WatchToggled,
    DeleteAll,
    SourceDirChanged(String),
    DestDirChanged(String),
    FilterModeChanged(FilterMode),
    FilterPatternChanged(String),
    SoundEnabledChanged(bool),
    SoundFileChanged(String),
    DeleteSoundEnabledChanged(bool),
    DeleteSoundFileChanged(String),
    LogLine(String),
    BrowseSourceDir,
    BrowseDestDir,
    BrowseSoundFile,
    BrowseDeleteSoundFile,
    SourceDirPicked(Option<std::path::PathBuf>),
    DestDirPicked(Option<std::path::PathBuf>),
    SoundFilePicked(Option<std::path::PathBuf>),
    DeleteSoundFilePicked(Option<std::path::PathBuf>),
    ToggleLocale,
    TrayEnabledChanged(bool),
    TrayEvent(TrayAction),
    WindowCloseRequested(iced::window::Id),
}

#[derive(Debug, Clone)]
enum TrayAction {
    Click,
    MenuRestore,
    MenuToggleWatch,
    MenuDeleteAll,
    MenuQuit,
}

// ---------------------------------------------------------------------------
// Subscription data (must be Hash)
// ---------------------------------------------------------------------------

#[derive(Hash, Clone)]
struct WatcherConfig {
    source_dir: PathBuf,
    dest_dir: PathBuf,
    filter_mode_str: String,
    pattern: String,
    sound_enabled: bool,
    sound_file: PathBuf,
}

// ---------------------------------------------------------------------------
// Tray subscription data
// ---------------------------------------------------------------------------

#[derive(Hash, Clone)]
struct TrayPollConfig {
    id_restore: String,
    id_toggle: String,
    id_delete: String,
    id_quit: String,
}

// ---------------------------------------------------------------------------
// Stop guard
// ---------------------------------------------------------------------------

struct StopGuard(Arc<AtomicBool>);

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Save & validate
// ---------------------------------------------------------------------------

fn save_config(state: &AppState) {
    let config = Config {
        directories: config::Directories {
            source: state.source_dir.clone(),
            destination: state.dest_dir.clone(),
        },
        filter: config::Filter {
            mode_str: state.filter_mode.to_string(),
            pattern: state.filter_pattern.clone(),
        },
        sound: config::Sound {
            enabled: state.sound_enabled,
            file: state.sound_file.clone(),
        },
        delete_sound: config::DeleteSound {
            enabled: state.delete_sound_enabled,
            file: state.delete_sound_file.clone(),
        },
        locale: config::LocaleConfig {
            language: state.locale.clone(),
        },
        tray: config::TrayConfig {
            enabled: state.tray_enabled,
        },
    };
    let toml_str = toml::to_string_pretty(&config).unwrap_or_default();
    if let Err(e) = std::fs::write(&state.config_path, toml_str) {
        eprintln!("Failed to save config: {}", e);
    }
}

fn validate_fields(state: &AppState) -> Vec<String> {
    let mut errors = Vec::new();
    let source = std::path::Path::new(&state.source_dir);
    if !source.exists() || !source.is_dir() {
        errors.push(t!("error.source_missing", path = state.source_dir.as_str()).to_string());
    }
    let dest = std::path::Path::new(&state.dest_dir);
    if !dest.exists() || !dest.is_dir() {
        errors.push(t!("error.dest_missing", path = state.dest_dir.as_str()).to_string());
    }
    if state.filter_mode == FilterMode::Regex
        && let Err(e) = regex::Regex::new(&state.filter_pattern)
    {
        errors.push(t!("error.regex_invalid", error = e.to_string()).to_string());
    }
    if state.sound_enabled {
        let sound_path = resolve_path(&state.sound_file, &state.exe_dir);
        if !sound_path.exists() {
            errors.push(
                t!(
                    "error.sound_missing",
                    path = sound_path.display().to_string()
                )
                .to_string(),
            );
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// Stream builder (fn ptr — no captures)
// ---------------------------------------------------------------------------

fn build_watcher_stream(
    config: &WatcherConfig,
) -> iced::futures::stream::BoxStream<'static, Message> {
    let running = Arc::new(AtomicBool::new(true));
    let guard = StopGuard(running.clone());

    let (log_tx, log_rx) = iced::futures::channel::mpsc::unbounded::<String>();

    let filter_mode = match config.filter_mode_str.as_str() {
        "glob" => FilterMode::Glob,
        _ => FilterMode::Regex,
    };

    watcher::start(
        config.source_dir.clone(),
        config.dest_dir.clone(),
        filter_mode,
        config.pattern.clone(),
        config.sound_enabled,
        config.sound_file.clone(),
        log_tx,
        running,
    );

    use iced::futures::StreamExt;
    // Guard lives inside the stream — drops only when subscription is cancelled.
    log_rx
        .map(move |s| {
            let _hold = &guard;
            Message::LogLine(s)
        })
        .boxed()
}

fn build_tray_stream(
    config: &TrayPollConfig,
) -> iced::futures::stream::BoxStream<'static, Message> {
    let running = Arc::new(AtomicBool::new(true));
    let guard = StopGuard(running.clone());

    let (event_tx, event_rx) = iced::futures::channel::mpsc::unbounded::<TrayAction>();

    tray::start(
        event_tx,
        running,
        config.id_restore.clone(),
        config.id_toggle.clone(),
        config.id_delete.clone(),
        config.id_quit.clone(),
    );

    use iced::futures::StreamExt;
    event_rx
        .map(move |action| {
            let _hold = &guard;
            Message::TrayEvent(action)
        })
        .boxed()
}

fn close_request_subscription() -> Subscription<Message> {
    window::close_requests().map(Message::WindowCloseRequested)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> iced::Result {
    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("dicom-watch"));
    let exe_dir = exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();

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

    let config_path = exe_dir.join("config.toml");

    rust_i18n::set_locale(&config.locale.language);

    iced::application(
        move || {
            // Create system tray on the main thread, after the event
            // loop has started (required by GTK on Linux and winit on
            // Windows).
            if config.tray.enabled {
                #[cfg(target_os = "linux")]
                if let Err(e) = gtk::init() {
                    eprintln!("DicomWatch: gtk::init failed: {e}");
                }

                match tray::build_tray() {
                    Ok((tray, id_restore, id_toggle, id_delete, id_quit)) => {
                        std::mem::forget(tray);
                        let _ = TRAY_IDS.set((id_restore, id_toggle, id_delete, id_quit));
                    }
                    Err(e) => {
                        eprintln!("DicomWatch: tray creation failed: {e}");
                    }
                }
            }

            let filter_mode = config.filter.mode().unwrap_or(FilterMode::Glob);
            AppState {
                exe_dir: exe_dir.clone(),
                config_path: config_path.clone(),
                source_dir: config.directories.source.clone(),
                dest_dir: config.directories.destination.clone(),
                filter_mode,
                filter_pattern: config.filter.pattern.clone(),
                sound_enabled: config.sound.enabled,
                sound_file: config.sound.file.clone(),
                delete_sound_enabled: config.delete_sound.enabled,
                delete_sound_file: config.delete_sound.file.clone(),
                locale: config.locale.language.clone(),
                watching: false,
                tray_enabled: config.tray.enabled,
                log: Vec::new(),
                field_errors: Vec::new(),
            }
        },
        AppState::update,
        AppState::view,
    )
    .window(iced::window::Settings {
        icon: iced::window::icon::from_file_data(include_bytes!("../assets/icon.png"), None).ok(),
        ..Default::default()
    })
    .title(TITLE)
    .subscription(AppState::subscription)
    .window_size((750.0, 700.0))
    .resizable(true)
    .run()
}

// ---------------------------------------------------------------------------
// I18n helpers
// ---------------------------------------------------------------------------

/// Returns the translated value for `key` as an owned `String`.
fn tr(key: &str) -> String {
    t!(key).to_string()
}

// ---------------------------------------------------------------------------
// AppState impl
// ---------------------------------------------------------------------------

impl AppState {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleLocale => {
                let new = if self.locale == "en" { "pt-BR" } else { "en" };
                rust_i18n::set_locale(new);
                self.locale = new.into();
                save_config(self);
                Task::none()
            }

            Message::WatchToggled => {
                if self.watching {
                    self.watching = false;
                } else {
                    self.field_errors = validate_fields(self);
                    if !self.field_errors.is_empty() {
                        return Task::none();
                    }
                    self.watching = true;
                    self.log.push(tr("log.watching_start"));
                }
                Task::none()
            }

            Message::DeleteAll => self.delete_all_files(),

            Message::SourceDirChanged(v) => {
                self.source_dir = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::DestDirChanged(v) => {
                self.dest_dir = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::FilterModeChanged(mode) => {
                self.filter_mode = mode;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::FilterPatternChanged(v) => {
                self.filter_pattern = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::SoundEnabledChanged(v) => {
                self.sound_enabled = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::SoundFileChanged(v) => {
                self.sound_file = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }

            Message::LogLine(line) => {
                self.log.push(line);
                if self.log.len() > 200 {
                    self.log.drain(0..100);
                }
                Task::none()
            }

            Message::BrowseSourceDir => {
                let dir = self.source_dir.clone();
                Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_directory(&dir)
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::SourceDirPicked,
                )
            }
            Message::BrowseDestDir => {
                let dir = self.dest_dir.clone();
                Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_directory(&dir)
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::DestDirPicked,
                )
            }
            Message::BrowseSoundFile => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("Audio", &["ogg", "wav", "mp3", "flac"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::SoundFilePicked,
            ),
            Message::BrowseDeleteSoundFile => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("Audio", &["ogg", "wav", "mp3", "flac", "oga"])
                        .pick_file()
                        .await
                        .map(|h| h.path().to_path_buf())
                },
                Message::DeleteSoundFilePicked,
            ),
            Message::SourceDirPicked(Some(path)) => {
                self.source_dir = path.to_string_lossy().into_owned();
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::SourceDirPicked(None) => Task::none(),
            Message::DestDirPicked(Some(path)) => {
                self.dest_dir = path.to_string_lossy().into_owned();
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::DestDirPicked(None) => Task::none(),
            Message::SoundFilePicked(Some(path)) => {
                self.sound_file = path.to_string_lossy().into_owned();
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::SoundFilePicked(None) => Task::none(),

            Message::DeleteSoundEnabledChanged(v) => {
                self.delete_sound_enabled = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::DeleteSoundFileChanged(v) => {
                self.delete_sound_file = v;
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }

            Message::DeleteSoundFilePicked(Some(path)) => {
                self.delete_sound_file = path.to_string_lossy().into_owned();
                self.field_errors = validate_fields(self);
                save_config(self);
                Task::none()
            }
            Message::DeleteSoundFilePicked(None) => Task::none(),

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
                TrayAction::Click | TrayAction::MenuRestore => iced::window::latest()
                    .and_then(|id| window::set_mode(id, iced::window::Mode::Windowed)),
                TrayAction::MenuToggleWatch => {
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
                TrayAction::MenuDeleteAll => self.delete_all_files(),
                TrayAction::MenuQuit => iced::window::latest().and_then(window::close),
            },
        }
    }

    fn delete_all_files(&mut self) -> Task<Message> {
        let play_sound = if self.delete_sound_enabled {
            let path = resolve_path(&self.delete_sound_file, &self.exe_dir);
            path.exists().then_some(path)
        } else {
            None
        };

        let dest = std::path::Path::new(&self.dest_dir);
        let mut removed = 0u32;
        match std::fs::read_dir(dest) {
            Ok(entries) => {
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
        if removed > 0
            && let Some(ref path) = play_sound
        {
            watcher::play_sound(path);
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        // State — borrowed &str from the Strs held by AppState.
        let status_str = if self.watching {
            "status.watching"
        } else {
            "status.idle"
        };
        let btn_str = if self.watching {
            "button.stop"
        } else {
            "button.start"
        };

        // ---- Status & actions ----
        let status_color = if self.watching {
            iced::Color::from_rgb(0.2, 0.7, 0.2)
        } else {
            iced::Color::from_rgb(0.55, 0.55, 0.55)
        };

        let status_badge = text(tr(status_str))
            .size(13)
            .shaping(Shaping::Advanced)
            .color(status_color);

        let watch_btn = button(text(tr(btn_str)).size(13).shaping(Shaping::Advanced))
            .padding([6, 24])
            .on_press(Message::WatchToggled);

        let delete_btn = button(
            text(tr("button.clear_all"))
                .size(13)
                .shaping(Shaping::Advanced),
        )
        .padding([6, 24])
        .style(button::danger)
        .on_press(Message::DeleteAll);

        let locale_btn = button(
            text(if self.locale == "en" { "EN" } else { "PT-BR" })
                .size(13)
                .shaping(Shaping::Advanced),
        )
        .padding([6, 12])
        .on_press(Message::ToggleLocale);

        let actions_row = row![
            status_badge,
            Space::new().width(12),
            watch_btn,
            Space::new().width(8),
            delete_btn,
            Space::new().width(Length::Fill),
            locale_btn,
        ]
        .align_y(Alignment::Center);

        // ---- Settings ----
        let browse_text = tr("button.browse");

        let mode_pick = pick_list(
            vec![FilterMode::Glob, FilterMode::Regex],
            Some(self.filter_mode),
            Message::FilterModeChanged,
        )
        .text_size(13);

        let settings_card = container(
            column![
                text(tr("field.source_dir"))
                    .size(13)
                    .shaping(Shaping::Advanced),
                row![
                    text_input(&tr("placeholder.source_dir"), &self.source_dir)
                        .on_input(Message::SourceDirChanged)
                        .padding(6)
                        .size(13)
                        .width(Length::Fill),
                    Space::new().width(6),
                    button(
                        text(browse_text.clone())
                            .size(13)
                            .shaping(Shaping::Advanced)
                    )
                    .padding([6, 14])
                    .on_press(Message::BrowseSourceDir),
                ],
                Space::new().height(6),
                text(tr("field.dest_dir"))
                    .size(13)
                    .shaping(Shaping::Advanced),
                row![
                    text_input(&tr("placeholder.dest_dir"), &self.dest_dir)
                        .on_input(Message::DestDirChanged)
                        .padding(6)
                        .size(13)
                        .width(Length::Fill),
                    Space::new().width(6),
                    button(
                        text(browse_text.clone())
                            .size(13)
                            .shaping(Shaping::Advanced)
                    )
                    .padding([6, 14])
                    .on_press(Message::BrowseDestDir),
                ],
                Space::new().height(6),
                row![
                    column![
                        text(tr("field.filter_mode"))
                            .size(13)
                            .shaping(Shaping::Advanced),
                        mode_pick,
                    ],
                    Space::new().width(8),
                    column![
                        text(tr("field.pattern"))
                            .size(13)
                            .shaping(Shaping::Advanced),
                        text_input(&tr("placeholder.filter_pattern"), &self.filter_pattern)
                            .on_input(Message::FilterPatternChanged)
                            .padding(6)
                            .size(13),
                    ]
                    .width(Length::Fill),
                ],
                Space::new().height(6),
                row![
                    toggler(self.sound_enabled)
                        .label(tr("field.sound_alert"))
                        .text_size(13)
                        .on_toggle(Message::SoundEnabledChanged),
                    Space::new().width(8),
                    text_input(&tr("placeholder.sound_file"), &self.sound_file)
                        .on_input(Message::SoundFileChanged)
                        .padding(6)
                        .size(13)
                        .width(Length::Fill),
                    Space::new().width(6),
                    button(
                        text(browse_text.clone())
                            .size(13)
                            .shaping(Shaping::Advanced)
                    )
                    .padding([6, 14])
                    .on_press(Message::BrowseSoundFile),
                ]
                .align_y(Alignment::Center),
                Space::new().height(6),
                row![
                    toggler(self.delete_sound_enabled)
                        .label(tr("field.delete_sound_alert"))
                        .text_size(13)
                        .on_toggle(Message::DeleteSoundEnabledChanged),
                    Space::new().width(8),
                    text_input(&tr("placeholder.sound_file"), &self.delete_sound_file)
                        .on_input(Message::DeleteSoundFileChanged)
                        .padding(6)
                        .size(13)
                        .width(Length::Fill),
                    Space::new().width(6),
                    button(
                        text(browse_text.clone())
                            .size(13)
                            .shaping(Shaping::Advanced)
                    )
                    .padding([6, 14])
                    .on_press(Message::BrowseDeleteSoundFile),
                ]
                .align_y(Alignment::Center),
                Space::new().height(6),
                row![
                    toggler(self.tray_enabled)
                        .label(tr("tray.setting"))
                        .text_size(13)
                        .on_toggle(Message::TrayEnabledChanged),
                ],
            ]
            .spacing(0),
        )
        .padding(14)
        .style(container_mod::bordered_box);

        // ---- Field errors ----
        let errors_list: Element<_> = if self.field_errors.is_empty() {
            Space::new().height(0).into()
        } else {
            let items: Vec<Element<_>> = self
                .field_errors
                .iter()
                .map(|e| {
                    text(e)
                        .size(13)
                        .shaping(Shaping::Advanced)
                        .color(iced::Color::from_rgb(0.9, 0.2, 0.2))
                        .into()
                })
                .collect();
            column(items).spacing(2).into()
        };

        // ---- Activity Log (terminal style) ----
        let log_card = container(scrollable(
            column(if self.log.is_empty() {
                vec![
                    text(tr("log.waiting"))
                        .size(13)
                        .shaping(Shaping::Advanced)
                        .color(iced::Color::from_rgb(0.5, 0.5, 0.5))
                        .into(),
                ]
            } else {
                let start = self.log.len().saturating_sub(200);
                self.log[start..]
                    .iter()
                    .map(|line| {
                        text(line)
                            .size(13)
                            .shaping(Shaping::Advanced)
                            .color(iced::Color::from_rgb(0.8, 0.9, 0.8))
                            .into()
                    })
                    .collect()
            })
            .spacing(1)
            .padding(10)
            .width(Length::Fill),
        ))
        .padding(0)
        .style(terminal_style);

        // ---- Layout ----
        container(
            column![
                actions_row,
                Space::new().height(6),
                errors_list,
                Space::new().height(8),
                text(tr("section.settings"))
                    .size(14)
                    .font(BOLD)
                    .shaping(Shaping::Advanced),
                Space::new().height(6),
                settings_card,
                Space::new().height(10),
                text(tr("section.activity_log"))
                    .size(14)
                    .font(BOLD)
                    .shaping(Shaping::Advanced),
                Space::new().height(6),
                log_card.height(Length::Fill),
            ]
            .padding(20)
            .spacing(0),
        )
        .style(app_bg_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = Vec::new();

        if self.watching {
            let config = WatcherConfig {
                source_dir: PathBuf::from(&self.source_dir),
                dest_dir: PathBuf::from(&self.dest_dir),
                filter_mode_str: self.filter_mode.to_string(),
                pattern: self.filter_pattern.clone(),
                sound_enabled: self.sound_enabled,
                sound_file: resolve_path(&self.sound_file, &self.exe_dir),
            };
            subs.push(iced::Subscription::run_with(config, build_watcher_stream));
        }

        if self.tray_enabled
            && let Some(ids) = TRAY_IDS.get()
        {
            subs.push(iced::Subscription::run_with(
                TrayPollConfig {
                    id_restore: ids.0.clone(),
                    id_toggle: ids.1.clone(),
                    id_delete: ids.2.clone(),
                    id_quit: ids.3.clone(),
                },
                build_tray_stream,
            ));
        }

        subs.push(close_request_subscription());

        Subscription::batch(subs)
    }
}

// ---------------------------------------------------------------------------
// Custom styles
// ---------------------------------------------------------------------------

fn app_bg_style(_theme: &iced::Theme) -> container_mod::Style {
    container_mod::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb(
            0.94, 0.94, 0.96,
        ))),
        ..container_mod::Style::default()
    }
}

fn terminal_style(_theme: &iced::Theme) -> container_mod::Style {
    container_mod::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb(
            0.12, 0.13, 0.15,
        ))),
        border: iced::Border {
            radius: 6.0.into(),
            width: 1.0,
            color: iced::Color::from_rgb(0.25, 0.25, 0.28),
        },
        ..container_mod::Style::default()
    }
}

// ---------------------------------------------------------------------------
// FilterMode traits
// ---------------------------------------------------------------------------

impl std::fmt::Display for FilterMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterMode::Glob => write!(f, "glob"),
            FilterMode::Regex => write!(f, "regex"),
        }
    }
}
