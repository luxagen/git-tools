#![allow(unused_imports)]

use std::env;
use std::f32::consts::E;
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

use mode::{PrimaryMode, initialize_operations, get_operations, get_mode_string};
use config::{Config, ConfigLineIterator};

/// Separator character used in listfiles
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
fn process_repo(config: &Config, repo: &RepoTriple) -> Result<()> {
    // Get operations
    let operations = get_operations();
    
    // Handle list_rrel first since it needs the original repo.remote
    if operations.list_rrel {
        println!("{}", repo.remote); // Use original repo.remote for relative path
        return Ok(());
    }
    
    if operations.list_lrel {
        println!("{}", repo.local); // NEEDS NOTHING
        return Ok(());
    }

    // Get local path info
    let path = Path::new(repo.local);

    let is_repo = match repository::is_dir_repo_root(repo.local) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Error checking if {} is a Git repository: {}", repo.local, err);
            return Ok(());
        }
    };

    let mut needs_checkout = false;

    if path.exists() {
        if path.is_dir() {
            if is_repo {
                // output: exists

                if operations.git {
                    // Execute git commands in the repository
                    repository::run_git_command(repo.local, &config.git_args)?; // NEEDS NOTHING
                }            
            }
            else {
                if operations.new {
                    // Create new repo
                    eprintln!("Creating new Git repository in {}", repo.local);
                    needs_checkout = repository::create_new(repo, config)?;  // NEEDS RREL
                    
                    eprintln!("{} created", repo.local);
                }
            }
        }
        else {
            // complain: not a dir
        }
    }
    else {
        if operations.clone {
            repository::clone_repo_no_checkout(&repo)?; // NEEDS RURL
            needs_checkout = true;
        }

        // complain?
    }

    if operations.list_rurl {
        println!("{}", repo.remote);  // NEEDS RURL
        return Ok(());
    }

//    if operations.new {
//        if !path.exists() {
//            eprintln!("ERROR: {} does not exist", repo.local);
//            return Ok(());
//        }
//        
//        if !path.is_dir() {
//            eprintln!("ERROR: {} is not a directory", repo.local);
//            return Ok(());
//        }
//
//        let is_repo = match repository::is_dir_repo_root(repo.local) {
//            Ok(result) => result,
//            Err(err) => {
//                eprintln!("Error checking if {} is a Git repository: {}", repo.local, err);
//                return Ok(());
//            }
//        };
//        
//        if is_repo {
//            eprintln!("{} already exists (skipping)", repo.local);
//            return Ok(());
//        }
//        
//        // Create new repo
//        eprintln!("Creating new Git repository in {}", repo.local);
//        needs_checkout = repository::create_new(repo, config)?;
//        
//        eprintln!("{} created", repo.local);
//    }

    // For all other operations, use the remote URL instead of the relative path
    // Redefine repo to use the URL version for other operations
    let repo = RepoTriple {
        remote: &get_remote_url(config, repo.remote),
        local: repo.local,
        media: repo.media,
    };

//    // Process based on path state
//    if !path.exists() {
//        // Only clone if clone operation is enabled
//        if !operations.clone {
//            eprintln!("ERROR: {} does not exist", repo.local);
//            return Ok(());
//        }
//
//        // Clone, configure, and checkout
//        repository::clone_repo_no_checkout(&repo)?;
//        needs_configure = true;
//        needs_checkout = true;
//    }

//    // Check if path is a directory
//    if !path.is_dir() {
//        eprintln!("ERROR: {} is not a directory", repo.local);
//        return Ok(());
//    }
    
    // Check if directory is a git repository
//    let is_repo = match repository::is_dir_repo_root(repo.local) {
//        Ok(result) => result,
//        Err(err) => {
//            eprintln!("Error checking if {} is a Git repository: {}", repo.local, err);
//            return Ok(());
//        }
//    };

//    if !is_repo {
//        // If we get here, the directory exists but isn't a git repository
//        eprintln!("ERROR: {} is not a Git repository", repo.local);
//        return Ok(());
//    }

    // Get remote URL
//    if !operations.new { // Don't print this for new repos (already did)
//        eprintln!("{} exists", repo.local);
//    }

    // Configure first, then update remote
    if operations.configure || needs_configure {
        repository::configure_repo(&repo, config)?;
    }

    if operations.set_remote {
        repository::set_remote(&repo)?;
    }

    // Checkout master if needed (for new repositories)
    if needs_checkout {
        repository::check_out(repo.local)?;
    }

    return Ok(());
}

/// Process a repository listfile
fn process_listfile(config: &mut Config, list_path: &Path) -> Result<()> {
    // Use ConfigLineIterator to handle file reading and line parsing
    let iter = ConfigLineIterator::from_file(list_path)?;
    
    // Process each parsed line
    for line_result in iter {
        // Handle parsing errors
        let cells = match line_result {
            Ok(cells) => cells,
            Err(err) => {
                eprintln!("Error parsing line: {}", err);
                continue;
            }
        };
        
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
        if let Err(err) = recursive::recurse_listfiles(parent_dir, config, mode::get_mode_string()) {
            eprintln!("Error during recursion: {}", err);
        }
    }
    
    Ok(())
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
            let key = cells[1].clone();
            let value = cells[2].clone();
            // Format: * KEY * VALUE
            config.set_from_string(&key, value);
        }
        return Ok(());
    }
    
    // Extract raw path components from cells
    let (raw_remote, raw_local, raw_media) = extract_repo_components(&cells);
    
    // Create raw repo specification with extracted components
    let raw_spec = OwnedRepoSpec {
        remote: raw_remote,
        local: raw_local,
        media: raw_media,
    };
    
    // Resolve paths by applying prefixes based on config settings
    let resolved_spec = resolve_repo_paths(config, &raw_spec);
    
    // Create a repo triple that borrows from our resolved spec
    // This is safe because resolved_spec lives for the rest of this function
    let repo_spec = RepoTriple {
        remote: &resolved_spec.remote,
        local: &resolved_spec.local,
        media: &resolved_spec.media,
    };
    
    // Filter out repositories that are not in or below the current directory
    if !passes_tree_filter(&config.tree_filter, repo_spec.local) {
        return Ok(());
    }
    
    if get_operations().debug {
        eprintln!("Potential target: {}", repo_spec.local);
    }
    
    // Process the repository
    if let Err(err) = process_repo(config, &repo_spec) {
        eprintln!("Error processing {}: {}", repo_spec.local, err);
    }
    
    Ok(())
}

// Create a struct to represent repository specifications with owned strings
#[derive(Debug, Clone)]
struct OwnedRepoSpec {
    remote: String,
    local: String,
    media: String,
}

// Use the shared RepoTriple from repository.rs
use crate::repository::RepoTriple;

/// Check if a repository local path passes the tree filter
/// Returns true if there is no filter or if the path is within the filter
fn passes_tree_filter(tree_filter: &str, local_path: &str) -> bool {
    // If there's no tree filter, all paths pass
    if tree_filter.is_empty() {
        return true;
    }
    
    // Get the absolute path from the current directory
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let abs_local_path = current_dir.join(local_path);
    let abs_local_str = abs_local_path.to_string_lossy().replace('\\', "/");
    let tree_filter_str = tree_filter.replace('\\', "/");
    
    // Check if the absolute path contains our filter string
    let passes = abs_local_str.contains(&tree_filter_str);
    
    if !passes && get_operations().debug {
        eprintln!("Skipping repository outside tree filter: {} (not in {})", local_path, tree_filter_str);
    }
    
    passes
}


/// Extract raw repository path components from config file cells
fn extract_repo_components(cells: &Vec<String>) -> (String, String, String) {
    // First cell is always the remote relative path
    let remote_rel = cells[0].clone();
    
    // Second cell is local relative path, defaults to repo_name if empty or missing
    let local_rel = if cells.len() > 1 && !cells[1].is_empty() {
        cells[1].clone()
    } else {
        // Extract repo name from remote path for default values
        let re = Regex::new(r"([^/]+?)(?:\.git)?$").unwrap();
        match re.captures(&remote_rel) {
            Some(caps) => caps.get(1).map_or(String::new(), |m| m.as_str().to_string()),
            None => String::new(),
        }
    };
    
    // Third cell is media relative path, defaults to local_rel if empty or missing
    let media_rel = if cells.len() > 2 && !cells[2].is_empty() {
        cells[2].clone()
    } else {
        local_rel.clone()
    };
    
    (remote_rel, local_rel, media_rel)
}

/// Apply path transformations to raw repository paths based on configuration settings
fn resolve_repo_paths(config: &Config, raw_spec: &OwnedRepoSpec) -> OwnedRepoSpec {
    // Apply appropriate prefixes to each path based on config settings
    let remote_path = cat_paths(&config.remote_dir, &raw_spec.remote);
    let local_path = cat_paths(&config.local_dir, &raw_spec.local);
    let media_path = cat_paths(&config.gm_dir, &raw_spec.media);
    
    // Return new spec with fully resolved paths
    OwnedRepoSpec {
        remote: remote_path,
        local: local_path,
        media: media_path,
    }
}

pub fn cat_paths(base: &str, rel: &str) -> String {
    // Absolute paths remain unchanged
    if rel.starts_with('/') || base.is_empty() {
        return rel.to_string();
    }

    // Relative paths get base prefix if applicable
    if !rel.is_empty() {
        format!("{}/{}", base, rel)
    } else {
        base.to_string()
    }
}

/// Get formatted remote URL based on configuration and remote relative path
fn get_remote_url(config: &Config, remote_rel_path: &str) -> String {
    // Get the base path, defaulting to empty string if not set
    let base_path = &config.rpath_base;
    
    // Use cat_paths to handle paths consistently
    let full_repo_path = cat_paths(&config.remote_dir, remote_rel_path);
    
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

    // Require LIST_FN (list_filename) to be set after config processing
    if config.list_filename.is_empty() {
        return Err(anyhow!("LIST_FN must be set in {}", config.list_filename));
    }
    
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
        eprintln!("No listfile found");
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
