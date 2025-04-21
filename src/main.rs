#![allow(dead_code)]
#![allow(unused_imports)]

use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::collections::HashMap;
use regex::Regex;
use url::Url;

mod process;
mod recursive;
mod repository;
mod mode;
mod config;
mod remote_url;

use mode::{PrimaryMode, initialize_operations, get_operations};
use config::Config;

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

/// Find the nearest configuration file by walking up directories
fn find_conf_file(config: &Config) -> Result<PathBuf> {
    let mut current_dir = env::current_dir()?;
    
    loop {
        let conf_path = current_dir.join(&config.config_filename);
        if conf_path.exists() {
            return Ok(conf_path);
        }
        
        if !current_dir.pop() {
            break;
        }
    }
    
    Err(anyhow!("Configuration file not found"))
}

/// Process a repository
fn process_repo(config: &Config, local_path: &str, remote_rel_path: &str, media_path: &str) -> Result<()> {
    // Get the current recurse prefix for path display
    let recurse_prefix = &config.recurse_prefix;
    
    // Create prefixed paths 
    let prefixed_local_path = format!("{}{}", recurse_prefix, local_path);
    let prefixed_remote_path = format!("{}{}", recurse_prefix, remote_rel_path);
    
    // Get operations
    let operations = get_operations();
    
    // Different behavior based on mode flags
    if operations.list_rrel {
        println!("{}", prefixed_remote_path);
        return Ok(());
    }
    
    if operations.list_lrel {
        println!("{}", prefixed_local_path);
        return Ok(());
    }
    
    if operations.list_rurl {
        // Generate remote URL using only the remote relative path
        println!("{}", get_remote_url(config, remote_rel_path));
        return Ok(());
    }
    
    // Skip processing for listing modes
    if operations.is_listing_mode() {
        return Ok(());
    }
    
    // Get local path info
    let path = Path::new(local_path);
    
    // Process based on path state
    if !path.exists() {
        if operations.new {
            eprintln!("ERROR: {} does not exist", prefixed_local_path);
            return Ok(());
        }
        
        // Only clone if clone operation is enabled
        if !operations.clone {
            eprintln!("ERROR: {} does not exist", prefixed_local_path);
            return Ok(());
        }
        
        // Clone, configure, and checkout
        repository::clone_repo_no_checkout(local_path, &get_remote_url(config, remote_rel_path))?;
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
    let is_repo = match repository::is_dir_repo_root(local_path) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Error checking if {} is a Git repository: {}", local_path, err);
            return Ok(());
        }
    };
    
    if is_repo {
        if operations.new {
            eprintln!("{} already exists (skipping)", prefixed_local_path);
            return Ok(());
        }
        
        // Get remote URL
        eprintln!("{} exists", prefixed_local_path);
        
        // Update remote and configure
        if operations.set_remote {
            repository::set_remote(local_path, &get_remote_url(config, remote_rel_path))?;
        }
        
        if operations.configure {
            repository::configure_repo(local_path, media_path, config)?;
        }
        
        if operations.git {
            // Execute git commands in the repository
            if let Some(git_args) = &config.git_args {
                repository::run_git_command(local_path, git_args)?;
            }
        }
        
        return Ok(());
    }
    
    // Handle non-repo directories
    if !operations.new {
        eprintln!("ERROR: {} is not a Git repository", prefixed_local_path);
        return Ok(());
    }
    
    // In "new" mode, we want to create git repositories for existing directories
    // that aren't git repositories yet, regardless of whether they're in .grm.repos
    
    // Only create a repository if the directory exists
    if path.exists() {
        // New mode for existing directory
        eprintln!("Creating new Git repository in {}", prefixed_local_path);
        repository::create_new(local_path, remote_rel_path, config)?;
        eprintln!("{} created", prefixed_local_path);
    } else {
        // Directory doesn't exist, just skip it
        eprintln!("{} does not exist (skipping)", prefixed_local_path);
    }
    
    Ok(())
}

/// Process a listfile (similar to listfile_process in Perl)
fn process_listfile(config: &mut Config, list_path: &Path) -> Result<()> {
    let contents = fs::read_to_string(list_path)
        .with_context(|| format!("Failed to read {}", list_path.display()))?;
    
    // Process each line in the file
    for line in contents.lines() {
        let line = line.trim();
        
        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        
        if let Err(err) = process_repo_line(config, line) {
            eprintln!("Error processing line \"{}\": {}", line, err);
        }
    }
    
    // Process subdirectories if recursion is enabled
    let operations = get_operations();
    if operations.recurse {
        let parent_dir = list_path.parent().unwrap_or(Path::new("."));
        if let Err(err) = recursive::recurse_listfiles(parent_dir, config, 
                                                       &get_mode_string()) {
            eprintln!("Error during recursion: {}", err);
        }
    }
    
    Ok(())
}

/// Get the current mode string
fn get_mode_string() -> String {
    let operations = get_operations();
    if operations.list_lrel { return "list-lrel".to_string(); }
    if operations.list_rrel { return "list-rrel".to_string(); }
    if operations.list_rurl { return "list-rurl".to_string(); }
    if operations.clone { return "clone".to_string(); }
    if operations.configure { return "configure".to_string(); }
    if operations.set_remote { return "set-remote".to_string(); }
    if operations.git { return "git".to_string(); }
    if operations.new { return "new".to_string(); }
    "status".to_string() // default
}

/// Process a repository line from a listfile
fn process_repo_line(config: &mut Config, line: &str) -> Result<()> {
    // Skip comments and empty lines BEFORE splitting
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        eprintln!("DEBUG-SKIP: Skipping empty/comment line");
        return Ok(());
    }

    // Process line to extract fields
    let fields = split_with_escapes(line, LIST_SEPARATOR);
    
    
    // Handle config lines (first field is empty, indicating it starts with separator)
    if fields.len() >= 2 && fields[0].trim().is_empty() {
        // This is a config line
        if fields.len() >= 3 {
            // Format: * KEY * VALUE
            let key = fields[1].trim().to_string();
            let value = fields[2].trim().to_string();
            config.set_from_string(key, value);
        }
        return Ok(());
    }
    
    // Get repository paths from fields
    let remote_rel_raw = &fields[0];
    let local_rel_raw = if fields.len() > 1 { &fields[1] } else { "" };
    let gm_rel_raw = if fields.len() > 2 { &fields[2] } else { "" };
    
    // Extract repo name from remote path for default values
    let re = Regex::new(r"([^/]+?)(?:\.git)?$").unwrap();
    let repo_name = match re.captures(remote_rel_raw) {
        Some(caps) => caps.get(1).map_or("", |m| m.as_str()),
        None => "",
    };
    
    // Unescape all paths - do this once and store as String
    let remote_rel_unescaped = unescape_backslashes(remote_rel_raw);
    
    // Apply defaults and unescape
    let local_rel_unescaped = if local_rel_raw.is_empty() {
        repo_name.to_string()
    } else {
        unescape_backslashes(local_rel_raw)
    };
    
    let gm_rel_unescaped = if gm_rel_raw.is_empty() {
        repo_name.to_string()
    } else {
        unescape_backslashes(gm_rel_raw)
    };
    
    // Get directory values from config
    let local_dir = config.local_dir.as_deref().unwrap_or("");
    let gm_dir = config.gm_dir.as_deref().unwrap_or("");
    
    // Construct full paths
    let local_path = cat_path(&[local_dir, &local_rel_unescaped]);
    let media_path = cat_path(&[gm_dir, &gm_rel_unescaped]);
    
    if get_operations().debug {
        eprintln!("Potential target: {}", local_path);
    }
    
    // Process the repository
    if let Err(err) = process_repo(config, &local_path, &remote_rel_unescaped, &media_path) {
        eprintln!("Error processing {}: {}", local_path, err);
    }
    
    Ok(())
}

/// Split a line by separator character, respecting escaped separators
fn split_with_escapes(line: &str, separator: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    
    // Special handling for lines that start with the separator
    if let Some(&c) = chars.peek() {
        if c == separator {
            // If line starts with separator, add an empty string as first field
            result.push(String::new());
            chars.next(); // Consume the separator
        }
    }
    
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

/// Get formatted remote URL based on configuration and remote relative path
fn get_remote_url(config: &Config, remote_rel_path: &str) -> String {
    let remote_dir = config.remote_dir.as_deref().unwrap_or("");
    
    // Use our new remote_url module to build the URL
    match &config.rlogin {
        Some(login) if !login.is_empty() => {
            // We have login information, construct the URL with it
            remote_url::build_remote_url(Some(login), remote_dir, remote_rel_path)
        },
        _ => {
            // No login info, just use the remote directory and path
            remote_url::build_remote_url(None, remote_dir, remote_rel_path)
        }
    }
}

fn main() -> Result<()> {
    // Set MSYS_NO_PATHCONV=1 to prevent Windows Git path conversion issues
    std::env::set_var("MSYS_NO_PATHCONV", "1");
    
    // Parse command line arguments
    let args = Args::parse();
    
    // Create configuration
    let mut config = Config::new();
    
    // Load configuration from file
    let conf_path = find_conf_file(&config)?;
    config.load_from_file(&conf_path)?;
    
    // Load configuration from environment variables
    config.load_from_env();
    
    // Initialize operations
    initialize_operations(args.mode);
    
    // Store git command arguments if in git mode
    if args.mode.to_string() == "git" && !args.args.is_empty() {
        let git_args = args.args.join(" ");
        config.git_args = Some(git_args);
    }
    
    // Get listfile directory and path
    let list_dir = find_listfile_dir(&config)?;
    let list_path = list_dir.join(&config.list_filename);
    
    // Process listfile
    if list_path.exists() {
        if let Err(err) = process_listfile(&mut config, &list_path) {
            eprintln!("Error processing listfile: {}", err);
        }
    } else {
        eprintln!("No .grm.repos file found");
    }
    
    Ok(())
}

/// Find directory containing listfile by walking up from current directory
fn find_listfile_dir(config: &Config) -> Result<PathBuf> {
    let mut current_dir = env::current_dir()?;
    
    loop {
        let list_path = current_dir.join(&config.list_filename);
        if list_path.exists() {
            return Ok(current_dir);
        }
        
        if !current_dir.pop() {
            return Err(anyhow!("Could not find listfile {} in current directory or any ancestor", config.list_filename));
        }
    }
}
