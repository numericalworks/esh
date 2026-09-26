use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsutil;
use crate::provider;

/// Resolved settings used to talk to the configured LLM provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Provider identifier, e.g. `ollama` or `openai`.
    pub provider: String,
    pub server_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default = "default_provider")]
    provider: String,
    server_url: String,
    model: String,
}

fn default_provider() -> String {
    provider::DEFAULT_ID.to_string()
}

#[derive(Default, Serialize, Deserialize)]
struct CredentialsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
}

/// Location of the on-disk settings files.
///
/// The API key is kept in a separate credentials file so it can be protected
/// independently of the rest of the configuration.
pub struct SettingsStore {
    pub config_path: PathBuf,
    pub credentials_path: PathBuf,
}

impl SettingsStore {
    pub fn from_env() -> Result<Self> {
        let dir = fsutil::config_dir()?;
        Ok(Self::new(dir.join("config.json"), dir.join("credentials.json")))
    }

    pub fn new(config_path: PathBuf, credentials_path: PathBuf) -> Self {
        Self {
            config_path,
            credentials_path,
        }
    }

    /// Returns `None` when `esh` has not been configured yet (or the stored
    /// configuration is incomplete), which triggers first-launch setup.
    pub fn load(&self) -> Result<Option<Settings>> {
        if !self.config_path.exists() {
            return Ok(None);
        }

        let config: ConfigFile = read_json(&self.config_path)?;

        let api_key = if self.credentials_path.exists() {
            read_json::<CredentialsFile>(&self.credentials_path)?.api_key
        } else {
            None
        };

        if config.server_url.trim().is_empty() || config.model.trim().is_empty() {
            return Ok(None);
        }

        if provider::lookup(&config.provider).is_none() {
            return Err(Error::other(format!(
                "unknown provider '{}' in the configuration; run `esh setup` to reconfigure",
                config.provider
            )));
        }

        Ok(Some(Settings {
            provider: config.provider,
            server_url: config.server_url,
            model: config.model,
            api_key,
        }))
    }

    pub fn save(&self, settings: &Settings) -> Result<()> {
        let config = ConfigFile {
            provider: settings.provider.clone(),
            server_url: settings.server_url.clone(),
            model: settings.model.clone(),
        };
        fsutil::write_private(&self.config_path, &serde_json::to_vec_pretty(&config)?)?;

        let credentials = CredentialsFile {
            api_key: settings.api_key.clone(),
        };
        fsutil::write_private(&self.credentials_path, &serde_json::to_vec_pretty(&credentials)?)?;

        Ok(())
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let contents = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("esh-config-{}-{nanos}-{label}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn settings(provider: &str) -> Settings {
        Settings {
            provider: provider.to_string(),
            server_url: "http://localhost:11434".to_string(),
            model: "llama3".to_string(),
            api_key: Some("secret".to_string()),
        }
    }

    #[test]
    fn reports_missing_configuration() {
        let dir = temp_dir("missing");
        let store = SettingsStore::new(dir.join("config.json"), dir.join("credentials.json"));
        assert!(store.load().unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn round_trips_settings_including_api_key() {
        let dir = temp_dir("roundtrip");
        let store = SettingsStore::new(dir.join("config.json"), dir.join("credentials.json"));
        let settings = settings("openai");

        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap(), Some(settings));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn api_key_is_optional() {
        let dir = temp_dir("nokey");
        let store = SettingsStore::new(dir.join("config.json"), dir.join("credentials.json"));
        let settings = Settings {
            provider: "ollama".to_string(),
            server_url: "http://localhost:11434".to_string(),
            model: "llama3".to_string(),
            api_key: None,
        };

        store.save(&settings).unwrap();

        assert_eq!(store.load().unwrap(), Some(settings));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn config_without_provider_defaults_to_ollama() {
        let dir = temp_dir("legacy");
        fs::write(
            dir.join("config.json"),
            r#"{"server_url":"http://localhost:11434","model":"llama3"}"#,
        )
        .unwrap();
        let store = SettingsStore::new(dir.join("config.json"), dir.join("credentials.json"));

        let settings = store.load().unwrap().unwrap();

        assert_eq!(settings.provider, "ollama");
        assert_eq!(settings.model, "llama3");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_provider_is_an_error() {
        let dir = temp_dir("unknown");
        fs::write(
            dir.join("config.json"),
            r#"{"provider":"nope","server_url":"http://x","model":"m"}"#,
        )
        .unwrap();
        let store = SettingsStore::new(dir.join("config.json"), dir.join("credentials.json"));

        let error = store.load().unwrap_err();

        assert!(error.to_string().contains("unknown provider"), "was: {error}");
        let _ = fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn stored_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("perms");
        let store = SettingsStore::new(dir.join("config.json"), dir.join("credentials.json"));
        let settings = settings("openai");

        store.save(&settings).unwrap();

        for path in [&store.config_path, &store.credentials_path] {
            let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{path:?} should be readable only by its owner");
        }
        let _ = fs::remove_dir_all(dir);
    }
}
