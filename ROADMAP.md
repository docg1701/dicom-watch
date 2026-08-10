# Roadmap

## v0.5.0 — i18n: English / Português (Brasil)

- [ ] Toggle in the UI to switch between English and pt-BR without restart
- [ ] All UI strings externalized into locale files (JSON or TOML)
- [ ] Labels, buttons, log messages, error messages — all translated
- [ ] Iced runtime locale detection, no config file flag needed
- [ ] Switch is instant — no reload, no recompilation

## v0.6.0 — Windows support

- [ ] App compiles and runs on Windows 7, 10, and 11
- [ ] `cargo build --release` produces a `.exe` (Windows) and a Linux binary
- [ ] CI builds and tests both targets
- [ ] Release zip includes `dicom-watch.exe` + `config.toml.example` for Windows
- [ ] Documentation (README, regex-guide, AGENTS.md) covers both Linux and Windows:
  - [ ] Install steps for each OS
  - [ ] Sound playback: `paplay`/`aplay` on Linux, `PlaySound` or default shell on Windows
  - [ ] Path conventions (`\` vs `/`)
  - [ ] File watcher differences (notify crate handles this, but document known quirks)
- [ ] Windows binary tested on a real Windows machine (not Wine)

## v0.7.0 — Application icon

- [ ] High-resolution icon sourced from the Obsidian icon set (already on this machine)
- [ ] Icon placed in `assets/` and embedded at compile time
- [ ] Works on:
  - [ ] Linux: Cinnamon, GNOME, KDE — appears in title bar, taskbar, alt-tab, app menu
  - [ ] Windows: taskbar, title bar, Start menu shortcuts, Alt+Tab
- [ ] Multiple resolutions: 16×16, 32×32, 48×48, 256×256 (`.png` or `.ico`)
- [ ] `.desktop` file for Linux (so it shows in app launchers with the icon)
- [ ] Windows: icon embedded in the `.exe` via `winres`
