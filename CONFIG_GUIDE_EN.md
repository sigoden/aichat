# AIChat Complete Configuration Guide

## Configuration Directory Structure

```
~/.config/aichat/
├── config.yaml              # Main configuration file
├── .env                     # Environment variables file
├── models-override.yaml     # Models override file
├── messages.md              # Messages file
├── roles/                   # Roles directory
│   └── *.md                # Role files
├── sessions/                # Sessions directory
│   └── *.yaml              # Session files
├── rags/                    # RAG directory
│   └── *.yaml              # RAG configuration files
├── macros/                  # Macros directory
│   └── *.yaml              # Macro files
├── functions/               # Functions directory
│   ├── functions.json      # Functions definition file
│   ├── bin/                # Functions binary directory
│   └── agents/             # Agents functions directory
│       ├── agents.txt      # Agents registry file
│       └── <agent-name>/   # Individual agent directories
│           └── index.yaml  # Agent definition file
└── agents/                 # Agents data directory
    └── <agent-name>/       # Individual agent data directories
        ├── config.yaml     # Agent configuration file
        ├── sessions/       # Agent sessions directory
        └── messages.md     # Agent messages file
```

## Environment Variable Overrides

All configuration paths can be overridden by environment variables:

### Main Configuration Environment Variables
```bash
# Configuration directory
AICHAT_CONFIG_DIR=/custom/path

# Specific file paths
AICHAT_CONFIG_FILE=/custom/config.yaml
AICHAT_ENV_FILE=/custom/.env
AICHAT_MESSAGES_FILE=/custom/messages.md
AICHAT_LOG_PATH=/custom/aichat.log

# Directory paths
AICHAT_ROLES_DIR=/custom/roles
AICHAT_SESSIONS_DIR=/custom/sessions
AICHAT_RAGS_DIR=/custom/rags
AICHAT_MACROS_DIR=/custom/macros
AICHAT_FUNCTIONS_DIR=/custom/functions
```

### Agent-Specific Environment Variables
```bash
# Agent data directory
<AGENT_NAME>_DATA_DIR=/custom/agent-data

# Agent configuration file
<AGENT_NAME>_CONFIG_FILE=/custom/agent-config.yaml

# Agent functions directory
<AGENT_NAME>_FUNCTIONS_DIR=/custom/agent-functions
```

Note: `<AGENT_NAME>` should be converted to uppercase with hyphens replaced by underscores, e.g., `code-assistant` becomes `CODE_ASSISTANT`

## Configuration Files Detailed

### 1. Main Configuration File (config.yaml)

```yaml
# ---- LLM Configuration ----
model: openai:gpt-4o             # Specify the LLM to use
temperature: null                # Default temperature parameter (0-1)
top_p: null                      # Default top-p parameter

# ---- Behavior Configuration ----
stream: true                     # Controls whether to use stream-style API
save: true                       # Indicates whether to persist the message
keybindings: emacs               # Choose keybinding style (emacs, vi)
editor: null                     # Specifies the command used to edit input buffer
wrap: no                         # Controls text wrapping (no, auto, <max-width>)
wrap_code: false                 # Enables or disables wrapping of code blocks

# ---- Function Calling Configuration ----
function_calling: true           # Enables or disables function calling
mapping_tools:                   # Alias for a tool or toolset
  fs: 'fs_cat,fs_ls,fs_mkdir,fs_rm,fs_write'
use_tools: null                  # Which tools to use by default

# ---- Prelude Configuration ----
repl_prelude: null               # Set a default role or session for REPL mode
cmd_prelude: null                # Set a default role or session for CMD mode
agent_prelude: null              # Set a session to use when starting an agent

# ---- Session Configuration ----
save_session: null               # Controls the persistence of the session
compress_threshold: 4000         # Compress session when token count reaches threshold
summarize_prompt: 'Summarize the discussion briefly in 200 words or less'
summary_prompt: 'This is a summary of the chat history as a recap: '

# ---- RAG Configuration ----
rag_embedding_model: null        # Specifies the embedding model for context retrieval
rag_reranker_model: null         # Specifies the reranker model for sorting documents
rag_top_k: 5                     # Specifies the number of documents to retrieve
rag_chunk_size: null             # Defines the size of chunks for document processing
rag_chunk_overlap: null          # Defines the overlap between chunks
rag_template: |                  # RAG query template
  Answer the query based on the context...

document_loaders:                # Document loaders
  pdf: 'pdftotext $1 -'
  docx: 'pandoc --to plain $1'

# ---- Appearance Configuration ----
highlight: true                  # Controls syntax highlighting
light_theme: false               # Activates a light color theme
left_prompt: '{color.green}...'  # Custom REPL left prompt
right_prompt: '{color.purple}...' # Custom REPL right prompt

# ---- Miscellaneous Configuration ----
serve_addr: 127.0.0.1:8000      # Server listening address
user_agent: null                # Set User-Agent HTTP header
save_shell_history: true        # Whether to save shell execution command to history
sync_models_url: https://...    # URL to sync model changes from

# ---- Clients Configuration ----
clients:
  - type: openai
    api_base: https://api.openai.com/v1
    api_key: xxx
    organization_id: org-xxx

  - type: openai-compatible
    name: ollama
    api_base: http://localhost:11434/v1
    models:
      - name: llama3.1
        max_input_tokens: 128000
        supports_function_calling: true
```

### 2. Role Files (*.md)

```markdown
---
model: openai:gpt-4o
temperature: 0.7
---

# Role Name

Role description and system prompt...
```

### 3. Session Files (*.yaml)

```yaml
model_id: openai:gpt-4o
temperature: null
top_p: null
use_tools: null
save_session: null
compress_threshold: 4000

role_name: null
agent_variables: {}
agent_instructions: ""

compressed_messages: []
messages:
  - role: user
    content: Hello
  - role: assistant
    content: Hi there!

data_urls: {}
```

### 4. RAG Configuration Files (*.yaml)

```yaml
name: my-rag
embedding_model: openai:text-embedding-3-small
reranker_model: null
chunk_size: 1000
chunk_overlap: 200
sources:
  - /path/to/document1.pdf
  - /path/to/document2.txt
```

### 5. Macro Files (*.yaml)

```yaml
name: my-macro
description: My macro description
commands:
  - .role my-role
  - .session my-session
  - Hello, how are you?
```

### 6. Agent Definition File (index.yaml)

```yaml
name: "my-agent"
description: "Agent description"
version: "1.0.0"

instructions: |
  Agent system prompt...

dynamic_instructions: false

variables:
  - name: "language"
    description: "Programming language"
    default: "Python"

conversation_starters:
  - "Conversation starter 1"
  - "Conversation starter 2"

documents:
  - /path/to/document.md
```

### 7. Agent Configuration File (config.yaml)

```yaml
model_id: null
temperature: null
top_p: null
use_tools: null
agent_prelude: null
instructions: null
variables: {}
```

## Configuration Priority

1. **Command-line arguments** (highest priority)
2. **Environment variables**
3. **Configuration files**
4. **Default values** (lowest priority)

## Configuration Initialization

On first run, AIChat will:
1. Check if configuration directory exists
2. If not exists and in TTY environment, create configuration file interactively
3. Support dynamic configuration loading via `AICHAT_PROVIDER` or `AICHAT_PLATFORM` environment variables

## Debugging Configuration

```bash
# View current configuration
AICHAT_CONFIG_DIR=/custom/path aichat --info

# List all available configurations
AICHAT_CONFIG_DIR=/custom/path aichat --list-models
AICHAT_CONFIG_DIR=/custom/path aichat --list-roles
AICHAT_CONFIG_DIR=/custom/path aichat --list-agents
```

This configuration system provides great flexibility, allowing users to fully customize AIChat's behavior and storage locations.