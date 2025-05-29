# AIChat: All-in-one LLM CLI Tool

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/mr3ZZUB9hG)

AIChat is an all-in-one LLM CLI tool featuring Shell Assistant, CMD & REPL Mode, RAG, AI Tools & Agents, and More. 

## Install

### Package Managers

- **Rust Developers:** `cargo install aichat`
- **Homebrew/Linuxbrew Users:** `brew install aichat`
- **Pacman Users**: `pacman -S aichat`
- **Windows Scoop Users:** `scoop install aichat`
- **Android Termux Users:** `pkg install aichat`

### Pre-built Binaries

Download pre-built binaries for macOS, Linux, and Windows from [GitHub Releases](https://github.com/sigoden/aichat/releases), extract them, and add the `aichat` binary to your `$PATH`.

## Features

### Multi-Providers

Integrate seamlessly with over 20 leading LLM providers through a unified interface. Supported providers include OpenAI, Claude, Gemini (Google AI Studio), Ollama, Groq, Azure-OpenAI, VertexAI, Bedrock, Github Models, Mistral, Deepseek, AI21, XAI Grok, Cohere, Perplexity, Cloudflare, OpenRouter, Ernie, Qianwen, Moonshot, ZhipuAI, Lingyiwanwu, MiniMax, Deepinfra, VoyageAI, any OpenAI-Compatible API provider.

### CMD Mode

Explore powerful command-line functionalities with AIChat's CMD mode.

![aichat-cmd](https://github.com/user-attachments/assets/6c58c549-1564-43cf-b772-e1c9fe91d19c)

### REPL Mode

Experience an interactive Chat-REPL with features like tab autocompletion, multi-line input support, history search, configurable keybindings, and custom REPL prompts.

![aichat-repl](https://github.com/user-attachments/assets/218fab08-cdae-4c3b-bcf8-39b6651f1362)

### Shell Assistant

Elevate your command-line efficiency. Describe your tasks in natural language, and let AIChat transform them into precise shell commands. AIChat intelligently adjusts to your OS and shell environment.

![aichat-execute](https://github.com/user-attachments/assets/0c77e901-0da2-4151-aefc-a2af96bbb004)

### Multi-Form Input

Accept diverse input forms such as stdin, local files and directories, and remote URLs, allowing flexibility in data handling.

| Input             | CMD                                  | REPL                             |
| ----------------- | ------------------------------------ | -------------------------------- |
| CMD               | `aichat hello`                       |                                  |
| STDIN             | `cat data.txt \| aichat`             |                                  |
| Last Reply        |                                      | `.file %%`                       |
| Local files       | `aichat -f image.png -f data.txt`    | `.file image.png data.txt`       |
| Local directories | `aichat -f dir/`                     | `.file dir/`                     |
| Remote URLs       | `aichat -f https://example.com`      | `.file https://example.com`      |
| External commands | ```aichat -f '`git diff`'```         | ```.file `git diff` ```          |
| Combine Inputs    | `aichat -f dir/ -f data.txt explain` | `.file dir/ data.txt -- explain` |

### Role

Customize roles to tailor LLM behavior, enhancing interaction efficiency and boosting productivity.

![aichat-role](https://github.com/user-attachments/assets/023df6d2-409c-40bd-ac93-4174fd72f030)

> The role consists of a prompt and model configuration.

### Session

Maintain context-aware conversations through sessions, ensuring continuity in interactions.

![aichat-session](https://github.com/user-attachments/assets/56583566-0f43-435f-95b3-730ae55df031)

> The left side uses a session, while the right side does not use a session.

### Macro

Streamline repetitive tasks by combining a series of REPL commands into a custom macro.

![aichat-macro](https://github.com/user-attachments/assets/23c2a08f-5bd7-4bf3-817c-c484aa74a651)

### RAG

Integrate external documents into your LLM conversations for more accurate and contextually relevant responses.

![aichat-rag](https://github.com/user-attachments/assets/359f0cb8-ee37-432f-a89f-96a2ebab01f6)

### Terminal History Context (RAG)

AIChat can further enhance its contextual understanding by leveraging your terminal command history. This allows the AI to provide more relevant assistance and suggestions based on your recent command-line activity.

**Purpose:**
- To provide `aichat` with context from your shell command history.
- To enable more relevant and intelligent assistance, especially for shell-related queries or when recalling past commands.

**User Consent:**
Due to the sensitive nature of shell command history (which can contain passwords, API keys, personal information, etc.), `aichat` requires your explicit consent before accessing this data.
- If `terminal_history_rag.enabled` is set to `true` in your configuration, `aichat` will prompt you for permission on its first run.
- If you grant consent, this choice is saved, and `aichat` will proceed to use the feature.
- If you deny consent, the feature will be automatically disabled for that session and future sessions until consent is granted (e.g., by manually setting `consent_given: true` in the config after understanding the risks, or if a command to re-prompt is added in the future).

**Configuration Options (in `config.yaml` under `terminal_history_rag`):**
```yaml
terminal_history_rag:
  enabled: false                 # Set to true to enable this feature (will prompt for consent if not given).
  # consent_given: false         # Managed by aichat after your first response to the consent prompt.
  max_history_commands: 2000     # Maximum number of recent shell commands to load and index.
  top_k: 3                       # Number of most relevant history snippets to retrieve for context.
  include_timestamps: true       # Whether to include command timestamps in the text sent for embedding (if available from your shell).
```

**Privacy and Security Considerations:**
- **Sensitive Data:** Be aware that your shell history can contain sensitive information like passwords, API keys, file paths, and personal details.
- **Data Usage:** When this feature is active, `aichat` reads your history, processes it locally to find relevant commands, and then includes snippets of these commands (the raw command text) in the prompt sent to the configured Large Language Model (LLM).
- **Risk Acceptance:** Only enable this feature if you understand and accept the risk of parts of your command history being processed and potentially sent to the LLM provider. `aichat` does not filter or redact sensitive data from your history before using it for RAG.

### Function Calling

Function calling supercharges LLMs by connecting them to external tools and data sources. This unlocks a world of possibilities, enabling LLMs to go beyond their core capabilities and tackle a wider range of tasks.

We have created a new repository [https://github.com/sigoden/llm-functions](https://github.com/sigoden/llm-functions) to help you make the most of this feature.

#### AI Tools

Integrate external tools to automate tasks, retrieve information, and perform actions directly within your workflow.

![aichat-tool](https://github.com/user-attachments/assets/7459a111-7258-4ef0-a2dd-624d0f1b4f92)

#### AI Agents (CLI version of OpenAI GPTs)

AI Agent = Instructions (Prompt) + Tools (Function Callings) + Documents (RAG).

![aichat-agent](https://github.com/user-attachments/assets/0b7e687d-e642-4e8a-b1c1-d2d9b2da2b6b)

### Multi-Agent Arena Mode

Engage multiple LLM agents in a conversation with each other. Based on an initial prompt, configured agents will take turns responding, allowing you to observe diverse perspectives and interaction styles.

**CLI Usage:**

```bash
aichat --agent <AGENT_NAME_1> --agent <AGENT_NAME_2> [--agent <AGENT_NAME_N>...] --prompt <INITIAL_PROMPT> [--max-turns <TURNS>]
```

**Arguments:**

*   `--agent <AGENT_NAME>`: Specify the name of a configured agent to participate. At least two agents are required. This argument can be used multiple times for additional agents.
*   `--prompt <INITIAL_PROMPT>`: The initial prompt or topic that kicks off the conversation between the agents.
*   `--max-turns <TURNS>`: Optional. The maximum number of LLM responses in the entire conversation (i.e., total turns taken by all agents). Defaults to 10. Each agent responding once counts as one turn.

**Agent Configuration:**

The agents specified with the `--agent` flag must be pre-configured in AIChat. This typically involves defining their roles, instructions, and any specific models they should use (e.g., via `assets/agents/<agent_name>/index.yaml` or as defined in your main `config.yaml` or `agents.yaml` file, depending on your setup). The unique personality, knowledge base, and conversational style of each agent are determined by its configuration.

**Example:**

```bash
# Example: Have a 'philosopher' agent and a 'comedian' agent discuss the meaning of life for 6 turns
aichat --agent philosopher --agent comedian --prompt "What is the meaning of life?" --max-turns 6
```

### Local Server Capabilities

AIChat includes a lightweight built-in HTTP server for easy deployment.

```
$ aichat --serve
Chat Completions API: http://127.0.0.1:8000/v1/chat/completions
Embeddings API:       http://127.0.0.1:8000/v1/embeddings
Rerank API:           http://127.0.0.1:8000/v1/rerank
LLM Playground:       http://127.0.0.1:8000/playground
LLM Arena:            http://127.0.0.1:8000/arena?num=2
```

#### Proxy LLM APIs

The LLM Arena is a web-based platform where you can compare different LLMs side-by-side. 

Test with curl:

```sh
curl -X POST -H "Content-Type: application/json" -d '{
  "model":"claude:claude-3-5-sonnet-20240620",
  "messages":[{"role":"user","content":"hello"}], 
  "stream":true
}' http://127.0.0.1:8000/v1/chat/completions
```

#### LLM Playground

A web application to interact with supported LLMs directly from your browser.

![aichat-llm-playground](https://github.com/user-attachments/assets/aab1e124-1274-4452-b703-ef15cda55439)

#### LLM Arena

A web platform to compare different LLMs side-by-side.

![aichat-llm-arena](https://github.com/user-attachments/assets/edabba53-a1ef-4817-9153-38542ffbfec6)

## Custom Themes

AIChat supports custom dark and light themes, which highlight response text and code blocks.

![aichat-themes](https://github.com/sigoden/aichat/assets/4012553/29fa8b79-031e-405d-9caa-70d24fa0acf8)

## Documentation

- [Chat-REPL Guide](https://github.com/sigoden/aichat/wiki/Chat-REPL-Guide)
- [Command-Line Guide](https://github.com/sigoden/aichat/wiki/Command-Line-Guide)
- [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide)
- [Macro Guide](https://github.com/sigoden/aichat/wiki/Macro-Guide)
- [RAG Guide](https://github.com/sigoden/aichat/wiki/RAG-Guide)
- [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables)
- [Configuration Guide](https://github.com/sigoden/aichat/wiki/Configuration-Guide)
- [Custom Theme](https://github.com/sigoden/aichat/wiki/Custom-Theme)
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)
- [FAQ](https://github.com/sigoden/aichat/wiki/FAQ)

## License

Copyright (c) 2023-2025 aichat-developers.

AIChat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.