use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use lazy_static::lazy_static;

/// Typed configuration values with proper types for each setting
#[derive(Debug, Clone)]
pub struct Config {
    /// Configuration filename (.grm.conf by default)
    config_filename: String,
    /// List filename (.grm.repos by default)
    list_filename: String,
    /// Whether recursion is enabled (1 by default)
    recurse_enabled: bool,
    /// Remote login information (e.g., ssh://user@host)
    rlogin: Option<String>,
    /// Remote path base directory
    rpath_base: Option<String>,
    /// Remote path template for new repositories
    rpath_template: Option<String>,
    /// Local base directory for repositories
    local_dir: Option<String>,
    /// Media base directory
    gm_dir: Option<String>,
    /// Remote directory
    remote_dir: Option<String>,
    /// Git arguments when in git mode
    git_args: Option<String>,
    /// Command to execute for configuration
    config_cmd: Option<String>,
    /// Recurse prefix for path display
    recurse_prefix: String,
}

/// Known configuration keys that the program recognizes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    /// Configuration filename
    ConfigFilename,
    /// List filename
    ListFilename,
    /// Whether recursion is enabled
    RecurseEnabled,
    /// Remote login information
    RLogin,
    /// Remote path base directory
    RPathBase,
    /// Remote path template for new repositories
    RPathTemplate,
    /// Local directory
    LocalDir,
    /// Media directory
    GmDir,
    /// Remote directory
    RemoteDir,
    /// Git arguments when in git mode
    GitArgs,
    /// Command to execute for configuration
    ConfigCmd,
    /// Recurse prefix for path display
    RecursePrefix,
}

impl ConfigKey {
    /// Convert a string key to a ConfigKey enum
    pub fn from_str(key: &str) -> Option<Self> {
        match key {
            "CONFIG_FILENAME" => Some(ConfigKey::ConfigFilename),
            "LIST_FN" => Some(ConfigKey::ListFilename),
            "OPT_RECURSE" => Some(ConfigKey::RecurseEnabled),
            "RLOGIN" => Some(ConfigKey::RLogin),
            "RPATH_BASE" => Some(ConfigKey::RPathBase),
            "RPATH_TEMPLATE" => Some(ConfigKey::RPathTemplate),
            "LOCAL_DIR" => Some(ConfigKey::LocalDir),
            "GM_DIR" => Some(ConfigKey::GmDir),
            "REMOTE_DIR" => Some(ConfigKey::RemoteDir),
            "GIT_ARGS" => Some(ConfigKey::GitArgs),
            "CONFIG_CMD" => Some(ConfigKey::ConfigCmd),
            "RECURSE_PREFIX" => Some(ConfigKey::RecursePrefix),
            _ => None,
        }
    }
    
    /// Convert a ConfigKey enum to its string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfigKey::ConfigFilename => "CONFIG_FILENAME",
            ConfigKey::ListFilename => "LIST_FN",
            ConfigKey::RecurseEnabled => "OPT_RECURSE",
            ConfigKey::RLogin => "RLOGIN",
            ConfigKey::RPathBase => "RPATH_BASE",
            ConfigKey::RPathTemplate => "RPATH_TEMPLATE",
            ConfigKey::LocalDir => "LOCAL_DIR",
            ConfigKey::GmDir => "GM_DIR",
            ConfigKey::RemoteDir => "REMOTE_DIR",
            ConfigKey::GitArgs => "GIT_ARGS",
            ConfigKey::ConfigCmd => "CONFIG_CMD",
            ConfigKey::RecursePrefix => "RECURSE_PREFIX",
        }
    }
}

impl Config {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self {
            config_filename: ".grm.conf".to_string(),
            list_filename: ".grm.repos".to_string(),
            recurse_enabled: true,
            rlogin: None,
            rpath_base: None,
            rpath_template: None,
            local_dir: None,
            gm_dir: None,
            remote_dir: None,
            git_args: None,
            config_cmd: None,
            recurse_prefix: String::new(),
        }
    }
    
    /// Get a configuration value by key string
    pub fn get(&self, key: &str) -> Option<&String> {
        match ConfigKey::from_str(key) {
            Some(config_key) => self.get_by_key(config_key),
            None => None,
        }
    }
    
    /// Get a configuration value by typed key
    pub fn get_by_key(&self, key: ConfigKey) -> Option<&String> {
        lazy_static! {
            static ref ENABLED: String = "1".to_string();
            static ref DISABLED: String = String::new();
        }
        
        match key {
            ConfigKey::ConfigFilename => Some(&self.config_filename),
            ConfigKey::ListFilename => Some(&self.list_filename),
            ConfigKey::RecurseEnabled => {
                if self.recurse_enabled {
                    Some(&*ENABLED)
                } else {
                    Some(&*DISABLED)
                }
            },
            ConfigKey::RLogin => self.rlogin.as_ref(),
            ConfigKey::RPathBase => self.rpath_base.as_ref(),
            ConfigKey::RPathTemplate => self.rpath_template.as_ref(),
            ConfigKey::LocalDir => self.local_dir.as_ref(),
            ConfigKey::GmDir => self.gm_dir.as_ref(),
            ConfigKey::RemoteDir => self.remote_dir.as_ref(),
            ConfigKey::GitArgs => self.git_args.as_ref(),
            ConfigKey::ConfigCmd => self.config_cmd.as_ref(),
            ConfigKey::RecursePrefix => Some(&self.recurse_prefix),
        }
    }
    
    /// Set a configuration value by key
    pub fn set(&mut self, key: String, value: String) {
        if let Some(config_key) = ConfigKey::from_str(&key) {
            match config_key {
                ConfigKey::ConfigFilename => self.config_filename = value,
                ConfigKey::ListFilename => self.list_filename = value,
                ConfigKey::RecurseEnabled => self.recurse_enabled = !value.is_empty(),
                ConfigKey::RLogin => self.rlogin = Some(value),
                ConfigKey::RPathBase => self.rpath_base = Some(value),
                ConfigKey::RPathTemplate => self.rpath_template = Some(value),
                ConfigKey::LocalDir => self.local_dir = Some(value),
                ConfigKey::GmDir => self.gm_dir = Some(value),
                ConfigKey::RemoteDir => self.remote_dir = Some(value),
                ConfigKey::GitArgs => self.git_args = Some(value),
                ConfigKey::ConfigCmd => self.config_cmd = Some(value),
                ConfigKey::RecursePrefix => self.recurse_prefix = value,
            }
        }
        // If the key is not recognized, we simply ignore it as requested
    }
    
    /// Get a boolean flag
    pub fn get_flag(&self, key: &str) -> bool {
        match self.get(key) {
            Some(value) => !value.is_empty(),
            None => false,
        }
    }
    
    /// Get the recurse prefix for path display
    pub fn get_recurse_prefix(&self) -> &str {
        &self.recurse_prefix
    }
    
    /// Get all configuration values
    pub fn all_values(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        
        // Add all non-None values
        result.push((
            ConfigKey::ConfigFilename.as_str().to_string(), 
            self.config_filename.clone()
        ));
        
        result.push((
            ConfigKey::ListFilename.as_str().to_string(), 
            self.list_filename.clone()
        ));
        
        result.push((
            ConfigKey::RecurseEnabled.as_str().to_string(),
            if self.recurse_enabled { "1".to_string() } else { "".to_string() },
        ));
        
        if let Some(value) = &self.rlogin {
            result.push((ConfigKey::RLogin.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.rpath_base {
            result.push((ConfigKey::RPathBase.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.rpath_template {
            result.push((ConfigKey::RPathTemplate.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.local_dir {
            result.push((ConfigKey::LocalDir.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.gm_dir {
            result.push((ConfigKey::GmDir.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.remote_dir {
            result.push((ConfigKey::RemoteDir.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.git_args {
            result.push((ConfigKey::GitArgs.as_str().to_string(), value.clone()));
        }
        
        if let Some(value) = &self.config_cmd {
            result.push((ConfigKey::ConfigCmd.as_str().to_string(), value.clone()));
        }
        
        result.push((
            ConfigKey::RecursePrefix.as_str().to_string(),
            self.recurse_prefix.clone(),
        ));
        
        result
    }
    
    /// Load configuration from environment variables
    pub fn load_from_env(&mut self) {
        for (key, value) in std::env::vars() {
            if key.starts_with("GRM_") {
                let config_key = key[4..].to_string(); // Remove GRM_ prefix
                
                // Special case for recurse prefix to avoid move issue
                if key == "GRM_RECURSE_PREFIX" {
                    self.recurse_prefix = value.clone();
                }
                
                self.set(config_key, value);
            }
        }
    }
    
    /// Load configuration from a file
    pub fn load_from_file(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open config file: {}", path.display()))?;
        
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if let Some((key, value)) = parse_config_line(&line) {
                self.set(key, value);
            }
        }
        
        Ok(())
    }
}

/// Parse a configuration line
fn parse_config_line(line: &str) -> Option<(String, String)> {
    // Skip comments and empty lines
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    
    // Config format is expected to be: * KEY * VALUE
    let parts: Vec<&str> = line.split('*').collect();
    if parts.len() >= 3 {
        let key = parts[1].trim().to_string();
        let value = parts[2].trim().to_string();
        if !key.is_empty() {
            return Some((key, value));
        }
    }
    
    None
}
