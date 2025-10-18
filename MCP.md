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
- **Tool Calling Permissions**: Fine-grained control over tool execution with support for permanent and runtime permissions

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
- **trusted** (optional, default: false): Whether to bypass all permission checks for tools from this server

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

## Tool Calling Permissions

AIChat provides a comprehensive permissions system to control tool execution, supporting both MCP tools and local functions. This system offers two patterns:
1. **Permanent permissions** via configuration file
2. **One-time "human-in-the-loop" permissions** during runtime

### Global Permission Level

Control all tool calls with a single setting:

```yaml
tool_call_permission: ask  # Options: always (default), ask, never
```

### Fine-Grained Tool Permissions

Define specific permissions for individual tools or patterns:

```yaml
tool_permissions:
  allowed:                     # Always allowed (no prompt)
    - fs_cat
    - fs_ls
    - mcp__filesystem__read_file
  denied:                      # Always denied
    - fs_rm
    - mcp__*__delete_*         # Supports wildcards
  ask:                         # Always prompt
    - fs_write
    - mcp__*__write_*
```

### Pattern Matching

Support for wildcard patterns:
- `fs_*` - matches all tools starting with "fs_"
- `mcp__*` - matches all MCP tools
- `mcp__filesystem__*` - matches all tools from filesystem server
- `mcp__*__write_*` - matches all write operations from any MCP server

### Trusted MCP Servers

Mark MCP servers as trusted to bypass permission checks:

```yaml
mcp_servers:
  - name: filesystem
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    enabled: true
    trusted: false  # Set to true to bypass all permission checks
```

**Use with caution**: Trusted servers bypass all permission checks for their tools.

### Session-Level Permissions

During a session, you can grant permissions that persist for that session only:

```
Tool call requested: mcp__filesystem__write_file

Allow this tool call?
> Yes (this time only)     # Allow once
  Yes (for this session)   # Allow for entire session
  No                       # Deny
```

Session permissions are automatically cleared when you exit the session.

### Runtime Configuration

Change permission settings at runtime:

```bash
.set tool_call_permission ask
.set verbose_tool_calls true
```

### Verbose Tool Calls

Enable verbose mode to see all tool calls, even when automatically allowed:

```yaml
verbose_tool_calls: true  # Print tool call info for all calls (default: false)
```

When enabled, you'll see output like:

```
🔧 Tool Call: mcp__filesystem__read_file [auto-allowed (allowed list)]
   Arguments: {
     "path": "/tmp/example.txt"
   }
```

This is useful for:
- Debugging which tools are being called
- Understanding permission decisions
- Monitoring AI assistant behavior
- Learning what tools the AI chooses to use

### Permission Examples

#### Example 1: Strict Security

```yaml
tool_call_permission: never
tool_permissions:
  allowed:
    - fs_cat
    - fs_ls
```

Only `fs_cat` and `fs_ls` are allowed; all other tools are denied.

#### Example 2: Interactive Mode

```yaml
tool_call_permission: ask
```

Every tool call will prompt for permission.

#### Example 3: Smart Defaults

```yaml
tool_permissions:
  allowed:
    - fs_cat
    - fs_ls
    - mcp__*__read_*   # All read operations allowed
  denied:
    - fs_rm
    - mcp__*__delete_* # All delete operations denied
```

Read operations are allowed, destructive operations are denied, and everything else prompts.

#### Example 4: Trusted Development Environment

```yaml
tool_call_permission: always
```

All tools are allowed without prompting (default behavior).

#### Example 5: Per-Server Trust

```yaml
# Trust filesystem server but not others
mcp_servers:
  - name: filesystem
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    enabled: true
    trusted: true    # All filesystem tools bypass permission checks

  - name: git
    command: npx
    args: ["-y", "@modelcontextprotocol/server-git"]
    enabled: true
    trusted: false   # Git tools still require permissions
```

### Permission Check Flow

When a tool is called, the following checks are performed in order:

1. Check if tool was already allowed in current session
2. Check if MCP tool is from a trusted server
3. Check specific tool permissions (denied → allowed → ask)
4. Fall back to global `tool_call_permission` setting
5. Prompt user if needed
6. Execute or deny based on result

### Security Considerations

- **Default behavior**: All tools allowed (backward compatible)
- **Trusted servers**: Use with caution - bypasses all checks
- **Pattern matching**: Test patterns carefully to avoid unintended matches
- **Session permissions**: Only persist for the current session
- **Permission denial**: Returns error message to LLM instead of executing

### Configuration Reference

#### Global Permission

```yaml
tool_call_permission: <level>
```

Values: `always`, `ask`, `never`

#### Tool Permissions

```yaml
tool_permissions:
  allowed: [<tools>]   # Always allowed
  denied: [<tools>]    # Always denied
  ask: [<tools>]       # Always prompt
```

#### Verbose Tool Calls

```yaml
verbose_tool_calls: <boolean>
```

Values: `true`, `false` (default)

When `true`, prints information about every tool call including:
- Tool name
- Permission decision (auto-allowed, denied, etc.)
- Arguments being passed to the tool

#### MCP Server Trust

```yaml
mcp_servers:
  - name: <name>
    trusted: <boolean>  # true to bypass permission checks
```

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

### Tool Permission Denied

If a tool call is denied by the permission system:
1. Check your `tool_call_permission` setting
2. Review `tool_permissions` configuration
3. Consider marking the server as `trusted: true` if you fully trust it
4. Grant session-level permission when prompted
