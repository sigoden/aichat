# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build                        # Debug build
cargo build --release              # Release build (LTO, stripped, opt-level="z")
cargo test --all                   # Run all tests (21 tests)
cargo test <test_name>             # Run single test, e.g. cargo test test_parse_structure_prompt1
cargo clippy --all --all-targets -- -D warnings   # Lint (CI enforces zero warnings)
cargo fmt --all --check            # Format check
```

CI runs on Linux, macOS, and Windows with `RUSTFLAGS=--deny warnings`.

## Architecture

AIChat is an all-in-one CLI for 20+ LLM providers. It has three working modes selected at startup in `main.rs`:

- **CMD mode** (default) - single request/response, then exit
- **REPL mode** (no args + interactive terminal) - persistent interactive loop with 36+ dot-commands
- **Serve mode** (`--serve`) - HTTP server exposing OpenAI-compatible APIs, plus web Playground and Arena UIs

### Core modules

| Directory | Purpose |
|-----------|---------|
| `src/client/` | LLM provider implementations behind the `Client` trait (`common.rs`) |
| `src/config/` | `Config` struct (global state machine), `Input`, `Role`, `Session`, `Agent` |
| `src/rag/` | RAG pipeline: document splitting, HNSW vector search, BM25 lexical search, reranking |
| `src/repl/` | Interactive REPL: line editing (reedline), completion, highlighting, command dispatch |
| `src/render/` | Markdown rendering with syntax highlighting (syntect) for terminal output |
| `src/serve.rs` | HTTP server (hyper) with chat completions, embeddings, rerank endpoints |
| `src/function.rs` | Tool/function calling: declaration registry, execution, result merging |
| `src/utils/` | Abort signals, clipboard, HTTP requests, spinners, document loaders, path utils |

### Client plugin system

New LLM providers are added via three macros in `src/client/macros.rs`:

1. `register_client!` - called once in `src/client/mod.rs` to register all providers. Generates the `ClientConfig` enum, per-client structs, `init_client()`, and model listing functions.
2. `impl_client_trait!` - implements the `Client` trait for a provider by wiring up `prepare_*` and `handle_*` functions for chat completions, embeddings, and rerank.
3. `client_common_fns!` - provides shared accessor methods (`global_config`, `name`, `model`, etc.).

Each provider module (e.g., `openai.rs`, `claude.rs`) defines a config struct and the three function pairs (prepare + handle) for chat, embeddings, and rerank. The `openai_compatible.rs` module backs 18 additional providers that share the OpenAI API format.

### Configuration and state

`Config` (`src/config/mod.rs`) acts as both persistent settings (from `~/.aichat/config.yaml`) and runtime state. It holds the current model, role, session, RAG, and agent. State transitions happen through methods like `use_role()`, `use_session()`, `use_agent()`. It's wrapped in `Arc<RwLock<Config>>` as `GlobalConfig` and threaded through the entire application.

### Tool call loop

When an LLM returns tool calls: deduplicate calls, execute each tool (find function in registry, run executable, capture output), merge results back into the input, then recursively call the LLM again. This loop is in `main.rs` and `function.rs`.

### RAG pipeline

Documents are split (language-aware: markdown, HTML, code), embedded via an embedding model, indexed with HNSW (vector) and BM25 (lexical), then hybrid-searched at query time with optional reranking. Data persists as YAML in `~/.aichat/rags/`.

### Key patterns

- **Abort signal** (`utils/abort_signal.rs`) - `Arc<AtomicBool>` + tokio watch channel, passed through async operations for Ctrl+C cancellation via `tokio::select!`
- **Streaming** (`client/stream.rs`) - SSE event processing with `SseHandler` for real-time token output
- **Role templating** (`config/role.rs`) - Markdown frontmatter (model, temperature, use_tools) + structured prompts with `### INPUT:`/`### OUTPUT:` sections for few-shot examples
- **Session compression** (`config/session.rs`) - summarizes old messages when token count exceeds threshold

## Adding a new LLM provider

1. Create `src/client/<provider>.rs` with a config struct and implement `prepare_chat_completions`, `chat_completions`, `chat_completions_streaming` (plus embeddings/rerank if supported)
2. Add the provider to `register_client!` in `src/client/mod.rs`
3. Use `impl_client_trait!` to wire up the trait implementation
4. If OpenAI-compatible, add to `OPENAI_COMPATIBLE_PROVIDERS` array instead
5. Add model definitions to `models.yaml`
