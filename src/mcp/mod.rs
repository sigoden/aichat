//! Model Context Protocol (MCP) client implementation
//!
//! This module provides MCP client functionality, allowing aichat to connect
//! to MCP servers and use their tools alongside local functions.
//!
//! # Architecture
//!
//! - `McpServerConfig`: Configuration for MCP servers (command, args, etc.)
//! - `McpClient`: Wrapper around a single MCP server connection
//! - `McpManager`: Manages multiple MCP server connections
//! - Schema conversion utilities to translate MCP tools to FunctionDeclarations
//!
//! # Usage
//!
//! MCP servers are configured in the aichat config file:
//!
//! ```yaml
//! mcp_servers:
//!   - name: filesystem
//!     command: npx
//!     args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
//!     enabled: true
//! ```
//!
//! Tools from MCP servers are automatically discovered and prefixed with
//! `mcp__<server>__<tool>` to avoid name conflicts. Double underscores are
//! used as sentinel markers around the server name to support server names
//! that contain underscores.

mod client;
mod config;
mod convert;

pub use client::McpManager;
pub use config::McpServerConfig;

// Re-export for future use when implementing full MCP protocol
#[allow(unused_imports)]
use convert::mcp_tool_to_function;

/// Check if a tool name is an MCP tool (starts with "mcp__")
pub fn is_mcp_tool(name: &str) -> bool {
    name.starts_with("mcp__")
}

/// Extract the server name from an MCP tool name
///
/// For example, "mcp__filesystem__read_file" -> Some("filesystem")
/// The format is: mcp__<server_name>__<tool_name>
pub fn extract_server_name(tool_name: &str) -> Option<String> {
    let without_prefix = tool_name.strip_prefix("mcp__")?;
    let end_of_server = without_prefix.find("__")?;
    Some(without_prefix[..end_of_server].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_mcp_tool() {
        assert!(is_mcp_tool("mcp__filesystem__read_file"));
        assert!(is_mcp_tool("mcp__git__log"));
        assert!(is_mcp_tool("mcp__my_server__tool_name"));
        assert!(!is_mcp_tool("mcp_filesystem_read_file")); // old format
        assert!(!is_mcp_tool("read_file"));
        assert!(!is_mcp_tool("git_log"));
    }

    #[test]
    fn test_extract_server_name() {
        assert_eq!(
            extract_server_name("mcp__filesystem__read_file"),
            Some("filesystem".to_string())
        );
        assert_eq!(
            extract_server_name("mcp__git__log"),
            Some("git".to_string())
        );
        // Test server names with underscores
        assert_eq!(
            extract_server_name("mcp__my_server__tool_name"),
            Some("my_server".to_string())
        );
        assert_eq!(extract_server_name("read_file"), None);
        assert_eq!(extract_server_name("mcp_old_format"), None); // old format should fail
    }
}
