use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// CUPS printer queue name, e.g. "EPSON_L3150_Series"
    pub printer_name: String,

    /// mDNS instance name as discovered, e.g. "EPSON L3150 Series"
    pub mdns_instance_name: String,

    /// mDNS hostname for TCP reachability check, e.g. "EPSON-L3150-Series.local."
    pub mdns_hostname: String,

    /// Seconds between each watchdog poll cycle
    pub poll_interval_secs: u64,

    /// Seconds to wait after printer detected before issuing cupsenable
    pub enable_delay_secs: u64,

    /// Path of the launchd plist that was installed
    pub plist_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            printer_name: "EPSON_L3150_Series".to_string(),
            mdns_instance_name: "EPSON L3150 Series".to_string(),
            mdns_hostname: String::new(),
            poll_interval_secs: 30,
            enable_delay_secs: 3,
            plist_path: String::new(),
        }
    }
}

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home)
            .join(".config")
            .join("epson-watchdog")
            .join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        let contents = fs::read_to_string(&path).with_context(|| {
            format!(
                "Cannot read config from {}. Run 'epson-watchdog install' first.",
                path.display()
            )
        })?;
        toml::from_str(&contents).context("Failed to parse config.toml")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create config dir: {}", parent.display()))?;
        }
        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, contents)
            .with_context(|| format!("Cannot write config to {}", path.display()))?;
        log::info!("Config saved to {}", path.display());
        Ok(())
    }
}
