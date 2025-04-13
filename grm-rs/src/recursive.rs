use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, anyhow};
use crate::Config;

/// Recursively process subdirectories, spawning new instances of the program
/// for directories containing listfiles
pub fn recurse_listfiles(dir: &Path, config: &Config, mode: &str) -> Result<()> {
    // Clean up the path before processing
    let dir_str = dir.to_string_lossy().to_string();
    let dir_str = dir_str.trim_end_matches('/');
    let dir_path = Path::new(dir_str);
    
    // Read directory entries
    let entries = fs::read_dir(dir_path)
        .with_context(|| format!("Failed to read directory: {}", dir_path.display()))?;
    
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        
        // Skip non-directories and hidden directories
        if !path.is_dir() || path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false) {
            continue;
        }
        
        let list_file_path = path.join(&config.list_filename);
        
        if list_file_path.exists() {
            // Recurse by spawning a new process
            recurse_to_subdirectory(&path, config, mode)?;
            
            // Skip further recursion - the spawned process will handle subdirectories
            continue;
        }
        
        // Continue recursing into this directory
        recurse_listfiles(&path, config, mode)?;
    }
    
    Ok(())
}

/// Spawn a new process to handle a subdirectory with a listfile
fn recurse_to_subdirectory(path: &Path, config: &Config, mode: &str) -> Result<()> {
    // Create a copy of environment variables for the child process
    let mut child_env: Vec<(String, String)> = env::vars().collect();
    
    // Remove any existing GRM_ variables
    child_env.retain(|(key, _)| !key.starts_with("GRM_"));
    
    // Get relative path for constructing the recurse prefix
    let current_dir = env::current_dir()?;
    let path_rel = if let Ok(rel_path) = path.strip_prefix(&current_dir) {
        rel_path.to_string_lossy().to_string()
    } else {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    
    // Set up new environment with config for the child process
    for (key, value) in config.all_values() {
        if key == "MODE" || key.starts_with("MODE_") {
            // Don't pass mode flags - these will be set by the child from command line
            continue;
        }
        
        // Set GRM_RECURSE_PREFIX to track hierarchy
        if key == "RECURSE_PREFIX" {
            let new_prefix = if config.recurse_prefix.is_empty() {
                format!("{}/", path_rel)
            } else {
                format!("{}{}/", config.recurse_prefix, path_rel)
            };
            
            child_env.push((format!("GRM_{}", key), new_prefix));
            continue;
        }
        
        // Add GRM_ prefix to all other config variables
        child_env.push((format!("GRM_{}", key), value.clone()));
    }
    
    // Always ensure RECURSE_PREFIX is set, even if it wasn't in the parent config
    if !child_env.iter().any(|(key, _)| key == "GRM_RECURSE_PREFIX") {
        child_env.push(("GRM_RECURSE_PREFIX".to_string(), format!("{}/", path_rel)));
    }
    
    // Get path to current executable
    let exe_path = env::current_exe()
        .context("Failed to get path to current executable")?;
    
    // Build command to execute in subdirectory
    let status = Command::new(exe_path)
        .arg(mode)
        .current_dir(path)
        .envs(child_env)
        .status()
        .with_context(|| format!("Failed to spawn recursive process in: {}", path.display()))?;
    
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(anyhow!("Recursive instance failed with exit code: {}", code));
    }
    
    Ok(())
}
