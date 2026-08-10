use crate::config::FilterMode;
use chrono::Local;
use notify::{EventKind, RecursiveMode, Watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Start a background watcher on `source_dir` that filters files matching
/// `pattern` (glob or regex), extracts zip archives to `dest_dir`, plays
/// `sound_file` on success, and pushes status lines to `event_sender`.
///
/// Pass `running` to signal stop; the watcher loop checks this flag.
#[allow(clippy::too_many_arguments)]
pub fn start(
    source_dir: PathBuf,
    dest_dir: PathBuf,
    filter_mode: FilterMode,
    pattern: String,
    sound_enabled: bool,
    sound_file: PathBuf,
    log_sender: futures::channel::mpsc::UnboundedSender<String>,
    running: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let log = |msg: &str| {
            let ts = Local::now().format("%H:%M:%S");
            let line = format!("[{}] {}", ts, msg);
            let _ = log_sender.unbounded_send(line);
        };

        // Compile the filter once.
        let matcher: Box<dyn Fn(&str) -> bool + Send> = match filter_mode {
            FilterMode::Glob => {
                let pat = match glob::Pattern::new(&pattern) {
                    Ok(p) => p,
                    Err(e) => {
                        log(&format!("Invalid glob pattern '{}': {}", pattern, e));
                        return;
                    }
                };
                Box::new(move |name: &str| pat.matches(name))
            }
            FilterMode::Regex => {
                let re = match regex::Regex::new(&pattern) {
                    Ok(r) => r,
                    Err(e) => {
                        log(&format!("Invalid regex pattern '{}': {}", pattern, e));
                        return;
                    }
                };
                Box::new(move |name: &str| re.is_match(name))
            }
        };

        let (notify_tx, notify_rx) = std::sync::mpsc::channel();

        let mut watcher = match notify::recommended_watcher(move |event| {
            let _ = notify_tx.send(event);
        }) {
            Ok(w) => w,
            Err(e) => {
                log(&format!("Failed to create file watcher: {}", e));
                return;
            }
        };

        if let Err(e) = watcher.watch(&source_dir, RecursiveMode::NonRecursive) {
            log(&format!(
                "Failed to watch directory '{}': {}",
                source_dir.display(),
                e
            ));
            return;
        }

        log(&format!(
            "Watching '{}' -> '{}' ({}: {})",
            source_dir.display(),
            dest_dir.display(),
            match filter_mode {
                FilterMode::Glob => "glob",
                FilterMode::Regex => "regex",
            },
            pattern
        ));

        // Main loop: receive filesystem events and process.
        while running.load(Ordering::Relaxed) {
            let event = match notify_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => event,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };

            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    log(&format!("Watch error: {}", e));
                    continue;
                }
            };

            // We only care about file creation events.
            let path = match event.kind {
                EventKind::Create(_) => event.paths.first().cloned(),
                _ => continue,
            };

            let path = match path {
                Some(p) => p,
                None => continue,
            };

            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if !matcher(file_name) {
                continue;
            }

            // Small delay: let the writing process finish.
            thread::sleep(Duration::from_millis(500));

            // Verify the file still exists and is readable.
            if !path.exists() {
                log(&format!("File vanished before processing: {}", file_name));
                continue;
            }

            log(&format!("New file detected: {}", file_name));

            // Extract the zip archive.
            match extract_zip(&path, &dest_dir) {
                Ok(count) => {
                    log(&format!(
                        "Extracted {} file(s) to '{}'",
                        count,
                        dest_dir.display()
                    ));

                    // Remove the original zip.
                    if let Err(e) = fs::remove_file(&path) {
                        log(&format!(
                            "Failed to remove original file '{}': {}",
                            path.display(),
                            e
                        ));
                    } else {
                        log("Original archive removed.");
                    }

                    // Play notification sound.
                    if sound_enabled {
                        play_sound(&sound_file, &log);
                    }

                    log("Done.");
                }
                Err(e) => {
                    log(&format!("Failed to extract '{}': {}", path.display(), e));
                }
            }
        }

        log("Watcher stopped.");
    });
}

/// Extract a zip archive to `dest_dir`. Returns the number of files extracted.
fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<usize, String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("cannot open zip: {}", e))?;

    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("cannot read zip: {}", e))?;

    let mut count = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("cannot read entry {}: {}", i, e))?;

        let outpath = match entry.enclosed_name() {
            Some(name) => dest_dir.join(name),
            None => continue,
        };

        if entry.is_dir() {
            fs::create_dir_all(&outpath)
                .map_err(|e| format!("cannot create dir '{}': {}", outpath.display(), e))?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create parent '{}': {}", parent.display(), e))?;
            }
            let mut output = fs::File::create(&outpath)
                .map_err(|e| format!("cannot create file '{}': {}", outpath.display(), e))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|e| format!("cannot write '{}': {}", outpath.display(), e))?;
            count += 1;
        }

        // Set Unix permissions if available.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode()
                && let Ok(meta) = fs::metadata(&outpath)
            {
                let mut perms = meta.permissions();
                if mode & 0o111 != 0 {
                    perms.set_mode(0o755);
                } else {
                    perms.set_mode(0o644);
                }
                let _ = fs::set_permissions(&outpath, perms);
            }
        }
    }

    Ok(count)
}

/// Play an audio file.
#[cfg(unix)]
fn play_sound(path: &Path, _log: &impl Fn(&str)) {
    let path_copy = path.to_path_buf();
    std::thread::spawn(move || {
        let path_str = path_copy.to_string_lossy().to_string();
        let result = std::process::Command::new("paplay")
            .arg(&path_str)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if result.is_err() {
            let _ = std::process::Command::new("aplay")
                .arg(&path_str)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    });
}

#[cfg(windows)]
fn play_sound(path: &Path, _log: &impl Fn(&str)) {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::playsoundapi::PlaySoundW;
    use winapi::um::winuser::SND_FILENAME;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SND_ASYNC (0x0001) so it doesn't block the watcher thread.
    unsafe { PlaySoundW(wide.as_ptr(), std::ptr::null_mut(), SND_FILENAME | 0x0001) };
}
