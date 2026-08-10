# DicomWatch

A Linux desktop app that watches a directory for new zip files, extracts them
to a destination folder, and notifies you — so you can focus on reading
studies instead of managing files.

## How it works

1. Drop a .zip file in your **source** directory (e.g., `~/Downloads`).
2. DicomWatch detects it, extracts the contents to the **destination**
   directory, deletes the original zip, and plays a sound.
3. Open the extracted DICOM files in [Weasis](https://nroduit.github.io/) (or
   any DICOM viewer).
4. When done, click **Delete All** to clean the destination folder.

## Requirements

- Linux (tested on Linux Mint)
- `paplay` (from `pulseaudio-utils`) or `aplay` (from `alsa-utils`) for sound
  — almost always pre-installed.
- Nothing else. The binary is self-contained.

## Installation

1. Download the `dicom-watch` binary from the [releases page](../../releases).
2. Place it in a directory of your choice, e.g.:
   ```bash
   mkdir -p ~/dicomwatch
   mv dicom-watch ~/dicomwatch/
   cd ~/dicomwatch
   chmod +x dicom-watch
   ```
3. Copy the example config and edit it:
   ```bash
   cp /path/to/config.toml.example config.toml
   nano config.toml
   ```
4. Run:
   ```bash
   ./dicom-watch
   ```

## Configuration

DicomWatch reads `config.toml` from the same directory as the binary. If the
file is missing or has errors, DicomWatch prints an error message and exits.

See [`config.toml.example`](config.toml.example) for a documented template.

### Structure

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

- **source**: directory to watch for new files.
- **destination**: directory where zip contents are extracted.
- **filter.mode**: `"glob"` for simple wildcards or `"regex"` for regular
  expressions.
- **filter.pattern**: filename pattern to match.
- **sound.enabled**: `true` or `false`.
- **sound.file**: path to an audio file (OGG, WAV, MP3).

For regex mode, see [`regex-guide.md`](regex-guide.md).

## Building from source

```bash
# Prerequisites
sudo apt install libx11-dev libwayland-dev libxkbcommon-dev

# Build
cargo build --release

# The binary is at target/release/dicom-watch
```

## License

MIT
