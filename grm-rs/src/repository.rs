use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, anyhow};
use crate::Config;
use crate::process;

/// Check if directory is a Git repository root
pub fn is_dir_repo_root(local_path: &str) -> Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-prefix"])
        .current_dir(local_path)
        .output()
        .with_context(|| format!("Failed to check if {} is a git repo root", local_path))?;
    
    if !output.status.success() {
        return Ok(false);
    }
    
    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(prefix.is_empty())
}

/// Clone a repository without checking it out
pub fn clone_repo_no_checkout(local_path: &str, remote_url: &str) -> Result<()> {
    println!("Cloning {} to {} (no checkout)", remote_url, local_path);
    
    // Ensure parent directory exists
    if let Some(parent) = Path::new(local_path).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directories for {}", local_path))?;
    }
    
    let status = process::run_sync_redir(&["git", "clone", "--no-checkout", remote_url, local_path])?;
    
    if status != 0 {
        return Err(anyhow!("Failed to clone repository with exit code: {}", status));
    }
    
    Ok(())
}

/// Configure a repository using the provided command
pub fn configure_repo(local_path: &str, media_path: &str, config: &Config) -> Result<()> {
    println!("Configuring repository at {} with media path {}", local_path, media_path);
    
    let config_cmd = match config.get("CONFIG_CMD") {
        Some(cmd) => cmd.clone(),
        None => return Ok(()),  // Skip if no configure command
    };
    
    // Split config command into args
    let args: Vec<&str> = config_cmd.split_whitespace().collect();
    
    if args.is_empty() {
        return Ok(());
    }
    
    let status = process::run_in_dir(local_path, &args)?;
    
    if status != 0 {
        return Err(anyhow!("Failed to configure repository with exit code: {}", status));
    }
    
    Ok(())
}

/// Update the remote URL for a repository
pub fn set_remote(local_path: &str, remote_url: &str) -> Result<()> {
    // Try to update the remote first
    let status = process::run_in_dir(local_path, &["git", "remote", "set-url", "origin", remote_url])?;
    
    // If remote update failed (exit code 3 for non-existent remote), try to add it
    if status != 0 {
        let status = process::run_in_dir(local_path, &["git", "remote", "add", "-f", "origin", remote_url])?;
        
        if status != 0 {
            return Err(anyhow!("Failed to add remote with exit code: {}", status));
        }
    }
    
    Ok(())
}

/// Checkout the default branch after cloning
pub fn check_out(local_path: &str) -> Result<()> {
    println!("Checking out repository at {}", local_path);
    
    // Reset to get the working directory in sync with remote
    let status = process::run_in_dir(local_path, &["git", "reset", "--hard"])?;
    
    if status != 0 {
        return Err(anyhow!("Failed to reset repository with exit code: {}", status));
    }
    
    Ok(())
}

/// Create a new repository 
pub fn create_new(local_path: &str, remote_path: &str, config: &Config) -> Result<()> {
    println!("Creating new repository at {} with remote {}", local_path, remote_path);
    
    // Check required configuration
    let rpath_template = config.get("RPATH_TEMPLATE")
        .ok_or_else(|| anyhow!("RPATH_TEMPLATE not set in configuration"))?;
    
    let rlogin = config.get("RLOGIN")
        .ok_or_else(|| anyhow!("RLOGIN not set in configuration"))?;
    
    let rpath_base = config.get("RPATH_BASE")
        .ok_or_else(|| anyhow!("RPATH_BASE not set in configuration"))?;
    
    // Parse SSH host
    let (ssh_host, effective_login) = if rlogin.is_empty() {
        ("localhost", "ssh://localhost".to_string())
    } else if let Some(host) = rlogin.strip_prefix("ssh://") {
        (host, rlogin.clone())
    } else {
        return Err(anyhow!("RLOGIN must be in format 'ssh://[user@]host' for SSH remote creation"));
    };
    
    // Check if current directory is already a git repo
    let virgin = !Path::new(local_path).join(".git").exists();
    
    // Construct remote path with .git extension
    let mut grm_rpath = format!("{}/{}", rpath_base, remote_path);
    if !grm_rpath.ends_with(".git") {
        grm_rpath.push_str(".git");
    }
    
    // Prompt for confirmation
    println!("About to create remote repo '{}'; are you sure? (y/n)", grm_rpath);
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("(aborted)");
        return Ok(());
    }
    
    // Create remote repo based on template
    let ssh_cmd = format!("xargs -0 -n 1 -- cp -na --reflink=auto '{}/{}'", rpath_base, rpath_template);
    
    let mut child = Command::new("ssh")
        .args([ssh_host, &ssh_cmd])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "Failed to spawn SSH command for repository creation")?;
    
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(format!("{}\0", grm_rpath).as_bytes())?;
    }
    
    let status = child.wait()?;
    if !status.success() {
        return Err(anyhow!("Remote repository creation failed with status: {:?}", status));
    }
    
    // Initialize git repository
    let status = process::run_in_dir(local_path, &["git", "init", "-q"])?;
    if status != 0 {
        return Err(anyhow!("Git init failed with exit code: {}", status));
    }
    
    // Configure the repository
    if let Some(config_cmd) = config.get("CONFIG_CMD") {
        let args: Vec<&str> = config_cmd.split_whitespace().collect();
        
        if !args.is_empty() {
            let status = process::run_in_dir(local_path, &args)?;
            if status != 0 {
                return Err(anyhow!("Config command failed with exit code: {}", status));
            }
        }
    }
    
    // Git remote URL
    let git_remote = format!("{}{}", effective_login, grm_rpath);
    
    // Check if remote exists
    let remote_exists = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(local_path)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    
    if remote_exists {
        // Update existing remote
        println!("Updating origin");
        let status = process::run_in_dir(local_path, &["git", "remote", "set-url", "origin", &git_remote])?;
        if status != 0 {
            println!("Warning: Failed to update remote URL, but continuing anyway");
        }
        
        let status = process::run_in_dir(local_path, &["git", "fetch", "origin"])?;
        if status != 0 {
            println!("Warning: git fetch failed with code {}", status);
        }
    } else {
        // Add new remote
        println!("Adding remote origin");
        let status = process::run_in_dir(local_path, &["git", "remote", "add", "origin", &git_remote])?;
        if status != 0 {
            return Err(anyhow!("Failed to add remote with exit code: {}", status));
        }
        
        let status = process::run_in_dir(local_path, &["git", "fetch", "origin"])?;
        if status != 0 {
            println!("Warning: git fetch failed with code {}, but remote was added", status);
        }
    }
    
    // Checkout master if this was a new repository
    if virgin {
        let status = process::run_in_dir(local_path, &["git", "checkout", "master"])?;
        if status != 0 {
            println!("Warning: git checkout master failed with code {}", status);
        }
    }
    
    println!("Repository created successfully");
    Ok(())
}
