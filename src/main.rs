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
use config::{Config, ConfigLineIterator};

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

/// Process a single repository
fn process_repo(config: &Config, local_path: &str, remote_rel_path: &str, media_path: &str) -> Result<()> {
    // Use the recurse prefix directly from the config
    let prefixed_local_path = format!("{}{}", config.recurse_prefix, local_path);
    
    // Get operations
    let operations = get_operations();
    
    // Different behavior based on mode flags
    if operations.list_rrel {
        println!("{}", remote_rel_path.unwrap_or(""));
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
    
    // Helper to avoid duplicating unwrap_or for media path
    let configure_repo = |should_configure: bool| -> Result<()> {
        if should_configure {
            repository::configure_repo(local_path, media_path.unwrap_or(""), config)?;
        }
        Ok(())
    };
    
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
        configure_repo(true)?;
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
        
        configure_repo(operations.configure)?;
        
        if operations.git {
            // Execute git commands in the repository
            if !config.git_args.is_empty() {
                repository::run_git_command(local_path, &config.git_args)?;
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
    if path.exists() && operations.new {
        eprintln!("Creating new Git repository in {}", prefixed_local_path);
        
        repository::create_new(local_path, remote_rel_path, config)?;
        eprintln!("{} created", prefixed_local_path);
    } else {
        // Directory doesn't exist, just skip it
        eprintln!("{} does not exist (skipping)", prefixed_local_path);
    }
    
    Ok(())
}

/// Process a repository listfile
fn process_listfile(config: &mut Config, list_path: &Path) -> Result<()> {
    // Use ConfigLineIterator to handle file reading and line parsing
    let iter = ConfigLineIterator::from_file(list_path)?;
    
    // Process each parsed line
    for cells in iter {
        // Skip empty lines and comments (already handled by ConfigLineIterator)
        if cells.is_empty() {
            continue;
        }
        
        // Process the repository line cells
        if let Err(err) = process_repo_line(config, cells) {
            eprintln!("Error processing repository line: {}", err);
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

/// Process cells from a repository list file
fn process_repo_line(config: &mut Config, cells: Vec<String>) -> Result<()> {
    // Skip empty cell arrays (already handled by ConfigLineIterator)
    if cells.is_empty() {
        return Ok(());
    }
    
    // Skip comment lines where first non-empty cell starts with #
    for cell in &cells {
        if !cell.is_empty() {
            if cell.starts_with('#') {
                return Ok(());
            }
            break; // Found first non-empty cell that doesn't start with #
        }
    }
    
    // Handle config lines (first cell is empty, indicating it starts with separator)
    if cells[0].is_empty() {
        // This is a config line
        if cells.len() >= 3 {
            // Format: * KEY * VALUE
            config.set_from_string(&cells[1], cells[2].clone());
        }
        return Ok(());
    }
    
    // Process cells as repository specification with appropriate defaults
    process_repo_cells(config, cells)
}

struct RepoSpec<'a> {
    remote_rel: &'a str,
    local_rel: &'a str,
    media_rel: &'a str,
}

/// Process cells as a repository specification
fn process_repo_cells(config: &mut Config, cells: Vec<String>) -> Result<()> {
    // We already know cells is not empty from process_repo_line check
    
    // First cell is always the remote relative path
    let remote_rel = cells[0].clone();
    
    // Extract repo name from remote path for default values
    let re = Regex::new(r"([^/]+?)(?:\.git)?$").unwrap();
    let repo_name = match re.captures(&remote_rel) {
        Some(caps) => caps.get(1).map_or("", |m| m.as_str()).to_string(),
        None => String::new(),
    };
    
    // Second cell is local relative path, defaults to repo_name if empty or missing
    let local_rel = if cells.len() > 1 && !cells[1].is_empty() {
        cells[1].clone()
    } else {
        repo_name.clone()
    };
    
    // Third cell is media relative path, defaults to local_rel if empty or missing
    let media_rel = if cells.len() > 2 && !cells[2].is_empty() {
        cells[2].clone()
    } else {
        local_rel.clone()
    };
    
    // Create a RepoSpec with references to our strings
    let repo_spec = RepoSpec {
        remote_rel: &remote_rel,
        local_rel: &local_rel,
        media_rel: &media_rel,
    };
    
    // Get directory values from config using the RepoSpec
    let local_path = get_local_repo_path(config, repo_spec.local_rel);
    let media_path = get_media_repo_path(config, repo_spec.media_rel);
    
    // Filter out repositories that are not in or below the current directory
    let tree_filter = &config.tree_filter;
    if !tree_filter.is_empty() {
        // Get the absolute path from the current directory
        let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let abs_local_path = current_dir.join(&local_path);
        let abs_local_str = abs_local_path.to_string_lossy().replace('\\', "/");
        let tree_filter_str = tree_filter.replace('\\', "/");
        
        // Check if the absolute path contains our original directory
        if !abs_local_str.contains(&tree_filter_str) {
            if get_operations().debug {
                eprintln!("Skipping repository outside tree filter: {} (not in {})", &local_path, tree_filter_str);
            }
            return Ok(());
        }
    }
    
    if get_operations().debug {
        eprintln!("Potential target: {}", &local_path);
    }
    
    // Process the repository
    if let Err(err) = process_repo(config, &local_path, repo_spec.remote_rel, &media_path) {
        eprintln!("Error processing {}: {}", &local_path, err);
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

/// Get remote repository path from config and relative path
pub fn get_remote_repo_path(config: &Config, repo_path: &str) -> String {
    let remote_dir = &config.remote_dir;
    if !remote_dir.is_empty() {
        if !repo_path.is_empty() {
            return format!("{}/{}", remote_dir, repo_path);
        }
        return remote_dir.to_string();
    }
    repo_path.to_string()
}

/// Generate a complete media repository path by combining gm_dir and repo_path
pub fn get_media_repo_path(config: &Config, repo_path: &str) -> String {
    let gm_dir = &config.gm_dir;
    if !gm_dir.is_empty() {
        if !repo_path.is_empty() {
            return format!("{}/{}", gm_dir, repo_path);
        }
        return gm_dir.to_string();
    }
    repo_path.to_string()
}

/// Generate a complete local repository path by combining local_dir and repo_path
pub fn get_local_repo_path(config: &Config, repo_path: &str) -> String {
    let local_dir = &config.local_dir;
    if !local_dir.is_empty() {
        if !repo_path.is_empty() {
            return format!("{}/{}", local_dir, repo_path);
        }
        return local_dir.to_string();
    }
    repo_path.to_string()
}

/// Get formatted remote URL based on configuration and remote relative path
fn get_remote_url(config: &Config, remote_rel_path: &str) -> String {
    // Get the base path, defaulting to empty string if not set
    let base_path = &config.rpath_base;
    
    // Use the remote repo path function to handle paths consistently
    let full_repo_path = get_remote_repo_path(config, remote_rel_path);
    
    // Choose URL format based on configuration
    if !config.rlogin.is_empty() {
        // We have login information
        remote_url::build_remote_url(&config.rlogin, base_path, &full_repo_path)
    } else {
        // No login info
        remote_url::build_remote_url("", base_path, &full_repo_path)
    }
}

fn main() -> Result<()> {
    std::env::set_var("MSYS_NO_PATHCONV", "1");
    
    // Save the original working directory to use as a tree filter (like $treeFilter in Perl)
    let tree_filter = env::current_dir()?;
    let tree_filter_str = tree_filter.to_string_lossy().to_string();
    
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
        config.git_args = git_args;
    }
    
    // Get listfile directory and path
    let list_dir = find_listfile_dir(&config)?;
    let list_path = list_dir.join(&config.list_filename);
    
    // Just like Perl, change to the listfile directory - this simplifies path handling
    env::set_current_dir(&list_dir)?;
    
    // Store original working directory for filtering
    config.tree_filter = tree_filter_str;
    
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
