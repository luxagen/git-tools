use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use clap::{Parser, ValueEnum};
use std::collections::HashMap;
use regex::Regex;
use url::Url;

mod process;
mod recursive;
mod repository;
mod mode;

use mode::{PrimaryMode, ModeConfig, MODE_CONFIG, initialize_mode};

/// Separator character used in .grm.repos files
const LIST_SEPARATOR: char = '*';

/// Git Repository Manager - Rust implementation
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Mode of operation
    #[clap(value_enum)]
    mode: PrimaryMode,
    
    /// Additional arguments (for git mode)
    #[clap(trailing_var_arg = true)]
    args: Vec<String>,
}

/// Config structure to hold GRM configuration
#[derive(Clone)]
pub struct Config {
    /// Configuration loaded from files and environment
    values: HashMap<String, String>,
    /// Path to current execution
    recurse_prefix: String,
}

impl Config {
    /// Create a new configuration
    fn new() -> Self {
        let mut config = Config {
            values: HashMap::new(),
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
                
                // Special case for recurse prefix
                if config_key == "RECURSE_PREFIX" {
                    config.recurse_prefix = config.values.get(config_key).unwrap().clone();
                }
            }
        }

        config
    }

    /// Get a configuration value
    pub fn get(&self, key: &str) -> Option<&String> {
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
    
    /// Get the current recurse prefix
    fn get_recurse_prefix(&self) -> &str {
        &self.recurse_prefix
    }
    
    /// Get all configuration values
    fn all_values(&self) -> Vec<(String, String)> {
        self.values.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
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
fn process_repo(config: &Config, local_path: &str, remote_path: &str, media_path: &str) -> Result<()> {
    // Get the current recurse prefix for path display
    let recurse_prefix = config.get_recurse_prefix();
    
    // Create prefixed paths 
    let prefixed_local_path = format!("{}{}", recurse_prefix, local_path);
    let prefixed_remote_path = format!("{}{}", recurse_prefix, remote_path);
    
    // Different behavior based on mode flags
    if MODE_CONFIG.get_flag("MODE_LIST_RREL") {
        println!("{}", prefixed_remote_path);
        return Ok(());
    }
    
    if MODE_CONFIG.get_flag("MODE_LIST_LREL") {
        println!("{}", prefixed_local_path);
        return Ok(());
    }
    
    if MODE_CONFIG.get_flag("MODE_LIST_RURL") {
        // Construct remote URL using the remote_path (without prefix)
        // since the remote server paths include the full hierarchy
        let remote_url = match (config.get("RLOGIN"), config.get("RPATH_BASE")) {
            (Some(login), Some(base)) => {
                // Handle escaping characters in remote paths
                let clean_remote_path = normalize_path_for_url(remote_path);
                format!("{}{}/{}", login, base, clean_remote_path)
            },
            _ => remote_path.to_string(),
        };
        println!("{}", remote_url);
        return Ok(());
    }
    
    // Skip processing for listing modes
    if MODE_CONFIG.get_flag("MODE_LIST_RREL") || 
       MODE_CONFIG.get_flag("MODE_LIST_LREL") || 
       MODE_CONFIG.get_flag("MODE_LIST_RURL") {
        return Ok(());
    }
    
    // Get local path info
    let path = Path::new(local_path);
    
    // Process based on path state
    if !path.exists() {
        if MODE_CONFIG.get_flag("MODE_NEW") {
            eprintln!("ERROR: {} does not exist", prefixed_local_path);
            return Ok(());
        }
        
        // Only clone if MODE_CLONE is set
        if !MODE_CONFIG.get_flag("MODE_CLONE") {
            eprintln!("ERROR: {} does not exist", prefixed_local_path);
            return Ok(());
        }
        
        // Get remote URL
        let remote_url = match (config.get("RLOGIN"), config.get("RPATH_BASE")) {
            (Some(login), Some(base)) => {
                // Handle escaping characters in remote paths
                let clean_remote_path = normalize_path_for_url(remote_path);
                format!("{}{}/{}", login, base, clean_remote_path)
            },
            _ => remote_path.to_string(),
        };
        
        // Clone, configure, and checkout
        repository::clone_repo_no_checkout(local_path, &remote_url)?;
        repository::configure_repo(local_path, media_path, config)?;
        repository::check_out(local_path)?;
        
        return Ok(());
    }
    
    // Check if path is a directory
    if !path.is_dir() {
        eprintln!("ERROR: {} is not a directory", prefixed_local_path);
        return Ok(());
    }
    
    // Check if directory is a git repository
    if repository::is_dir_repo_root(local_path)? {
        if MODE_CONFIG.get_flag("MODE_NEW") {
            eprintln!("{} already exists (skipping)", prefixed_local_path);
            return Ok(());
        }
        
        // Get remote URL
        let remote_url = match (config.get("RLOGIN"), config.get("RPATH_BASE")) {
            (Some(login), Some(base)) => {
                // Handle escaping characters in remote paths
                let clean_remote_path = normalize_path_for_url(remote_path);
                format!("{}{}/{}", login, base, clean_remote_path)
            },
            _ => remote_path.to_string(),
        };
        
        eprintln!("{} exists", prefixed_local_path);
        
        // Update remote and configure
        if MODE_CONFIG.get_flag("MODE_SET_REMOTE") {
            repository::set_remote(local_path, &remote_url)?;
        }
        
        if MODE_CONFIG.get_flag("MODE_CONFIGURE") {
            repository::configure_repo(local_path, media_path, config)?;
        }
        
        if MODE_CONFIG.get_flag("MODE_GIT") {
            // Execute git commands in the repository
            if let Some(git_args) = config.get("GIT_ARGS") {
                repository::run_git_command(local_path, git_args)?;
            }
        }
        
        return Ok(());
    }
    
    // Handle non-repo directories
    if !MODE_CONFIG.get_flag("MODE_NEW") {
        eprintln!("ERROR: {} is not a Git repository", prefixed_local_path);
        return Ok(());
    }
    
    // In "new" mode, we want to create git repositories for existing directories
    // that aren't git repositories yet, regardless of whether they're in .grm.repos
    
    // Only create a repository if the directory exists
    if path.exists() {
        // New mode for existing directory
        eprintln!("Creating new Git repository in {}", prefixed_local_path);
        repository::create_new(local_path, remote_path, config)?;
        eprintln!("{} created", prefixed_local_path);
    } else {
        // Directory doesn't exist, just skip it
        eprintln!("{} does not exist (skipping)", prefixed_local_path);
    }
    
    Ok(())
}

/// Process a listfile (similar to listfile_process in Perl)
fn process_listfile(config: &mut Config, path: &Path) -> Result<()> {
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
        
        // Parse the line using the separator character (*)
        let separator = LIST_SEPARATOR;
        let parts = split_with_escapes(line, separator);
        if parts.len() < 1 {
            continue;
        }
        
        // Get the raw paths from the listfile
        let remote_rel_raw = parts[0].clone();
        
        if remote_rel_raw.is_empty() {
            // This is a configuration line
            if parts.len() >= 3 {
                let key = parts[1].clone();
                let value = parts[2].clone();
                if !key.is_empty() {
                    config.set(key, value);
                }
            }
            continue;
        }
        
        // Extract repo name from remote path for default values
        let re = Regex::new(r"([^/]+?)(?:\.git)?$").unwrap();
        let repo_name = match re.captures(&remote_rel_raw) {
            Some(caps) => caps.get(1).map_or("", |m| m.as_str()),
            None => "",
        };
        
        // Get remaining path parts
        let local_rel_raw = if parts.len() >= 2 { parts[1].clone() } else { String::new() };
        let gm_rel_raw = if parts.len() >= 3 { parts[2].clone() } else { String::new() };
        
        // Unescape all paths - do this once and store as String
        let remote_rel_unescaped = unescape_backslashes(&remote_rel_raw);
        
        // Apply defaults and unescape
        let local_rel_unescaped = if local_rel_raw.is_empty() {
            repo_name.to_string()
        } else {
            unescape_backslashes(&local_rel_raw)
        };
        
        let gm_rel_unescaped = if gm_rel_raw.is_empty() {
            repo_name.to_string()
        } else {
            unescape_backslashes(&gm_rel_raw)
        };
        
        // Construct full paths
        let remote_dir = config.get("REMOTE_DIR").map(|s| s.as_str()).unwrap_or("");
        let local_dir = config.get("LOCAL_DIR").map(|s| s.as_str()).unwrap_or("");
        let gm_dir = config.get("GM_DIR").map(|s| s.as_str()).unwrap_or("");
        
        let remote_path = cat_path(&[remote_dir, &remote_rel_unescaped]);
        let local_path = cat_path(&[local_dir, &local_rel_unescaped]);
        let media_path = cat_path(&[gm_dir, &gm_rel_unescaped]);
        
        if MODE_CONFIG.get_flag("MODE_DEBUG") {
            eprintln!("Potential target: {}", local_path);
        }
        
        // Process the repository
        process_repo(config, &local_path, &remote_path, &media_path)?;
    }
    
    Ok(())
}

/// Split a line by separator character, respecting escaped separators
fn split_with_escapes(line: &str, separator: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();
    
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Backslash found - get the next character and treat it literally
            if let Some(next_char) = chars.next() {
                current.push(next_char);
            } else {
                // Handle trailing backslash at end of line
                current.push('\\');
            }
        } else if c == separator {
            // Unescaped separator - add current part to result and start a new one
            result.push(current.trim().to_string());
            current = String::new();
        } else {
            // Regular character, just add it
            current.push(c);
        }
    }
    
    // Add the last part
    result.push(current.trim().to_string());
    
    result
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

/// Clean and normalize a path for remote URL construction
fn normalize_path_for_url(path: &str) -> String {
    // Use the standard URL library for proper path encoding
    // This handles spaces, brackets, and all other special characters
    Url::parse("http://example.com/")
        .unwrap()
        .join(path)
        .unwrap()
        .path()
        .trim_start_matches('/')
        .to_string()
}

/// Unescape backslash sequences in a string
/// Simply removes backslashes, treating the character after each as a literal
fn unescape_backslashes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some() {
            // Skip the backslash and add whatever character follows it
            if let Some(next_char) = chars.next() {
                result.push(next_char);
            }
        } else {
            result.push(c);
        }
    }
    
    result
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Create configuration
    let mut config = Config::new();
    
    // Load configuration from file
    let conf_path = find_conf_file(&config)?;
    config.load_from_file(&conf_path)?;
    
    // Initialize mode configuration
    initialize_mode(args.mode);
    
    // Store git command arguments if in git mode
    if args.mode.to_string() == "git" && !args.args.is_empty() {
        let git_args = args.args.join(" ");
        config.set("GIT_ARGS".to_string(), git_args);
    }
    
    // Get listfile directory and path
    let list_dir = find_listfile_dir(&config)?;
    let list_file_name = config.get("LIST_FN")
        .ok_or_else(|| anyhow!("LIST_FN not set"))?;
    let list_path = list_dir.join(list_file_name);
    
    // Process listfile
    if list_path.exists() {
        process_listfile(&mut config, &list_path)?;
    } else {
        eprintln!("No .grm.repos file found");
    }
    
    Ok(())
}

/// Find directory containing listfile by walking up from current directory
fn find_listfile_dir(config: &Config) -> Result<PathBuf> {
    let list_fn = config.get("LIST_FN")
        .ok_or_else(|| anyhow!("LIST_FN not set"))?;
    
    let mut current_dir = env::current_dir()?;
    
    loop {
        let list_path = current_dir.join(list_fn);
        if list_path.exists() {
            return Ok(current_dir);
        }
        
        if !current_dir.pop() {
            return Err(anyhow!("Could not find listfile {} in current directory or any ancestor", list_fn));
        }
    }
}
