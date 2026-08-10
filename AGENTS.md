# AGENTS.md — dicom-watch

> Agent instruction set. Keep ≤200 lines. Update in the same PR that changes conventions.

## Commands

```bash
# Development
cargo build                             # debug build
cargo run                               # debug build + run (needs config.toml next to binary)

# Lint & format (must pass before push)
cargo fmt                               # auto-format
cargo fmt --check                       # check formatting (CI)
cargo clippy                            # lint (warnings)
cargo clippy -- -D warnings             # strict lint — all warnings = errors (CI)

# Test
cargo test                              # all tests
cargo test -- --nocapture               # with stdout visible

# Release (LOCAL — binary compiled here, uploaded to existing GitHub Release)
cargo build --release                   # optimized binary at target/release/dicom-watch
./scripts/release.sh vX.Y.Z             # package + upload to GitHub Release (requires gh CLI)
```

## Full development & release routine

### Day-to-day

```
edit → cargo fmt → cargo clippy → cargo test → commit → push → PR → merge
                                                                    │
                                                          CI: fmt + clippy + test
```

### Cutting a release

```
1. Bump version in Cargo.toml
2. cargo build                     # sync Cargo.lock
3. Commit: fix: version bump X.Y.Z -> A.B.C (context)
4. PR → merge to main
5. git tag vA.B.C <merge-commit-sha>   # LIGHTWEIGHT — NOT -a, NOT -m
6. git push origin vA.B.C
7. CI fires: test → release job creates GitHub Release with categorized changelog
8. LOCAL: cargo build --release
9. LOCAL: ./scripts/release.sh vA.B.C  # packages zip + uploads to the release
```

Steps 8-9 happen on your Linux Mint machine (the release binary must be built
on the target OS). Step 7 creates the release with changelog only (no binary).
The zip contains `dicom-watch` + `config.toml.example`.

## Project structure

```
src/
  main.rs      — Iced GUI: AppState, Message, update(), view(), subscription()
  watcher.rs   — Background thread: notify watcher, zip extraction, sound
  config.rs    — Config load/validate from config.toml, FilterMode, path resolution
scripts/
  release.sh   — Local build + package + upload to GitHub Release
config.toml.example  — Documented template; user copies to config.toml
prd.md               — Product requirements (human reference)
regex-guide.md       — User-facing regex documentation
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

## Testing

- `#[cfg(test)] mod tests` inline at the bottom of each source file
- Naming: `test_<function>_<scenario>`
- Test: config validation paths, regex compilation, path resolution
- Do NOT test: GUI layout (Iced widgets), sound playback, notify events
- CI runs `cargo test` on Rust stable, Ubuntu latest

## Git workflow

- Branch: `feature/<name>`, `fix/<name>` from `main`
- Commits: conventional — `feat:`, `fix:`, `chore:`, `docs:`, `ci:`
- PR required to merge to `main`
- Tag: **lightweight** only — `git tag vX.Y.Z <sha>` (never `-a`, never `-m`)

## CI (`.github/workflows/ci.yml`)

| Trigger | What runs |
|---------|-----------|
| Push / PR to `main` | `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test` |
| Push tag `v*.*.*` | Same tests, then `release` job creates GitHub Release with auto-generated changelog grouped by `feat:`/`fix:`/other |

The CI never compiles a release binary. Release binaries are built locally
(`cargo build --release`) and uploaded via `scripts/release.sh`.

## Version — single source of truth

`Cargo.toml` → `version` field. `Cargo.lock` syncs on `cargo build`.
Never edit version numbers anywhere else.

## Boundaries

### ✅ Always
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` before pushing
- Commit `Cargo.lock` together with `Cargo.toml` changes
- Validate config at startup — crash with a clear message, never default-fill
- Log errors with context (file path, pattern, error message)

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
