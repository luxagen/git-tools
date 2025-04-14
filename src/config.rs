use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, anyhow};

/// Typed configuration values with proper types for each setting
#[derive(Debug, Clone)]
pub struct Config {
    /// Configuration filename (.grm.conf by default)
    pub config_filename: String,
    /// List filename (.grm.repos by default)
    pub list_filename: String,
    /// Whether recursion is enabled (1 by default)
    pub recurse_enabled: bool,
    /// Remote login information (e.g., ssh://user@host)
    pub rlogin: Option<String>,
    /// Remote path base directory
    pub rpath_base: Option<String>,
    /// Remote path template for new repositories
    pub rpath_template: Option<String>,
    /// Local base directory for repositories
    pub local_dir: Option<String>,
    /// Media base directory
    pub gm_dir: Option<String>,
    /// Remote directory
    pub remote_dir: Option<String>,
    /// Git arguments when in git mode
    pub git_args: Option<String>,
    /// Command to execute for configuration
    pub config_cmd: Option<String>,
    /// Recurse prefix for path display
    pub recurse_prefix: String,
    /// Tree filter path for filtering repositories to current subtree
    pub tree_filter: Option<String>,
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
            tree_filter: None,
        }
    }
    
    /// Get all configuration values as string key-value pairs (for environment variable passing)
    pub fn all_values(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        
        // Add all values with their string representations
        result.push(("CONFIG_FILENAME".to_string(), self.config_filename.clone()));
        result.push(("LIST_FN".to_string(), self.list_filename.clone()));
        result.push(("OPT_RECURSE".to_string(), if self.recurse_enabled { "1".to_string() } else { String::new() }));
        
        if let Some(ref v) = self.rlogin {
            result.push(("RLOGIN".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.rpath_base {
            result.push(("RPATH_BASE".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.rpath_template {
            result.push(("RPATH_TEMPLATE".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.local_dir {
            result.push(("LOCAL_DIR".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.gm_dir {
            result.push(("GM_DIR".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.remote_dir {
            result.push(("REMOTE_DIR".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.git_args {
            result.push(("GIT_ARGS".to_string(), v.clone()));
        }
        
        if let Some(ref v) = self.config_cmd {
            result.push(("CONFIG_CMD".to_string(), v.clone()));
        }
        
        if !self.recurse_prefix.is_empty() {
            result.push(("RECURSE_PREFIX".to_string(), self.recurse_prefix.clone()));
        }
        
        if let Some(ref v) = self.tree_filter {
            result.push(("TREE_FILTER".to_string(), v.clone()));
        }
        
        result
    }
    
    /// Load configuration from environment variables starting with GRM_
    pub fn load_from_env(&mut self) {
        // Check if this is a recursive invocation and set the recurse_prefix
        if let Ok(prefix) = std::env::var("GRM_RECURSE_PREFIX") {
            self.recurse_prefix = prefix;
        } else {
            self.recurse_prefix = String::new();
        }
        
        // Determine if we are in a recursive call for permission checking
        let is_recursive = !self.recurse_prefix.is_empty();
        
        for (key, value) in std::env::vars() {
            if let Some(conf_key) = key.strip_prefix("GRM_") {
                // For root process, only allow specific variables from environment
                if !is_recursive {
                    match conf_key {
                        "CONFIG_FILENAME" | "LIST_FN" | "CONFIG_CMD" => {
                            // These are allowed from environment for root process
                        },
                        _ => {
                            // All other variables are not allowed for root process
                            continue;
                        }
                    }
                }
                
                // Set configuration value
                self.set_from_string(conf_key.to_string(), value);
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
                self.set_from_string(key, value);
            }
        }
        
        Ok(())
    }
    
    /// Set a configuration value from string key and value
    pub fn set_from_string(&mut self, key: String, value: String) {
        match key.as_str() {
            "CONFIG_FILENAME" => self.config_filename = value,
            "LIST_FN" => self.list_filename = value,
            "OPT_RECURSE" => self.recurse_enabled = !value.is_empty(),
            "RLOGIN" => self.rlogin = Some(value),
            "RPATH_BASE" => self.rpath_base = Some(value),
            "RPATH_TEMPLATE" => self.rpath_template = Some(value),
            "LOCAL_DIR" => self.local_dir = Some(value),
            "GM_DIR" => self.gm_dir = Some(value),
            "REMOTE_DIR" => self.remote_dir = Some(value),
            "GIT_ARGS" => self.git_args = Some(value),
            "CONFIG_CMD" => self.config_cmd = Some(value),
            "RECURSE_PREFIX" => self.recurse_prefix = value,
            "TREE_FILTER" => self.tree_filter = Some(value),
            _ => {} // Ignore unknown keys
        }
    }
}

/// Parse a configuration line
fn parse_config_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    
    // Skip empty lines and comments
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    
    // Split by the first non-whitespace character
    let parts: Vec<&str> = line.splitn(3, '*').collect();
    
    if parts.len() < 3 {
        return None;
    }
    
    // First part should be empty or just whitespace
    let first = parts[0].trim();
    if !first.is_empty() {
        return None;
    }
    
    // Second part is the key
    let key = parts[1].trim();
    
    // Third part is the value
    let value = parts[2].trim();
    
    Some((key.to_string(), value.to_string()))
}

/// Skip leading whitespace in the input string (excluding CR and LF).
/// Returns the remaining string starting at the first non-whitespace character, newline, or end of string
fn skip_whitespace(input: &str) -> &str {
    let mut input = input;
    
    // Skip leading whitespace (excluding CR and LF) until we find non-whitespace or newline
    loop {
        input = match input.chars().next() {
            // Found regular whitespace (not CR or LF)
            Some(c) if c.is_whitespace() && c != '\r' && c != '\n' => {
                &input[c.len_utf8()..]
            },
            // Found CR, LF, other non-whitespace, or end of string
            _ => return input,
        };
    }
}

/// Parse a cell (column) from a line in the config/repos file.
/// Returns a tuple containing:
/// - Option<String>: The parsed cell content trimmed of whitespace (if found)
/// - &str: The remaining unparsed portion of the input string
///
/// The function works by:
/// - Skipping leading whitespace until a newline or end of string is encountered
/// - Collecting characters until a newline or end of string is encountered
/// - Trimming trailing whitespace from the right of the parsed string
pub fn parse_cell(input: &str) -> (Option<String>, &str) {
    // Skip leading whitespace
    let input = skip_whitespace(input);
    
    // If we hit a newline, CR, or empty string while skipping whitespace
    if input.is_empty() || input.starts_with('\n') || input.starts_with('\r') {
        return (None, input);
    }
    
    // Start building the cell content
    let mut cell = String::new();
    let mut input = input;
    
    // Process one character at a time, handling escapes
    while let Some(c) = input.chars().next() {
        // Remove the current character from input
        input = &input[c.len_utf8()..];
        
        // Handle escaping
        if c == '\\' && !input.is_empty() {
            // Get the escaped character
            let escaped = input.chars().next().unwrap();
            // Add the escaped character to the cell (not the backslash)
            cell.push(escaped);
            // Advance past the escaped character
            input = &input[escaped.len_utf8()..];
        } else if c == '\n' || c == '\r' {
            // Found a newline (LF or CR)
            
            // Handle CRLF by consuming the LF if this is CR followed by LF
            if c == '\r' && input.starts_with('\n') {
                input = &input['\n'.len_utf8()..];
            }
            
            // Don't include the line break in the cell
            break;
        } else {
            // Normal character - add it to the cell
            cell.push(c);
        }
    }
    
    // Return the cell with trailing whitespace trimmed, and the remaining input
    (Some(cell.trim_end().to_string()), input)
}
