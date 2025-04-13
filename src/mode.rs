use clap::ValueEnum;
use once_cell::sync::Lazy;

/// Repository operation modes with explicit permissions and capabilities
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeConfig {
    /// Primary operation mode
    pub primary_mode: PrimaryMode,
    /// Optional operations to perform
    pub operations: Operations,
}

/// Primary operation modes that determine the main behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PrimaryMode {
    /// Clone repositories
    Clone,
    /// Execute git commands
    Git,
    /// Update remote URL
    #[clap(name = "set-remote")]
    SetRemote,
    /// Configure repositories 
    Configure,
    /// List remote relative paths
    #[clap(name = "list-rrel")]
    ListRrel,
    /// List remote URLs
    #[clap(name = "list-rurl")]
    ListRurl,
    /// List local relative paths
    #[clap(name = "list-lrel")]
    ListLrel,
    /// Run with clone and set-remote
    Run,
    /// Create new repositories
    New,
}

/// Operations that can be performed on repositories
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Operations {
    /// Clone the repository if it doesn't exist
    pub clone: bool,
    /// Configure the repository (run CONFIG_CMD)
    pub configure: bool,
    /// Update the remote URL
    pub set_remote: bool,
    /// Run git commands in the repository
    pub git: bool,
    /// Create a new repository
    pub new: bool,
    /// Debug mode
    pub debug: bool,
    /// List remote relative paths
    pub list_rrel: bool,
    /// List remote URLs
    pub list_rurl: bool,
    /// List local relative paths
    pub list_lrel: bool,
}

impl From<PrimaryMode> for Operations {
    fn from(mode: PrimaryMode) -> Self {
        let mut ops = Operations::default();
        match mode {
            PrimaryMode::Clone => {
                ops.clone = true;
                ops.configure = true;
            },
            PrimaryMode::Git => {
                ops.git = true;
                ops.set_remote = true;
                ops.configure = true;
            },
            PrimaryMode::SetRemote => {
                ops.set_remote = true;
            },
            PrimaryMode::Configure => {
                ops.configure = true;
            },
            PrimaryMode::ListRrel => {
                ops.list_rrel = true;
            },
            PrimaryMode::ListRurl => {
                ops.list_rurl = true;
            },
            PrimaryMode::ListLrel => {
                ops.list_lrel = true;
            },
            PrimaryMode::Run => {
                ops.clone = true;
                ops.set_remote = true;
                ops.configure = true;
            },
            PrimaryMode::New => {
                ops.new = true;
            },
        }
        ops
    }
}

impl std::fmt::Display for PrimaryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrimaryMode::Clone => write!(f, "clone"),
            PrimaryMode::Git => write!(f, "git"),
            PrimaryMode::SetRemote => write!(f, "set-remote"),
            PrimaryMode::Configure => write!(f, "configure"),
            PrimaryMode::ListRrel => write!(f, "list-rrel"),
            PrimaryMode::ListRurl => write!(f, "list-rurl"),
            PrimaryMode::ListLrel => write!(f, "list-lrel"),
            PrimaryMode::Run => write!(f, "run"),
            PrimaryMode::New => write!(f, "new"),
        }
    }
}

impl ModeConfig {
    /// Create a new ModeConfig from a primary mode
    pub fn new(primary_mode: PrimaryMode) -> Self {
        let operations = Operations::from(primary_mode);
        Self {
            primary_mode,
            operations,
        }
    }

    /// Check if this is a listing mode
    pub fn is_listing_mode(&self) -> bool {
        matches!(
            self.primary_mode,
            PrimaryMode::ListRrel | PrimaryMode::ListRurl | PrimaryMode::ListLrel
        )
    }
    
    /// Get a flag by its legacy name for compatibility with the old code
    pub fn get_flag(&self, flag_name: &str) -> bool {
        match flag_name {
            "MODE_CLONE" => self.operations.clone,
            "MODE_CONFIGURE" => self.operations.configure,
            "MODE_SET_REMOTE" => self.operations.set_remote,
            "MODE_GIT" => self.operations.git,
            "MODE_NEW" => self.operations.new,
            "MODE_DEBUG" => self.operations.debug,
            "MODE_LIST_RREL" => self.operations.list_rrel,
            "MODE_LIST_RURL" => self.operations.list_rurl,
            "MODE_LIST_LREL" => self.operations.list_lrel,
            _ => false,
        }
    }
}

/// Global MODE_CONFIG initialized once at startup
pub static MODE_CONFIG: Lazy<ModeConfig> = Lazy::new(|| {
    // Default to a safe no-op mode configuration - will be set during initialization
    ModeConfig::new(PrimaryMode::Configure)
});

/// Initialize the global mode configuration - call this ONCE at startup
pub fn initialize_mode(primary_mode: PrimaryMode) {
    // Use once_cell::sync::Lazy to initialize this only once
    // The initial value will be replaced on first access with the actual mode
    let mode_config = ModeConfig::new(primary_mode);
    
    // Safety: This function should only be called once during application startup
    // before any threads are spawned.
    unsafe {
        let ptr = &MODE_CONFIG as *const Lazy<ModeConfig> as *mut Lazy<ModeConfig>;
        std::ptr::write(ptr, Lazy::new(|| mode_config));
    }
}
