# DicomWatch — PRD

## Overview

A Linux desktop application (single binary) that monitors a source directory for
newly created archive files (.zip), extracts them to a destination directory,
removes the original archive, and plays a notification sound. The graphical
interface (Iced) provides watch start/stop, destination cleanup, and a verbose
event log.

## Functional Requirements

| ID  | Description |
|-----|-------------|
| F01 | Monitor a source directory for new files |
| F02 | Filter by glob pattern (`*.zip`) or regex |
| F03 | Extract .zip archives to destination directory |
| F04 | Remove original archive after successful extraction |
| F05 | Play a configurable sound file on extraction complete |
| F06 | Watch button: toggle monitoring on/off |
| F07 | Delete button: remove all contents from destination directory |
| F08 | Verbose log with timestamps visible in the UI |
| F09 | Configuration via config.toml next to the binary |
| F10 | If config.toml is missing or invalid, show a friendly error and exit |
| F11 | Editable fields in the UI to change settings at runtime |
| F12 | Save config.toml to disk when settings are modified in the UI |

## Non-Functional Requirements

| ID  | Description |
|-----|-------------|
| NF01 | Linux only (x86_64-unknown-linux-gnu) |
| NF02 | Single static binary, no external runtime dependencies |
| NF03 | No hardcoded personal paths in source or config |
| NF04 | Beginner-friendly regex documentation |
| NF05 | Binary at ~/dicomwatch/ with config.toml alongside |

## Tech Stack

| Component     | Crate      | Version |
|---------------|------------|---------|
| GUI           | iced       | 0.14.0  |
| File watcher  | notify     | 8.2.0   |
| Config        | toml       | 0.8     |
| ZIP extraction| zip        | 2       |
| Sound         | rodio      | 0.22    |
| Pattern match | glob, regex| 0.3, 1  |
| Log timestamp | chrono     | 0.4     |

MSRV: Rust 1.91 (current system). Iced 0.14 MSRV is 1.88 — compatible.

## Architecture

```
main.rs ──► Config (config.rs) ── loads config.toml
    │
    ├── Iced Application (functional API)
    │   ├── State: AppState { config, watching, log, ... }
    │   ├── update(): processes messages (Watch, Delete, ConfigChange...)
    │   ├── view(): renders UI (buttons, fields, log area)
    │   └── subscription(): when watching=true, spawns background watcher
    │
    └── Watcher (watcher.rs) ── runs in background thread
        ├── notify::RecommendedWatcher monitors source directory
        ├── CREATE event → match filter → extract ZIP → play sound → log
        └── Sends log messages via channel to the UI
```

## Usage Flow

1. User downloads `dicom-watch` binary and places it in `~/dicomwatch/`
2. User copies `config.toml.example` → `config.toml` and edits paths
3. User runs `./dicom-watch`
4. App validates config.toml. If OK, opens the window.
5. User clicks **Watch** → monitoring starts
6. A .zip file appears in the source directory → extracted → sound plays
7. Log shows: `[14:23:01] New file: exam_123.zip → Extracted to /home/.../dicom/studies`
8. User opens DICOMs in Weasis (manually)
9. User clicks **Delete** → destination directory is cleaned
10. User clicks **Stop** → monitoring stops

## config.toml Format

```toml
[directories]
source = "/home/user/Downloads"
destination = "/home/user/dicom/studies"

[filter]
# "glob" or "regex"
mode = "glob"
pattern = "*.zip"

[sound]
enabled = true
file = "/home/user/sounds/notification.ogg"
```

Validation rules:
- `directories.source` must exist and be a directory
- `directories.destination` must exist and be a directory
- `filter.mode` must be `"glob"` or `"regex"`
- If `filter.mode = "regex"`, `filter.pattern` must be valid Rust regex syntax
- If `sound.enabled = true`, `sound.file` must exist
- Paths can be absolute or relative (relative to the binary's directory)
- If config.toml is missing: error showing expected path
- If TOML syntax is invalid: error with approximate line/column
- If validation fails: specific error naming the problematic field

## Implementation Priorities

1. Config loading + validation (test in CLI mode first)
2. Iced UI with buttons and log area
3. Watcher + extraction + sound
4. Editable settings fields + auto-save to config.toml
5. Documentation (README, regex-guide, config.toml.example)
6. Release build with LTO + strip

## Success Criteria

- [ ] Binary compiles and runs on Linux Mint without errors
- [ ] Monitors directory and extracts .zip automatically
- [ ] Sound plays after extraction
- [ ] Log is visible and verbose in the UI
- [ ] Delete button cleans destination directory
- [ ] config.toml errors are clear and point to the problem
- [ ] Release binary < 20MB
