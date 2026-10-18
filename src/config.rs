use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub notes_dir: Option<String>,
    #[serde(default)]
    pub hide_done: bool,
}

impl Config {
    pub fn load() -> Config {
        Self::path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str::<Config>(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).expect("Config serializes to TOML");
        fs::write(path, text)
    }

    pub fn path() -> Option<PathBuf> {
        Self::path_from_env(
            std::env::var("XDG_CONFIG_HOME").ok(),
            std::env::var("HOME").ok(),
            std::env::var("USERPROFILE").ok(),
        )
    }

    fn path_from_env(
        xdg_config_home: Option<String>,
        home: Option<String>,
        user_profile: Option<String>,
    ) -> Option<PathBuf> {
        let base = xdg_config_home
            .as_deref()
            .filter(|s| !s.is_empty() && Path::new(s).is_absolute())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| Path::new(&h).join(".config")))
            .or_else(|| user_profile.map(|p| Path::new(&p).join(".config")));
        Some(base?.join("gtd-app").join("config.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip() {
        let config = Config {
            notes_dir: Some("/tmp/vault/GTD".into()),
            hide_done: true,
        };
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn missing_fields_default() {
        let back: Config = toml::from_str("").unwrap();
        assert_eq!(back, Config::default());
        let back: Config = toml::from_str("notes_dir = \"/x\"\n").unwrap();
        assert_eq!(back.notes_dir.as_deref(), Some("/x"));
        assert!(!back.hide_done);
    }

    #[test]
    fn path_prefers_xdg_when_absolute() {
        let p = Config::path_from_env(Some("/cfg".into()), Some("/home/u".into()), None).unwrap();
        assert_eq!(p, PathBuf::from("/cfg/gtd-app/config.toml"));
    }

    #[test]
    fn path_falls_back_to_home() {
        let p = Config::path_from_env(None, Some("/home/u".into()), None).unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.config/gtd-app/config.toml"));
    }

    #[test]
    fn path_ignores_relative_xdg() {
        let p = Config::path_from_env(Some("relative".into()), Some("/home/u".into()), None).unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.config/gtd-app/config.toml"));
    }
}
