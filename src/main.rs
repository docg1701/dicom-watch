mod config;
mod watcher;

use config::{resolve_path, Config, FilterMode};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, toggler, Space,
};
use iced::{Alignment, Element, Length, Subscription};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

    watching: bool,
    watcher_stop_flag: Option<Arc<AtomicBool>>,

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
    LogLine(String),
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
// Stop guard
// ---------------------------------------------------------------------------

struct StopGuard(Arc<AtomicBool>);

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
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
        errors.push(format!(
            "Source directory does not exist: {}",
            state.source_dir
        ));
    }
    let dest = std::path::Path::new(&state.dest_dir);
    if !dest.exists() || !dest.is_dir() {
        errors.push(format!(
            "Destination directory does not exist: {}",
            state.dest_dir
        ));
    }
    if state.filter_mode == FilterMode::Regex {
        if let Err(e) = regex::Regex::new(&state.filter_pattern) {
            errors.push(format!("Invalid regex: {}", e));
        }
    }
    if state.sound_enabled {
        let sound_path = resolve_path(&state.sound_file, &state.exe_dir);
        if !sound_path.exists() {
            errors.push(format!("Sound file not found: {}", sound_path.display()));
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// Stream builder (fn ptr — no captures)
// ---------------------------------------------------------------------------

fn build_watcher_stream(config: &WatcherConfig) -> impl iced::futures::Stream<Item = Message> {
    let running = Arc::new(AtomicBool::new(true));
    let _guard = StopGuard(running.clone());

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

    iced::futures::StreamExt::map(log_rx, Message::LogLine)
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
            std::process::exit(1);
        }
    };

    let config_path = exe_dir.join("config.toml");

    iced::application(
        move || {
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
                watching: false,
                watcher_stop_flag: None,
                log: Vec::new(),
                field_errors: Vec::new(),
            }
        },
        AppState::update,
        AppState::view,
    )
    .title("DicomWatch")
    .subscription(AppState::subscription)
    .window_size((750.0, 560.0))
    .resizable(true)
    .run()
}

// ---------------------------------------------------------------------------
// AppState impl
// ---------------------------------------------------------------------------

impl AppState {
    fn update(&mut self, message: Message) {
        match message {
            Message::WatchToggled => {
                if self.watching {
                    if let Some(flag) = self.watcher_stop_flag.take() {
                        flag.store(false, Ordering::SeqCst);
                    }
                    self.watching = false;
                } else {
                    self.field_errors = validate_fields(self);
                    if !self.field_errors.is_empty() {
                        return;
                    }
                    self.watching = true;
                    self.log.push("[Watch] Starting...".into());
                }
            }

            Message::DeleteAll => {
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
            }

            Message::SourceDirChanged(v) => {
                self.source_dir = v;
                self.field_errors = validate_fields(self);
                save_config(self);
            }
            Message::DestDirChanged(v) => {
                self.dest_dir = v;
                self.field_errors = validate_fields(self);
                save_config(self);
            }
            Message::FilterModeChanged(mode) => {
                self.filter_mode = mode;
                self.field_errors = validate_fields(self);
                save_config(self);
            }
            Message::FilterPatternChanged(v) => {
                self.filter_pattern = v;
                self.field_errors = validate_fields(self);
                save_config(self);
            }
            Message::SoundEnabledChanged(v) => {
                self.sound_enabled = v;
                self.field_errors = validate_fields(self);
                save_config(self);
            }
            Message::SoundFileChanged(v) => {
                self.sound_file = v;
                self.field_errors = validate_fields(self);
                save_config(self);
            }

            Message::LogLine(line) => {
                self.log.push(line);
                if self.log.len() > 1000 {
                    self.log.drain(0..500);
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let status_icon = if self.watching { "●" } else { "○" };
        let status_text = if self.watching { "Watching" } else { "Stopped" };
        let status_color = if self.watching {
            iced::Color::from_rgb(0.2, 0.8, 0.2)
        } else {
            iced::Color::from_rgb(0.6, 0.6, 0.6)
        };

        let status_row = row![
            text(status_icon).size(16).color(status_color),
            Space::new().width(6),
            text(status_text).size(14),
        ]
        .align_y(Alignment::Center);

        let watch_btn_label = if self.watching { "⏹ Stop" } else { "▶ Watch" };
        let watch_btn = button(text(watch_btn_label).size(14))
            .padding([8, 20])
            .on_press(Message::WatchToggled);

        let delete_btn = button(text("🗑 Delete All").size(14))
            .padding([8, 20])
            .style(button::danger)
            .on_press(Message::DeleteAll);

        let buttons_row =
            row![watch_btn, Space::new().width(10), delete_btn]
                .align_y(Alignment::Center);

        // ---- Settings ----
        let mode_pick = pick_list(
            vec![FilterMode::Glob, FilterMode::Regex],
            Some(self.filter_mode),
            Message::FilterModeChanged,
        );

        let settings_section = column![
            text("Settings").size(16),
            Space::new().height(8),
            text("Source directory:").size(12),
            text_input("", &self.source_dir)
                .on_input(Message::SourceDirChanged)
                .padding(4)
                .size(13),
            Space::new().height(6),
            text("Destination directory:").size(12),
            text_input("", &self.dest_dir)
                .on_input(Message::DestDirChanged)
                .padding(4)
                .size(13),
            Space::new().height(6),
            row![
                column![text("Filter mode:").size(12), mode_pick,].width(Length::FillPortion(1)),
                Space::new().width(10),
                column![
                    text("Pattern:").size(12),
                    text_input("", &self.filter_pattern)
                        .on_input(Message::FilterPatternChanged)
                        .padding(4)
                        .size(13),
                ]
                .width(Length::FillPortion(3)),
            ],
            Space::new().height(6),
            row![
                toggler(self.sound_enabled)
                    .label("Sound")
                    .on_toggle(Message::SoundEnabledChanged),
                Space::new().width(10),
                text_input("Sound file path", &self.sound_file)
                    .on_input(Message::SoundFileChanged)
                    .padding(4)
                    .size(13)
                    .width(Length::Fill),
            ]
            .align_y(Alignment::Center),
        ]
        .spacing(2);

        // ---- Field errors ----
        let errors_list: Element<_> = if self.field_errors.is_empty() {
            Space::new().width(0).into()
        } else {
            let items: Vec<Element<_>> = self
                .field_errors
                .iter()
                .map(|e| {
                    text(e)
                        .size(12)
                        .color(iced::Color::from_rgb(0.9, 0.2, 0.2))
                        .into()
                })
                .collect();
            column(items).spacing(2).into()
        };

        // ---- Log ----
        let log_lines: Vec<Element<_>> = self
            .log
            .iter()
            .rev()
            .take(200)
            .map(|line| text(line).size(12).into())
            .collect();

        let log_area = scrollable(
            column(log_lines)
                .spacing(1)
                .padding(6)
                .width(Length::Fill),
        )
        .height(Length::Fill);

        let log_header = text("Activity Log").size(16);

        // ---- Layout ----
        let left_panel = column![
            status_row,
            Space::new().height(10),
            buttons_row,
            Space::new().height(14),
            errors_list,
            Space::new().height(10),
            settings_section,
        ]
        .width(Length::FillPortion(2))
        .spacing(4);

        let right_panel =
            column![log_header, Space::new().height(6), log_area]
                .width(Length::FillPortion(3))
                .spacing(4)
                .height(Length::Fill);

        container(
            row![
                left_panel,
                Space::new().width(16),
                right_panel
            ]
            .padding(16)
            .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        if !self.watching {
            return Subscription::none();
        }

        let config = WatcherConfig {
            source_dir: PathBuf::from(&self.source_dir),
            dest_dir: PathBuf::from(&self.dest_dir),
            filter_mode_str: self.filter_mode.to_string(),
            pattern: self.filter_pattern.clone(),
            sound_enabled: self.sound_enabled,
            sound_file: resolve_path(&self.sound_file, &self.exe_dir),
        };

        iced::Subscription::run_with(config, build_watcher_stream)
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

impl FilterMode {
    fn to_string(&self) -> String {
        match self {
            FilterMode::Glob => "glob".into(),
            FilterMode::Regex => "regex".into(),
        }
    }
}
