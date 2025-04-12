use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use clap::{Parser, ValueEnum};
use std::collections::HashMap;
use regex::Regex;

mod process;

/// Git Repository Manager - Rust implementation
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Mode of operation
    #[clap(value_enum)]
    mode: Mode,
}

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    /// Clone repositories
    Clone,
    /// Execute git commands
    Git,
    /// Update remote URL
    SetRemote,
    /// Configure repositories
    Configure,
    /// List remote relative paths
    ListRrel,
    /// List remote URLs
    ListRurl,
    /// List local relative paths
    ListLrel,
    /// Run with clone and set-remote
    Run,
    /// Create new repositories
    New,
}

/// Config structure to hold GRM configuration
struct Config {
    /// Configuration loaded from files and environment
    values: HashMap<String, String>,
    /// Flag values for operation modes
    mode_flags: HashMap<String, bool>,
    /// Path to current execution
    recurse_prefix: String,
}

impl Config {
    /// Create a new configuration
    fn new() -> Self {
        let mut config = Config {
            values: HashMap::new(),
            mode_flags: HashMap::new(),
            recurse_prefix: String::new(),
        };

        // Set defaults
        config.values.insert("CONFIG_FILENAME".to_string(), ".grm.conf".to_string());
        config.values.insert("LIST_FN".to_string(), ".grm.repos".to_string());
        config.values.insert("OPT_RECURSE".to_string(), "1".to_string());
        
        // Load environment variables
        for (key, value) in env::vars() {
            if key.starts_with("GRM_") {
                let config_key = &key[4..]; // Remove GRM_ prefix
                config.values.insert(config_key.to_string(), value);
            }
        }

        config
    }

    /// Get a configuration value
    fn get(&self, key: &str) -> Option<&String> {
        self.values.get(key)
    }

    /// Set a configuration value
    fn set(&mut self, key: String, value: String) {
        self.values.insert(key, value);
    }

    /// Get a boolean flag
    fn get_flag(&self, key: &str) -> bool {
        match self.values.get(key) {
            Some(value) => !value.is_empty(),
            None => false,
        }
    }

    /// Set a mode flag
    fn set_mode_flag(&mut self, mode: &str, value: bool) {
        self.mode_flags.insert(mode.to_string(), value);
    }

    /// Get a mode flag
    fn get_mode_flag(&self, mode: &str) -> bool {
        *self.mode_flags.get(mode).unwrap_or(&false)
    }

    /// Load configuration from a file
    fn load_from_file(&mut self, path: &Path) -> Result<()> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open config file: {}", path.display()))?;
        
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if let Some((key, value)) = parse_config_line(&line) {
                self.values.insert(key, value);
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

/// Find the nearest configuration file by walking up directories
fn find_conf_file(config: &Config) -> Result<PathBuf> {
    let config_filename = config.get("CONFIG_FILENAME")
        .ok_or_else(|| anyhow!("CONFIG_FILENAME not set"))?;
    
    let mut current_dir = env::current_dir()?;
    
    loop {
        let conf_path = current_dir.join(config_filename);
        if conf_path.exists() {
            return Ok(conf_path);
        }
        
        if !current_dir.pop() {
            return Err(anyhow!("Could not find configuration file {}", config_filename));
        }
    }
}

/// Process a repository
fn process_repo(config: &Config, local_path: &str, remote_path: &str, _media_path: &str) -> Result<()> {
    // Implement the repository processing logic similar to the Perl version
    println!("Processing repository: local={}, remote={}", local_path, remote_path);
    
    // Different behavior based on mode flags
    if config.get_mode_flag("MODE_LIST_RREL") {
        println!("{}", remote_path);
        return Ok(());
    }
    
    if config.get_mode_flag("MODE_LIST_LREL") {
        println!("{}", local_path);
        return Ok(());
    }
    
    if config.get_mode_flag("MODE_LIST_RURL") {
        // Construct remote URL
        let remote_url = match (config.get("RLOGIN"), config.get("RPATH_BASE")) {
            (Some(login), Some(base)) => format!("{}{}/{}", login, base, remote_path),
            _ => remote_path.to_string(),
        };
        println!("{}", remote_url);
        return Ok(());
    }
    
    // Continue with other mode operations like clone, configure, etc.
    // This would be a direct translation of the corresponding Perl functions
    
    Ok(())
}

/// Process a listfile (similar to listfile_process in Perl)
fn process_listfile(config: &mut Config, path: &Path) -> Result<()> {
    println!("Processing listfile: {}", path.display());
    
    let file = File::open(path)
        .with_context(|| format!("Failed to open listfile: {}", path.display()))?;
    
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        
        // Skip comments and empty lines
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        // Parse the line: REMOTE_REL * LOCAL_REL * GM_REL
        let parts: Vec<&str> = line.split('*').collect();
        if parts.len() < 1 {
            continue;
        }
        
        let remote_rel = parts[0].trim();
        if remote_rel.is_empty() {
            // This is a configuration line
            if parts.len() >= 3 {
                let key = parts[1].trim();
                let value = parts[2].trim();
                if !key.is_empty() {
                    config.set(key.to_string(), value.to_string());
                }
            }
            continue;
        }
        
        let local_rel = if parts.len() >= 2 { parts[1].trim() } else { "" };
        let gm_rel = if parts.len() >= 3 { parts[2].trim() } else { "" };
        
        // Extract repo name from remote path for default values
        let re = Regex::new(r"(?:.*(?<!/))?(.*?)(?:\.git)?$").unwrap();
        let repo_name = match re.captures(remote_rel) {
            Some(caps) => caps.get(1).map_or("", |m| m.as_str()),
            None => "",
        };
        
        let local_rel = if local_rel.is_empty() { repo_name } else { local_rel };
        let gm_rel = if gm_rel.is_empty() { repo_name } else { gm_rel };
        
        // Construct full paths
        let remote_dir = config.get("REMOTE_DIR").map(|s| s.as_str()).unwrap_or("");
        let local_dir = config.get("LOCAL_DIR").map(|s| s.as_str()).unwrap_or("");
        let gm_dir = config.get("GM_DIR").map(|s| s.as_str()).unwrap_or("");
        
        let remote_path = cat_path(&[remote_dir, remote_rel]);
        let local_path = cat_path(&[local_dir, local_rel]);
        let media_path = cat_path(&[gm_dir, gm_rel]);
        
        // Process the repository
        if let Err(e) = process_repo(config, &local_path, &remote_path, &media_path) {
            eprintln!("Error processing repository: {}", e);
        }
    }
    
    Ok(())
}

/// Concatenate path segments
fn cat_path(pieces: &[&str]) -> String {
    let mut result = String::new();
    
    for piece in pieces.iter().filter(|p| !p.is_empty()) {
        let piece = piece.trim_start_matches("./");
        
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(piece);
        
        // If this is an absolute path, return immediately
        if piece.starts_with('/') {
            return result;
        }
    }
    
    result
}

/// Set mode based on command line args
fn set_mode(config: &mut Config, mode: Mode) {
    match mode {
        Mode::Clone => {
            config.set_mode_flag("MODE_CLONE", true);
            config.set_mode_flag("MODE_CONFIGURE", true); // Cloning requires configuration
        },
        Mode::Git => {
            config.set_mode_flag("MODE_GIT", true);
            config.set_mode_flag("MODE_SET_REMOTE", true);
            config.set_mode_flag("MODE_CONFIGURE", true);
        },
        Mode::SetRemote => {
            config.set_mode_flag("MODE_SET_REMOTE", true);
        },
        Mode::Configure => {
            config.set_mode_flag("MODE_CONFIGURE", true);
        },
        Mode::ListRrel => {
            config.set_mode_flag("MODE_LIST_RREL", true);
        },
        Mode::ListRurl => {
            config.set_mode_flag("MODE_LIST_RURL", true);
        },
        Mode::ListLrel => {
            config.set_mode_flag("MODE_LIST_LREL", true);
        },
        Mode::Run => {
            config.set_mode_flag("MODE_CLONE", true);
            config.set_mode_flag("MODE_SET_REMOTE", true);
            config.set_mode_flag("MODE_CONFIGURE", true);
        },
        Mode::New => {
            config.set_mode_flag("MODE_NEW", true);
        },
    }
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Initialize configuration
    let mut config = Config::new();
    
    // Find and load configuration file
    let conf_path = find_conf_file(&config)?;
    config.load_from_file(&conf_path)?;
    
    // Set mode based on command line args
    set_mode(&mut config, args.mode);
    
    // Find and process listfile
    let list_fn = config.get("LIST_FN")
        .ok_or_else(|| anyhow!("LIST_FN not set in configuration"))?;
    
    let listfile = PathBuf::from(list_fn);
    if !listfile.exists() {
        return Err(anyhow!("Listfile {} not found", listfile.display()));
    }
    
    process_listfile(&mut config, &listfile)?;
    
    // Implement the recursive listfile processing if OPT_RECURSE is set
    if config.get_flag("OPT_RECURSE") {
        // TO DO: implement recursive listfile processing
    }
    
    Ok(())
}
