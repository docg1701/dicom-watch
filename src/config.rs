use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub directories: Directories,
    pub filter: Filter,
    pub sound: Sound,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Directories {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Filter {
    #[serde(rename = "mode")]
    pub mode_str: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sound {
    pub enabled: bool,
    pub file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Glob,
    Regex,
}

impl Filter {
    pub fn mode(&self) -> Result<FilterMode, String> {
        match self.mode_str.as_str() {
            "glob" => Ok(FilterMode::Glob),
            "regex" => Ok(FilterMode::Regex),
            other => Err(format!(
                "filter.mode must be 'glob' or 'regex', found '{}'",
                other
            )),
        }
    }
}

/// Load and validate config.toml from the same directory as the executable.
pub fn load_config(exe_dir: &Path) -> Result<Config, String> {
    let config_path = exe_dir.join("config.toml");

    if !config_path.exists() {
        return Err(format!(
            "Configuration file not found.\n\n\
             Expected at: {}\n\n\
             Create a config.toml file in the same directory as the executable.\n\
             See config.toml.example for reference.",
            config_path.display()
        ));
    }

    let content = std::fs::read_to_string(&config_path).map_err(|e| {
        format!(
            "Failed to read configuration file:\n  {}\n\nError: {}",
            config_path.display(),
            e
        )
    })?;

    let config: Config = toml::from_str(&content).map_err(|e| {
        let msg = e.to_string();
        format!(
            "Syntax error in configuration file:\n  {}\n\n{}\n\n\
             Check that all keys are correct and values are properly quoted.",
            config_path.display(),
            msg
        )
    })?;

    validate_config(&config, exe_dir)?;

    Ok(config)
}

fn validate_config(config: &Config, exe_dir: &Path) -> Result<(), String> {
    // Validate source directory
    let source = Path::new(&config.directories.source);
    if !source.exists() {
        return Err(format!(
            "Source directory does not exist:\n  {}\n\n\
             Check directories.source in config.toml",
            config.directories.source
        ));
    }
    if !source.is_dir() {
        return Err(format!(
            "Source path is not a directory:\n  {}\n\n\
             Check directories.source in config.toml",
            config.directories.source
        ));
    }

    // Validate destination directory
    let dest = Path::new(&config.directories.destination);
    if !dest.exists() {
        return Err(format!(
            "Destination directory does not exist:\n  {}\n\n\
             Create the directory or check directories.destination in config.toml",
            config.directories.destination
        ));
    }
    if !dest.is_dir() {
        return Err(format!(
            "Destination path is not a directory:\n  {}",
            config.directories.destination
        ));
    }

    // Validate filter mode
    config.filter.mode()?;

    // If regex mode, compile to check validity
    if config.filter.mode()? == FilterMode::Regex {
        regex::Regex::new(&config.filter.pattern).map_err(|e| {
            format!(
                "Invalid regex in filter.pattern:\n  '{}'\n\nError: {}\n\n\
                 See regex-guide.md for help with regular expressions.",
                config.filter.pattern, e
            )
        })?;
    }

    // Validate sound file if enabled
    if config.sound.enabled {
        let sound_path = resolve_path(&config.sound.file, exe_dir);
        if !sound_path.exists() {
            return Err(format!(
                "Sound file not found:\n  {}\n\n\
                 Check sound.file in config.toml or disable sound (sound.enabled = false)",
                sound_path.display()
            ));
        }
    }

    Ok(())
}

/// Resolve a path: absolute paths are used as-is; relative paths are resolved
/// against the executable's directory.
pub fn resolve_path(path_str: &str, exe_dir: &Path) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        exe_dir.join(p)
    }
}
