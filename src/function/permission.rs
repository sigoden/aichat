use super::ToolCall;
use crate::config::{GlobalConfig, ToolPermissions};
use crate::utils::{color_text, dimmed_text};
use anyhow::Result;
use fancy_regex::Regex;
use inquire::Select;
use nu_ansi_term::Color;
use std::collections::HashSet;
use std::sync::LazyLock;

static WILDCARD_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionLevel {
    Always, // Always allow without prompting
    Never,  // Always deny
    Ask,    // Prompt user each time
}

impl PermissionLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "always" => PermissionLevel::Always,
            "never" => PermissionLevel::Never,
            "ask" => PermissionLevel::Ask,
            _ => PermissionLevel::Ask, // Default to ask for safety
        }
    }
}

#[derive(Debug)]
pub struct ToolPermission {
    config: GlobalConfig,
    session_allowed: HashSet<String>,
    role_tool_call_permission: Option<String>,
    role_tool_permissions: Option<ToolPermissions>,
}

impl ToolPermission {
    pub fn new(config: &GlobalConfig) -> Self {
        // Load existing session permissions if in a session
        let session_allowed = config
            .read()
            .session
            .as_ref()
            .map(|s| s.get_session_tool_permissions().clone())
            .unwrap_or_default();

        Self {
            config: config.clone(),
            session_allowed,
            role_tool_call_permission: None,
            role_tool_permissions: None,
        }
    }

    pub fn new_with_role(
        config: &GlobalConfig,
        role_tool_call_permission: Option<String>,
        role_tool_permissions: Option<ToolPermissions>,
    ) -> Self {
        // Load existing session permissions if in a session
        let session_allowed = config
            .read()
            .session
            .as_ref()
            .map(|s| s.get_session_tool_permissions().clone())
            .unwrap_or_default();

        Self {
            config: config.clone(),
            session_allowed,
            role_tool_call_permission,
            role_tool_permissions,
        }
    }

    /// Check if a tool call is permitted
    pub fn check_permission(&mut self, tool_call: &ToolCall) -> Result<bool> {
        let tool_name = &tool_call.name;

        // Check if already allowed in this session
        if self.session_allowed.contains(tool_name) {
            // Print tool call info if verbose mode is enabled
            if self.config.read().verbose_tool_calls {
                self.print_tool_call_info(tool_call, "auto-allowed (session)");
            }
            return Ok(true);
        }

        let config = self.config.read();

        // Check if this is an MCP tool from a trusted server
        if tool_name.starts_with("mcp__") {
            if config.mcp_manager.is_some() {
                // Extract server name from tool name (format: mcp__<server>__<tool>)
                if let Some(server_name) = crate::mcp::extract_server_name(tool_name) {
                    // Check MCP server configs for trust status
                    if let Some(server_config) =
                        config.mcp_servers.iter().find(|s| s.name == server_name)
                    {
                        if server_config.trusted {
                            // Print tool call info if verbose mode is enabled
                            if config.verbose_tool_calls {
                                drop(config);
                                self.print_tool_call_info(
                                    tool_call,
                                    "auto-allowed (trusted server)",
                                );
                            }
                            return Ok(true);
                        }
                    }
                }
            }
        }

        // Get the default permission level (role-specific overrides global)
        let default_permission = if let Some(perm) = &self.role_tool_call_permission {
            PermissionLevel::from_str(perm)
        } else if let Some(perm) = &config.tool_call_permission {
            PermissionLevel::from_str(perm)
        } else {
            PermissionLevel::Always // Default behavior: always allow
        };

        // Check specific tool permissions (role-specific overrides global)
        let tool_perms = self
            .role_tool_permissions
            .as_ref()
            .or(config.tool_permissions.as_ref());
        if let Some(tool_perms) = tool_perms {
            // Check denied list first
            if let Some(denied) = &tool_perms.denied {
                if self.matches_any_pattern(tool_name, denied) {
                    if config.verbose_tool_calls {
                        drop(config);
                        self.print_tool_call_info(tool_call, "denied");
                    }
                    return Ok(false);
                }
            }

            // Check allowed list
            if let Some(allowed) = &tool_perms.allowed {
                if self.matches_any_pattern(tool_name, allowed) {
                    if config.verbose_tool_calls {
                        drop(config);
                        self.print_tool_call_info(tool_call, "auto-allowed (allowed list)");
                    }
                    return Ok(true);
                }
            }

            // Check ask list
            if let Some(ask) = &tool_perms.ask {
                if self.matches_any_pattern(tool_name, ask) {
                    drop(config); // Release lock before prompting
                    return self.prompt_user(tool_call);
                }
            }
        }

        let verbose = config.verbose_tool_calls;
        // Fall back to global permission setting
        drop(config);
        match default_permission {
            PermissionLevel::Always => {
                if verbose {
                    self.print_tool_call_info(tool_call, "auto-allowed (global)");
                }
                Ok(true)
            }
            PermissionLevel::Never => {
                if verbose {
                    self.print_tool_call_info(tool_call, "denied (global)");
                }
                Ok(false)
            }
            PermissionLevel::Ask => self.prompt_user(tool_call),
        }
    }

    /// Prompt the user for permission
    fn prompt_user(&mut self, tool_call: &ToolCall) -> Result<bool> {
        // Format arguments for display
        let args_display = if tool_call.arguments.is_object() {
            serde_json::to_string_pretty(&tool_call.arguments).unwrap_or_else(|_| "{}".to_string())
        } else {
            tool_call.arguments.to_string()
        };

        // Truncate long arguments
        let args_display = if args_display.len() > 200 {
            format!("{}... (truncated)", &args_display[..200])
        } else {
            args_display
        };
        println!();
        println!(
            "Can I run {} with the following arguments?\n{}",
            color_text(tool_call.name.as_str(), Color::Cyan),
            dimmed_text(args_display.as_str())
        );

        let options = vec!["Yes (this time only)", "Yes (for this session)", "No"];

        let choice = Select::new("Allow this tool call?", options)
            .with_help_message("Choose how to respond to this tool call")
            .prompt();

        match choice {
            Ok("Yes (this time only)") => Ok(true),
            Ok("Yes (for this session)") => {
                self.session_allowed.insert(tool_call.name.clone());

                // Save to session if we're in one
                if let Some(session) = self.config.write().session.as_mut() {
                    session.add_session_tool_permission(tool_call.name.clone());
                }

                Ok(true)
            }
            Ok("No") => Ok(false),
            _ => Ok(false), // Default to no on error or cancellation
        }
    }

    /// Check if a tool name matches any pattern in a list
    fn matches_any_pattern(&self, tool_name: &str, patterns: &[String]) -> bool {
        patterns
            .iter()
            .any(|pattern| self.matches_pattern(tool_name, pattern))
    }

    /// Check if a tool name matches a pattern (supports wildcards)
    fn matches_pattern(&self, tool_name: &str, pattern: &str) -> bool {
        if pattern == tool_name {
            return true;
        }

        // Convert glob pattern to regex
        if pattern.contains('*') {
            let regex_pattern = WILDCARD_PATTERN.replace_all(pattern, ".*");
            let regex_pattern = format!("^{}$", regex_pattern);
            if let Ok(re) = Regex::new(&regex_pattern) {
                if let Ok(is_match) = re.is_match(tool_name) {
                    return is_match;
                }
            }
        }

        false
    }

    /// Clear session permissions (call on session exit)
    pub fn clear_session_permissions(&mut self) {
        self.session_allowed.clear();
    }

    /// Check if a tool is permanently allowed in config
    pub fn is_permanently_allowed(&self, tool_name: &str) -> bool {
        let config = self.config.read();

        if let Some(tool_perms) = &config.tool_permissions {
            if let Some(allowed) = &tool_perms.allowed {
                return self.matches_any_pattern(tool_name, allowed);
            }
        }

        false
    }

    /// Print tool call information for verbose mode
    fn print_tool_call_info(&self, tool_call: &ToolCall, status: &str) {
        let prompt = format!(
            "Call {} {} [{}]",
            tool_call.name, tool_call.arguments, status
        );
        println!("{}", dimmed_text(&prompt));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matching() {
        let config = GlobalConfig::default();
        let permission = ToolPermission::new(&config);

        assert!(permission.matches_pattern("fs_cat", "fs_cat"));
        assert!(permission.matches_pattern("fs_cat", "fs_*"));
        assert!(permission.matches_pattern("mcp__filesystem__read", "mcp__*"));
        assert!(permission.matches_pattern("mcp__filesystem__write", "mcp__*__write*"));
        assert!(!permission.matches_pattern("fs_cat", "fs_ls"));
        assert!(!permission.matches_pattern("web_search", "fs_*"));
    }
}
