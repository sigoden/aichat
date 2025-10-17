use serde::{Deserialize, Serialize};

/// Configuration for an MCP server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    /// Unique name for this server
    pub name: String,

    /// Command to execute to start the MCP server
    pub command: String,

    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set for the server process
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    /// Whether this server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Optional description of what this server provides
    #[serde(default)]
    pub description: Option<String>,
}

fn default_true() -> bool {
    true
}

impl McpServerConfig {
    /// Create a new MCP server configuration
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: vec![],
            env: Default::default(),
            enabled: true,
            description: None,
        }
    }

    /// Add an argument to the command
    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add multiple arguments to the command
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args.extend(args);
        self
    }

    /// Add an environment variable
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set whether the server is enabled
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}
