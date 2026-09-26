use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Directory holding `esh` configuration and credentials.
///
/// Honours `ESH_CONFIG_HOME`, then `XDG_CONFIG_HOME`, then `%APPDATA%` on
/// Windows, and finally falls back to `~/.config/esh`.
pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = env_path("ESH_CONFIG_HOME") {
        return Ok(dir);
    }
    if let Some(dir) = env_path("XDG_CONFIG_HOME") {
        return Ok(dir.join("esh"));
    }
    #[cfg(windows)]
    {
        if let Some(dir) = env_path("APPDATA") {
            return Ok(dir.join("esh"));
        }
    }
    Ok(home()?.join(".config").join("esh"))
}

/// Directory holding `esh` data such as the translation history.
///
/// Honours `ESH_DATA_HOME`, then `XDG_DATA_HOME`, then `%LOCALAPPDATA%` on
/// Windows, and finally falls back to `~/.local/share/esh`.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = env_path("ESH_DATA_HOME") {
        return Ok(dir);
    }
    if let Some(dir) = env_path("XDG_DATA_HOME") {
        return Ok(dir.join("esh"));
    }
    #[cfg(windows)]
    {
        if let Some(dir) = env_path("LOCALAPPDATA") {
            return Ok(dir.join("esh"));
        }
    }
    Ok(home()?.join(".local").join("share").join("esh"))
}

/// Path of the JSON file holding the translation history.
pub fn history_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("history.json"))
}

fn home() -> Result<PathBuf> {
    env_path("HOME")
        .or_else(|| env_path("USERPROFILE"))
        .ok_or_else(|| Error::other("could not determine the home directory"))
}

fn env_path(name: &str) -> Option<PathBuf> {
    match env::var_os(name) {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

/// Creates `path` (and its parents) and restricts it to the current user.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `contents` to `path`, keeping the file readable only by the current
/// user (mode `0600` on Unix). Used to protect the stored API key.
pub fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.write_all(b"\n")?;

    // Tighten permissions even if the file already existed with looser ones.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}
