# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Build
cargo build                 # Debug build
cargo build --release       # Release build (with LTO, strip, opt-level=z)

# Test
cargo test --all            # Run all tests

# Lint & Format
cargo clippy --all --all-targets -- -D warnings
cargo fmt --all --check     # Check formatting
cargo fmt --all             # Apply formatting

# Cross-compilation (requires: brew install zig && cargo install cargo-zigbuild)
make setup                  # Install all cross-compilation targets
make macos                  # Build for macOS (x86_64 + ARM64)
make macos-universal        # Create fat binary
make linux                  # Build for Linux via zigbuild
make windows                # Build for Windows via zigbuild
make dist                   # Create distribution packages in dist/
```

## Architecture Overview

AIChat is a multi-provider LLM CLI tool with three working modes: CMD (single query), REPL (interactive), and SERVE (HTTP server).

### Core Modules

**`src/main.rs`** - Entry point. Parses CLI args, initializes `GlobalConfig` (`Arc<RwLock<Config>>`), and routes to the appropriate mode handler.

**`src/config/`** - Central state management:
- `mod.rs` - `Config` struct holding all settings and runtime state (model, role, session, agent, rag, functions)
- `role.rs` - Role system with `RoleLike` trait. Special roles: `%shell%`, `%explain-shell%`, `%code%`, `%create-title%`
- `session.rs` - Conversation persistence in markdown format with token tracking and auto-compression
- `input.rs` - `Input` type encapsulating user input from text, files, URLs, or commands
- `agent.rs` - Agent = Instructions + Tools (function calling) + Documents (RAG)

**`src/client/`** - LLM provider implementations:
- `common.rs` - Core `Client` trait with `chat_completions()` and `chat_completions_streaming()`
- Provider-specific files: `openai.rs`, `claude.rs`, `gemini.rs`, `bedrock.rs`, `vertexai.rs`, `cohere.rs`, `openai_compatible.rs`
- Macro-based provider registration in `mod.rs`

**`src/repl/`** - Interactive mode using `reedline`:
- `mod.rs` - Main loop with ~39 commands (`.model`, `.role`, `.session`, `.agent`, `.rag`, `.execute`, etc.)
- `prompt.rs` - Custom prompt rendering with template variables
- `completer.rs` - Context-aware tab completion
- `highlighter.rs` - Markdown syntax highlighting

**`src/render/`** - Output formatting:
- `markdown.rs` - `MarkdownRender` with syntect-based code highlighting
- `stream.rs` - Real-time streaming output handling

**`src/rag/`** - Retrieval-Augmented Generation:
- Hybrid search using HNSW (vector) + BM25 (keyword)
- Document chunking via `RecursiveCharacterTextSplitter` in `splitter/`

**`src/function.rs`** - Function calling / tool execution for LLM tool use

**`src/serve.rs`** - HTTP server with OpenAI-compatible endpoints (`/v1/chat/completions`, `/v1/embeddings`, `/v1/rerank`) and web UIs (`/playground`, `/arena`)

### Key Data Flow

1. **CMD mode**: CLI args → Input creation → Client selection → API call → Render output → Save session
2. **REPL mode**: Command loop → Parse input → Process (chat/command) → Render → Update state
3. **Agent execution**: Load definition → Initialize functions/RAG → Message loop with tool calls

### Important Types

- `GlobalConfig` = `Arc<RwLock<Config>>` - Thread-safe shared config
- `Input` - User input with text, media, tool calls, role, and context flags
- `Message` - Chat message with `MessageRole` (System/User/Assistant) and `MessageContent`
- `Model` - Provider-specific model metadata with capabilities

### Config File Locations

- Main config: `~/.config/aichat/config.yaml`
- Roles: `~/.config/aichat/roles/*.md`
- Sessions: `~/.config/aichat/sessions/*.md`
- Agents: `~/.config/aichat/agents/<name>/index.yaml`
- Functions: `~/.config/aichat/functions/functions.json`
- RAGs: `~/.config/aichat/rags/`
- Macros: `~/.config/aichat/macros/*.sh`

## Adding a New LLM Provider

1. Create `src/client/<provider>.rs` implementing the `Client` trait
2. Register in `src/client/mod.rs` using the provider macro
3. Add model definitions to `models.yaml`

## CI Requirements

All PRs must pass on Ubuntu, macOS, and Windows:
- `cargo test --all`
- `cargo clippy --all --all-targets -- -D warnings`
- `cargo fmt --all --check`

Warnings are denied via `RUSTFLAGS=--deny warnings`.
