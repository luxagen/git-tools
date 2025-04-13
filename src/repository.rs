use std::fs;
use std::io::Write;
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
    let config_cmd = match config.get("CONFIG_CMD") {
        Some(cmd) => cmd.clone(),
        None => return Ok(()),  // Skip if no configure command
    };
    
    // Try to detect the shell environment
    let shell_cmd = detect_shell_command(&config_cmd)?;
    
    // Execute through the detected shell
    let status = Command::new(shell_cmd.executable)
        .args(&shell_cmd.args)
        .current_dir(local_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute config command in {}: {}", local_path, config_cmd))?;
    
    if !status.success() {
        return Err(anyhow!("Configuration command '{}' failed with exit code: {}", 
                          config_cmd, status.code().unwrap_or(-1)));
    }
    
    Ok(())
}

/// Shell command structure
struct ShellCommand {
    executable: String,
    args: Vec<String>,
}

/// Detect what shell to use based on environment
fn detect_shell_command(cmd: &str) -> Result<ShellCommand> {
    // First, try to use explicit shell environment variables
    if let Ok(shell_path) = std::env::var("SHELL") {
        // User has a SHELL variable defined, use it
        let shell_name = Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sh");
            
        // Check if it's a known shell and use appropriate args
        if shell_name.contains("bash") || shell_name == "sh" {
            return Ok(ShellCommand {
                executable: shell_path,
                args: vec!["-c".to_string(), cmd.to_string()],
            });
        } else if shell_name.contains("zsh") {
            return Ok(ShellCommand {
                executable: shell_path,
                args: vec!["-c".to_string(), cmd.to_string()],
            });
        } else if shell_name.contains("fish") {
            return Ok(ShellCommand {
                executable: shell_path,
                args: vec!["-c".to_string(), cmd.to_string()],
            });
        }
    }
    
    // Look for Git Bash on Windows
    if cfg!(windows) {
        // Check common Git Bash locations
        let git_bash_locations = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ];
        
        for path in git_bash_locations.iter() {
            if Path::new(path).exists() {
                return Ok(ShellCommand {
                    executable: path.to_string(),
                    args: vec!["-c".to_string(), cmd.to_string()],
                });
            }
        }
        
        // Check if bash is in PATH
        if let Ok(output) = Command::new("where").arg("bash").output() {
            if output.status.success() && !output.stdout.is_empty() {
                // Convert stdout to string first to fix the borrowing issue
                let output_str = String::from_utf8_lossy(&output.stdout).to_string();
                let bash_path = output_str.lines().next().unwrap_or("bash").trim();
                
                return Ok(ShellCommand {
                    executable: bash_path.to_string(),
                    args: vec!["-c".to_string(), cmd.to_string()],
                });
            }
        }
        
        // If all else fails on Windows, try PowerShell
        return Ok(ShellCommand {
            executable: "powershell".to_string(),
            args: vec!["-Command".to_string(), cmd.to_string()],
        });
    }
    
    // Default for Unix platforms
    Ok(ShellCommand {
        executable: "sh".to_string(),
        args: vec!["-c".to_string(), cmd.to_string()],
    })
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
    print!("About to create remote repo '{}'; are you sure? (y/n) ", grm_rpath);
    std::io::stdout().flush()?;
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
        // Use the same shell detection logic we implemented for configure_repo
        let shell_cmd = detect_shell_command(config_cmd)?;
        
        // Execute through the detected shell
        let status = Command::new(shell_cmd.executable)
            .args(&shell_cmd.args)
            .current_dir(local_path)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .with_context(|| format!("Failed to execute CONFIG_CMD: {}", config_cmd))?;
        
        if !status.success() {
            return Err(anyhow!("Config command failed with exit code: {:?}", status));
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

/// Run a git command in the repository
pub fn run_git_command(local_path: &str, args_str: &str) -> Result<()> {
    // Split the arguments string into individual arguments
    let args: Vec<&str> = args_str.split_whitespace().collect();
    
    // Construct the full command: git + args
    let mut cmd_args = vec!["git"];
    cmd_args.extend(args);
    
    // Run the git command in the repository directory
    let status = process::run_in_dir(local_path, &cmd_args)?;
    
    if status != 0 {
        return Err(anyhow!("Git command failed with exit code: {}", status));
    }
    
    Ok(())
}
