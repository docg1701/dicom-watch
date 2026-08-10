# DicomWatch

A Linux desktop app that watches a directory for new zip files, extracts them,
and notifies you — so you focus on reading studies, not managing files.

## What it is

DicomWatch monitors a source directory for incoming zip archives (DICOM studies),
auto-extracts them to a destination folder, deletes the original archive, and
plays a notification sound. Built with Iced for a native GTK-like GUI on X11
and Wayland. Single binary, no runtime dependencies beyond what your Linux
desktop already has.

## Install

1. Download `dicom-watch` from the [releases page](../../releases).
2. Place it in its own directory and make it executable:
   ```bash
   mkdir -p ~/dicomwatch
   mv dicom-watch ~/dicomwatch/
   cd ~/dicomwatch
   chmod +x dicom-watch
   ```
3. Copy and edit the configuration:
   ```bash
   cp /path/to/config.toml.example config.toml
   nano config.toml
   ```
4. Run:
   ```bash
   ./dicom-watch
   ```

## Usage

```bash
./dicom-watch                          # start the GUI
cargo run                              # debug build + run
cargo build --release                  # optimized binary at target/release/
cargo test                             # run tests
cargo fmt --check && cargo clippy      # lint check
```

### Typical workflow

1. Drop a `.zip` file in your source directory (e.g. `~/Downloads`).
2. DicomWatch detects it, extracts contents, removes the zip, plays a sound.
3. Open the extracted DICOMs in [Weasis](https://nroduit.github.io/).
4. Click **Delete All** to clean the destination folder when done.

## Configuration

`config.toml` lives next to the binary. See [`config.toml.example`](config.toml.example)
for a documented template.

```toml
[directories]
source = "/home/user/Downloads"
destination = "/home/user/dicom/studies"

[filter]
mode = "glob"          # or "regex"
pattern = "*.zip"

[sound]
enabled = true
file = "/home/user/sounds/notification.ogg"
```

Validation errors at startup are explicit — missing directories, invalid regex,
sound file not found — pointing to the exact field to fix.

For regex mode, see [`regex-guide.md`](regex-guide.md).

## Build from source

```bash
# System dependencies (Debian/Ubuntu)
sudo apt install libx11-dev libwayland-dev libxkbcommon-dev

# Build
cargo build --release
```

## License

MIT
