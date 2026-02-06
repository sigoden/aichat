# axel-chat: Terminal Interface for the Axel AI Assistant

A fork of [sigoden/aichat](https://github.com/sigoden/aichat) (MIT/Apache 2.0), rebranded and preconfigured for the Axel AI backend.

## What is axel-chat?

axel-chat is a feature-rich terminal chat client built on top of [aichat](https://github.com/sigoden/aichat). It provides:

- **REPL Mode** with tab completion, multi-line input, history search, and configurable keybindings
- **CMD Mode** for one-shot queries from the command line
- **Shell Assistant** for natural language → shell command translation
- **RAG** for document-augmented conversations
- **Sessions** for persistent context-aware conversations
- **20+ LLM providers** supported (OpenAI, Claude, Gemini, Ollama, etc.)

The only difference from upstream aichat is that axel-chat is preconfigured to connect to the Axel AI backend (`localhost:8000`) by default.

## Install

### Build from source

```bash
git clone https://github.com/NorthProt-Inc/axel-chat.git
cd axel-chat
cargo build --release
cp target/release/axel-chat ~/.cargo/bin/
```

## Quick Start

1. Make sure the Axel backend is running on `localhost:8000`

2. Create config file `~/.config/axel_chat/config.yaml`:

```yaml
model: axel:axel
stream: true
save: true
keybindings: emacs

clients:
  - type: openai-compatible
    name: axel
    api_base: http://localhost:8000/v1
    api_key: sk-axel-local
    models:
      - name: axel
        max_input_tokens: 2000000
        supports_vision: true
        supports_function_calling: false
```

3. Start chatting:

```bash
# REPL mode
axel-chat

# Single query
axel-chat -e "안녕"

# Check available models
axel-chat :models
```

## Configuration

axel-chat uses the same configuration format as aichat. Config directory: `~/.config/axel_chat/`

- `config.yaml` - Main configuration
- `roles/` - Custom roles
- `sessions/` - Saved sessions

See [config.example.yaml](config.example.yaml) for all available options.

## Documentation

For full documentation, refer to the upstream aichat wiki:

- [Chat-REPL Guide](https://github.com/sigoden/aichat/wiki/Chat-REPL-Guide)
- [Command-Line Guide](https://github.com/sigoden/aichat/wiki/Command-Line-Guide)
- [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide)
- [RAG Guide](https://github.com/sigoden/aichat/wiki/RAG-Guide)
- [Configuration Guide](https://github.com/sigoden/aichat/wiki/Configuration-Guide)
- [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables)

> **Note:** In upstream docs, replace `aichat` with `axel-chat` and `AICHAT_*` env vars with `AXEL_CHAT_*`.

## Upstream Sync

```bash
git fetch upstream && git merge upstream/main
# Conflicts are expected only in: Cargo.toml, src/config/mod.rs, config.example.yaml, README.md
```

## Credits

This project is a fork of [aichat](https://github.com/sigoden/aichat) by [sigoden](https://github.com/sigoden). All credit for the core functionality goes to the aichat developers and contributors.

## License

Copyright (c) 2023-2025 aichat-developers.

axel-chat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
