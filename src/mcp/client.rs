use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rmcp::model::CallToolRequestParam;
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

use super::config::McpServerConfig;
use super::convert::mcp_tool_to_function;
use crate::function::FunctionDeclaration;

/// Wrapper around an MCP client connection
pub struct McpClient {
    name: String,
    config: McpServerConfig,
    tools: Arc<RwLock<Vec<FunctionDeclaration>>>,
    connected: Arc<RwLock<bool>>,
    service: Arc<RwLock<Option<RunningService<RoleClient, ()>>>>,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("name", &self.name)
            .field("config", &self.config)
            .field("tools", &self.tools)
            .field("connected", &self.connected)
            .field("service", &"<MCP Service>")
            .finish()
    }
}

impl McpClient {
    /// Create a new MCP client (not yet connected)
    pub fn new(config: McpServerConfig) -> Self {
        let name = config.name.clone();
        Self {
            name,
            config,
            tools: Arc::new(RwLock::new(Vec::new())),
            connected: Arc::new(RwLock::new(false)),
            service: Arc::new(RwLock::new(None)),
        }
    }

    /// Get the server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if the client is connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Connect to the MCP server
    ///
    /// This will:
    /// 1. Start the MCP server process via child process transport
    /// 2. Initialize the service and get server info
    /// 3. Discover available tools
    /// 4. Convert tools to FunctionDeclarations
    pub async fn connect(&self) -> Result<()> {
        if *self.connected.read().await {
            return Ok(());
        }

        log::info!("Connecting to MCP server '{}'...", self.name);

        // Step 1: Create command for child process
        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args);

        // Set environment variables
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Step 2: Create transport and service
        let transport = TokioChildProcess::new(cmd).map_err(|e| {
            anyhow!(
                "Failed to create transport for MCP server '{}': {}",
                self.name,
                e
            )
        })?;

        // Allow an empty role to listen on the transport. This is a peculiarity
        // of the MCP library. Any type that implements the ServiceExt trait can
        // serve the transport. Since we don't need a custom type here, the unit
        // type is sufficient.
        let role = ();
        let service = role.serve(transport).await.map_err(|e| {
            anyhow!(
                "Failed to initialize MCP service for server '{}': {}",
                self.name,
                e
            )
        })?;

        log::info!("MCP server '{}' initialized successfully", self.name);

        // Get server info
        let server_info = service.peer_info();
        log::debug!("Connected to server: {:?}", server_info);

        // Step 3: Discover available tools
        let mut discovered_tools = Vec::new();

        log::debug!("Listing tools from MCP server '{}'...", self.name);

        match service.list_tools(Default::default()).await {
            Ok(tools_result) => {
                log::info!(
                    "Found {} tools from MCP server '{}'",
                    tools_result.tools.len(),
                    self.name
                );

                // Step 4: Convert tools to FunctionDeclarations
                for tool in tools_result.tools {
                    // Convert input_schema to JSON Value
                    let schema_value = serde_json::to_value(&tool.input_schema)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    match mcp_tool_to_function(
                        &self.name,
                        &tool.name,
                        &tool.description.unwrap_or_default(),
                        &schema_value,
                    ) {
                        Ok(func_decl) => {
                            log::debug!("Registered MCP tool: {}", func_decl.name);
                            discovered_tools.push(func_decl);
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to convert tool '{}' from server '{}': {}",
                                tool.name,
                                self.name,
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to list tools from MCP server '{}': {}",
                    self.name,
                    e
                );
            }
        }

        // Update internal state
        *self.tools.write().await = discovered_tools;
        *self.service.write().await = Some(service);
        *self.connected.write().await = true;

        log::info!(
            "Successfully connected to MCP server '{}' with {} tools",
            self.name,
            self.tools.read().await.len()
        );

        Ok(())
    }

    /// Disconnect from the MCP server
    pub async fn disconnect(&self) -> Result<()> {
        if !*self.connected.read().await {
            return Ok(());
        }

        log::info!("Disconnecting from MCP server '{}'...", self.name);

        // Gracefully shutdown the service
        if let Some(service) = self.service.write().await.take() {
            if let Err(e) = service.cancel().await {
                log::warn!("Error during shutdown of MCP server '{}': {}", self.name, e);
            }
        }

        *self.connected.write().await = false;
        *self.tools.write().await = Vec::new();

        log::info!("Disconnected from MCP server '{}'", self.name);

        Ok(())
    }

    /// Get the list of available tools from this server
    pub async fn get_tools(&self) -> Vec<FunctionDeclaration> {
        self.tools.read().await.clone()
    }

    /// Call a tool on this MCP server
    ///
    /// # Arguments
    /// * `tool_name` - The unprefixed tool name (without "mcp_<server>_" prefix)
    /// * `arguments` - JSON arguments for the tool
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        if !*self.connected.read().await {
            bail!("MCP server '{}' is not connected", self.name);
        }

        log::debug!(
            "Calling tool '{}' on MCP server '{}' with arguments: {}",
            tool_name,
            self.name,
            arguments
        );

        // Get the service
        let service = self.service.read().await;
        let service = service
            .as_ref()
            .ok_or_else(|| anyhow!("MCP service not initialized for server '{}'", self.name))?;

        // Prepare the call tool parameters
        let arguments_map = if let Value::Object(map) = arguments {
            Some(map)
        } else if arguments.is_null() {
            None
        } else {
            return Err(anyhow!(
                "Tool arguments must be a JSON object or null, got: {}",
                arguments
            ));
        };

        let params = CallToolRequestParam {
            name: tool_name.to_string().into(),
            arguments: arguments_map,
        };

        // Call the tool via MCP protocol
        let result = service.call_tool(params).await.map_err(|e| {
            anyhow!(
                "Failed to call tool '{}' on MCP server '{}': {}",
                tool_name,
                self.name,
                e
            )
        })?;

        log::debug!(
            "Tool '{}' on server '{}' returned successfully",
            tool_name,
            self.name
        );

        // Convert the result to JSON
        let result_json = serde_json::to_value(&result)
            .map_err(|e| anyhow!("Failed to serialize tool result: {}", e))?;

        Ok(result_json)
    }
}

/// Manager for MCP server connections
#[derive(Debug)]
pub struct McpManager {
    clients: Arc<RwLock<HashMap<String, Arc<McpClient>>>>,
}

impl McpManager {
    /// Create a new MCP manager
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize MCP servers from configurations
    pub async fn initialize(&self, configs: Vec<McpServerConfig>) -> Result<()> {
        let mut clients = self.clients.write().await;

        for config in configs {
            if !config.enabled {
                continue;
            }

            let name = config.name.clone();
            let client = Arc::new(McpClient::new(config));
            clients.insert(name, client);
        }

        Ok(())
    }

    /// Connect to a specific server
    pub async fn connect(&self, server_name: &str) -> Result<()> {
        let clients = self.clients.read().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| anyhow!("MCP server '{}' not found", server_name))?;

        client.connect().await
    }

    /// Connect to all enabled servers
    pub async fn connect_all(&self) -> Result<()> {
        let clients = self.clients.read().await;

        for client in clients.values() {
            if let Err(e) = client.connect().await {
                log::warn!("Failed to connect to MCP server '{}': {}", client.name(), e);
            }
        }

        Ok(())
    }

    /// Disconnect from a specific server
    pub async fn disconnect(&self, server_name: &str) -> Result<()> {
        let clients = self.clients.read().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| anyhow!("MCP server '{}' not found", server_name))?;

        client.disconnect().await
    }

    /// Disconnect from all servers
    pub async fn disconnect_all(&self) -> Result<()> {
        let clients = self.clients.read().await;

        for client in clients.values() {
            if let Err(e) = client.disconnect().await {
                log::warn!(
                    "Failed to disconnect from MCP server '{}': {}",
                    client.name(),
                    e
                );
            }
        }

        Ok(())
    }

    /// Get all available tools from all connected servers
    pub async fn get_all_tools(&self) -> Vec<FunctionDeclaration> {
        let clients = self.clients.read().await;
        let mut tools = Vec::new();

        for client in clients.values() {
            if client.is_connected().await {
                tools.extend(client.get_tools().await);
            }
        }

        tools
    }

    /// Get tools from a specific server
    pub async fn get_server_tools(&self, server_name: &str) -> Result<Vec<FunctionDeclaration>> {
        let clients = self.clients.read().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| anyhow!("MCP server '{}' not found", server_name))?;

        Ok(client.get_tools().await)
    }

    /// Call a tool on the appropriate MCP server
    ///
    /// # Arguments
    /// * `prefixed_name` - The full tool name including "mcp_<server>_" prefix
    /// * `arguments` - JSON arguments for the tool
    pub async fn call_tool(&self, prefixed_name: &str, arguments: Value) -> Result<Value> {
        // Parse the prefixed name to extract server name and tool name
        let parts: Vec<&str> = prefixed_name
            .strip_prefix("mcp__")
            .ok_or_else(|| anyhow!("Invalid MCP tool name: {}", prefixed_name))?
            .splitn(2, "__")
            .collect();

        if parts.len() != 2 {
            bail!("Invalid MCP tool name format: {}", prefixed_name);
        }

        let server_name = parts[0];
        let tool_name = parts[1];

        let clients = self.clients.read().await;
        let client = clients
            .get(server_name)
            .ok_or_else(|| anyhow!("MCP server '{}' not found", server_name))?;

        client.call_tool(tool_name, arguments).await
    }

    /// List all configured servers with their status
    pub async fn list_servers(&self) -> Vec<(String, bool, Option<String>)> {
        let clients = self.clients.read().await;
        let mut servers = Vec::new();

        for (name, client) in clients.iter() {
            let connected = client.is_connected().await;
            let description = client.config.description.clone();
            servers.push((name.clone(), connected, description));
        }

        servers.sort_by(|a, b| a.0.cmp(&b.0));
        servers
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
