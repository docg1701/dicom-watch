# AGENTS.md — dicom-watch

> Agent instruction set. Keep ≤200 lines. Update in the same PR that changes conventions.

## Agent behavior — documentation freshness

- **Before any development, bugfix, or review cycle**, run `SKILL:find-docs` to
  refresh the agent's knowledge of the libraries and tools involved in that
  cycle. Do not rely on training-data memory alone.
- **If a code-related operation fails twice in a row**, stop and run
  `SKILL:find-docs` before the next attempt. Do not retry blindly.
- **If the agent is less than 90% certain** it can write the code from training
  memory alone, run `SKILL:find-docs` first.
- **When analyzing code**, account for the difference between the repository's
  current (latest stable) version of each dependency and the version that was
  current at the model's training cutoff. Prefer the repository's actual
  version.

## Commands

```bash
# Development (Linux or Windows)
cargo build                             # debug build
cargo run                               # debug build + run (needs config.toml next to binary)

# Lint & format (LOCAL — run before every push)
cargo fmt                               # auto-format
cargo fmt --check                       # verify formatting
cargo clippy                            # lint (warnings)
cargo clippy -- -D warnings             # strict lint — all warnings = errors

# Test (LOCAL)
cargo test                              # all tests
cargo test -- --nocapture               # with stdout visible

# Release (LOCAL — binary compiled here, uploaded to existing GitHub Release)
cargo build --release                   # optimized binary at target/release/dicom-watch
./scripts/release.sh vX.Y.Z             # Linux: package + upload to GitHub Release (requires gh CLI)
```

### Windows-specific

Cross-compile from Linux or build on a Windows machine:

```bash
# From Linux (requires mingw-w64 + Rust target):
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# Binary at target/x86_64-pc-windows-gnu/release/dicom-watch.exe

# Or on Windows:
cargo build --release
# Binary at target\release\dicom-watch.exe
```

Package and upload (Linux):
```bash
./scripts/release-windows.sh vX.Y.Z
```

Manual packaging (Windows):
```powershell
# Create zip containing dicom-watch.exe + config.toml.example + alarm.wav
# Upload via gh release upload vX.Y.Z dicom-watch-vX.Y.Z-windows-x86_64.zip
```

## Full development & release routine

### Day-to-day (everything local)

```
edit → cargo fmt → cargo clippy -- -D warnings → cargo test → commit → push → PR → merge
                                                                              │
                                                                   CI: cargo fmt --check only
```

CI never compiles. clippy and test are LOCAL.

### Cutting a release

```
1. Bump version in Cargo.toml
2. cargo build                     # sync Cargo.lock
3. Commit: fix: version bump X.Y.Z -> A.B.C (context)
4. PR → merge to master
5. git tag vA.B.C <merge-commit-sha>   # LIGHTWEIGHT — NOT -a, NOT -m
6. git push origin vA.B.C
7. CI fires: fmt check passes → release job auto-creates GitHub Release with categorized changelog
8. LOCAL: cargo build --release
9. LOCAL: ./scripts/release.sh vA.B.C          # Linux zip → upload
10. LOCAL: ./scripts/release-windows.sh vA.B.C  # Windows zip → upload (cross-compile or VM)
```

**Steps 8-10 are MANDATORY.** Every release MUST have both
`dicom-watch-vX.Y.Z-linux-x86_64.zip` and `dicom-watch-vX.Y.Z-windows-x86_64.zip`
as GitHub Release assets. A release is incomplete until both are uploaded.

Steps 8-10 run on your Linux Mint machine. Step 7 is fully automatic — the
release is created with changelog only (no binaries).

Linux zip contains: `dicom-watch` + `install.sh` + `uninstall.sh` + `icon.png` + `config.toml.example` + `alarm.wav` + `delete.wav`.
Windows zip contains: `dicom-watch.exe` + `config.toml.example` + `alarm.wav`.

## Project structure

```
src/
  main.rs      — Iced GUI: AppState, Message, update(), view(), subscription()
  watcher.rs   — Background thread: notify watcher, zip extraction, sound
  config.rs    — Config load/validate from config.toml, FilterMode, path resolution
locales/
  en.toml      — English translations
  pt-BR.toml   — Brazilian Portuguese translations
scripts/
  release.sh   — Local build + package + upload to GitHub Release (Linux)
  install.sh   — Linux: installs .desktop entry + icon to application menu
config.toml.example  — Documented template; user copies to config.toml
docs/
  PRD.md             — Product requirements (v0.5.0 - v0.7.0)
  ROADMAP.md         — Future milestones
  regex-guide.md     — User-facing regex documentation
build.rs             — Windows: embeds icon.ico into .exe via winres
assets/
  icon.png           — App icon (256×256, embedded at compile time)
  icon.ico           — Multi-resolution Windows icon (16/32/48/256)
```

`main.rs` owns the UI state machine. `watcher.rs` owns filesystem I/O.
`config.rs` owns serialization. No module reaches into another's internals.

## Code style

**Functions ≤ 30 lines.** Extract helpers. `watcher::start()` is the exception —
one long-lived thread spawn, acceptable.

**Thread communication: channels, never shared mutable state.**
```rust
// Good
let (tx, rx) = iced::futures::channel::mpsc::unbounded();
watcher::start(src, dst, mode, pat, sound, file, tx, running);
StreamExt::map(rx, Message::LogLine)

// Bad
Arc<Mutex<Vec<String>>> polled in the UI
```

**Error handling: validate early, fail loud.**
```rust
// Good — crash at startup with exact field name
let config = config::load_config(&exe_dir)?;

// Bad — silently use defaults for missing config
let source = config.directories.source.unwrap_or("/tmp");
```

## Testing (LOCAL ONLY)

- `#[cfg(test)] mod tests` inline at the bottom of each source file
- Naming: `test_<function>_<scenario>`
- Test: config validation paths, regex compilation, path resolution
- Do NOT test: GUI layout (Iced widgets), sound playback, notify events
- `cargo test` runs locally before push

## Git workflow

- Branch: `feature/<name>`, `fix/<name>` from `master`
- Commits: conventional — `feat:`, `fix:`, `chore:`, `docs:`, `ci:`
- PR required to merge to `master`
- Tag: **lightweight** only — `git tag vX.Y.Z <sha>` (never `-a`, never `-m`)

## CI (`.github/workflows/ci.yml`)

| Trigger | What runs |
|---------|-----------|
| Push / PR to `master` | `cargo fmt --check` (zero compilation) |
| Push tag `v*.*.*` | `cargo fmt --check`, then `release` job auto-creates GitHub Release |

CI never compiles. Never runs clippy. Never runs tests. All of that is local.
Linting and testing are mandatory locally before every push:

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Release binary is built locally (`cargo build --release`) and uploaded via
`scripts/release.sh`.

## Version — single source of truth

`Cargo.toml` → `version` field. `Cargo.lock` syncs on `cargo build`.
Never edit version numbers anywhere else.

## Boundaries

### ✅ Always (LOCAL — before push)
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- Commit `Cargo.lock` together with `Cargo.toml` changes
- Validate config at startup — crash with a clear message, never default-fill
- Log errors with context (file path, pattern, error message)
- **Every release MUST have both Linux AND Windows zips uploaded to GitHub Release assets**

### ⚠️ Ask first
- Adding a dependency to `Cargo.toml`
- Changing config.toml format (breaks existing user configs)
- Adding CLI arguments beyond what config.toml covers
- Changing MSRV (currently Rust 1.91)

### 🚫 Never
- Hardcode personal paths (`/home/galvani/...`) in source or examples
- `unwrap()` on user input or filesystem operations — use `match` or `.map_err()`
- Silence errors with `let _ = ...` without a comment explaining why
- `git tag -a` / `git tag -m` — annotated tags produce duplicated release titles
- `gh release create` / `gh release edit` manually — CI owns release creation
- Force-push a tag after CI created the release
- Commit `config.toml` — gitignored; contains user paths
