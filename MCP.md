# MCP (Model Context Protocol) Integration

AIChat supports the Model Context Protocol (MCP), allowing it to connect to external servers that provide tools, resources, and other capabilities.

## What is MCP?

The Model Context Protocol is an open standard that enables AI applications to securely connect to external data sources and tools. MCP servers expose capabilities that can be used by AI assistants to extend their functionality beyond their base capabilities.

For more information, visit: https://modelcontextprotocol.io

## Features

- **Automatic Tool Discovery**: MCP tools are automatically discovered when connecting to servers
- **Seamless Integration**: MCP tools work alongside local functions with no code changes
- **Name Conflict Prevention**: MCP tools are prefixed with `mcp__<server>__<tool>` to avoid conflicts
- **REPL Commands**: Manage MCP server connections interactively
- **Configuration-Driven**: Define MCP servers in your aichat config file

## Configuration

Add MCP servers to your `config.yaml`:

```yaml
mcp_servers:
  - name: filesystem
    command: npx
    args:
      - "-y"
      - "@modelcontextprotocol/server-filesystem"
      - "/tmp"
    enabled: true
    description: "File system operations"

  - name: git
    command: npx
    args:
      - "-y"
      - "@modelcontextprotocol/server-git"
    enabled: true
    description: "Git repository operations"
```

### Configuration Fields

- **name** (required): Unique identifier for the server
- **command** (required): Command to execute the MCP server
- **args** (optional): Arguments passed to the command
- **env** (optional): Environment variables for the server process
- **enabled** (optional, default: true): Whether to auto-connect on startup
- **description** (optional): Human-readable description

See `config.mcp.example.yaml` for more examples.

## Usage

### REPL Commands

```bash
# List all configured MCP servers
.mcp list

# Connect to a server
.mcp connect filesystem

# Disconnect from a server
.mcp disconnect filesystem

# List all available MCP tools
.mcp tools

# List tools from a specific server
.mcp tools filesystem
```

### Using MCP Tools

MCP tools are automatically available when function calling is enabled. They can be used just like local functions:

```bash
# Enable all tools (local + MCP)
aichat --role %functions% "read the file /tmp/example.txt"

# Use specific MCP tools in a role
# In your role definition:
use_tools: "mcp__filesystem__read_file,mcp__git__log"
```

### Tool Naming

MCP tools are prefixed to avoid name conflicts:
- Format: `mcp__<server>__<tool>`
- Example: `mcp__filesystem__read_file` (filesystem server's `read_file` tool)

## Available MCP Servers

Popular MCP servers you can use with aichat:

### Official Servers

- **@modelcontextprotocol/server-filesystem** - File system operations
- **@modelcontextprotocol/server-git** - Git repository operations
- **@modelcontextprotocol/server-sqlite** - SQLite database queries
- **@modelcontextprotocol/server-postgres** - PostgreSQL database queries
- **@modelcontextprotocol/server-github** - GitHub API integration
- **@modelcontextprotocol/server-slack** - Slack integration

### Community Servers

Visit https://github.com/modelcontextprotocol for more servers.

## Example Workflows

### File Operations

```yaml
# config.yaml
mcp_servers:
  - name: filesystem
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
    enabled: true
```

In REPL:
```
> Can you read and summarize the README.md file?
(AIChat will use mcp__filesystem__read_file to read the file)
```

### Git Operations

```yaml
# config.yaml
mcp_servers:
  - name: git
    command: npx
    args: ["-y", "@modelcontextprotocol/server-git"]
    enabled: true
```

In REPL:
```
> What commits were made in the last week?
(AIChat will use mcp__git__log to query git history)
```

### Database Queries

```yaml
# config.yaml
mcp_servers:
  - name: sqlite
    command: npx
    args: ["-y", "@modelcontextprotocol/server-sqlite", "/path/to/db.sqlite"]
    enabled: true
```

In REPL:
```
> How many users are in the database?
(AIChat will use mcp__sqlite__query to run SQL queries)
```

## Troubleshooting

### Server Won't Connect

1. Ensure the MCP server command is installed and in PATH
2. Check server logs (if available)
3. Verify arguments and environment variables are correct
4. Try connecting manually: `.mcp connect <server>`

### Tools Not Appearing

1. Verify server is connected: `.mcp list`
2. Check available tools: `.mcp tools <server>`
3. Ensure `function_calling` is enabled in config
4. Check `use_tools` configuration in your role

### Permission Errors

- Filesystem server: Ensure the specified directory is readable/writable
- Git server: Ensure you have access to the repository
- Database servers: Verify file permissions and credentials
