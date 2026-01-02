//! Configuration management for TrinityChain

use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub network: NetworkConfig,
    pub database: DatabaseConfig,
    pub miner: MinerConfig,
    #[serde(default)]
    pub ai_validation: AIValidationConfig,
}

#[derive(Debug, Deserialize)]
pub struct NetworkConfig {
    pub p2p_port: u16,
    pub api_port: u16,
    #[serde(default = "default_network_id")]
    pub network_id: String,
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
    #[serde(default = "default_min_peers")]
    pub min_peers: u16,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_data_dir")]
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct MinerConfig {
    pub threads: usize,
    pub beneficiary_address: String,
    #[serde(default = "default_mining_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct AIValidationConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub enable_transaction_validation: bool,
    #[serde(default = "default_enabled")]
    pub enable_for_all_clients: bool,
}

impl Default for AIValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: "claude-3-5-haiku-20241022".to_string(),
            provider: "anthropic".to_string(),
            timeout_secs: 30,
            enable_transaction_validation: true,
            enable_for_all_clients: true,
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_model() -> String {
    "claude-3-5-haiku-20241022".to_string()
}

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_timeout() -> u64 {
    30
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_str = fs::read_to_string("config.toml").unwrap_or_default();
    let config: Config = if config_str.is_empty() {
        // Provide sane defaults when config.toml is absent
        Config {
            network: NetworkConfig {
                p2p_port: 8333,
                api_port: 8080,
                network_id: default_network_id(),
                bootstrap_peers: Vec::new(),
                min_peers: default_min_peers(),
            },
            database: DatabaseConfig {
                path: default_data_dir(),
            },
            miner: MinerConfig {
                threads: 1,
                beneficiary_address: "00000000000000000000000000000000".to_string(),
                enabled: default_mining_enabled(),
            },
            ai_validation: AIValidationConfig::default(),
        }
    } else {
        toml::from_str(&config_str)?
    };

    // Validate critical values
    if config.database.path.is_empty() {
        return Err("database.path must be set in config.toml".into());
    }

    if config.miner.beneficiary_address.is_empty() {
        return Err("miner.beneficiary_address must be set in config.toml".into());
    }

    Ok(config)
}

fn default_network_id() -> String {
    "devnet".to_string()
}

fn default_min_peers() -> u16 {
    1
}

fn default_data_dir() -> String {
    "./data".to_string()
}

fn default_mining_enabled() -> bool {
    false
}
