# AIChat: All-in-one AI CLI Tool

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/mr3ZZUB9hG)

AIChat is an all-in-one LLM CLI tool featuring Shell Assistant, Chat-REPL, RAG, AI Tools & Agents, and More. 

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

### Multi-Platform Support

Seamlessly integrate with over 20 leading LLM platforms via a unified interface, including OpenAI, Azure-OpenAI, Gemini (Google AI Studio), VertexAI, Claude, Ollama, Groq, VertexAI, Bedrock, Mistral, AI21, Grok, Cohere, Perplexity, Cloudflare, OpenRouter, Ernie, Qianwen, Moonshot, Deepseek, ZhipuAI, Yi, Jina, VoyageAI, and any OpenAI-Compatible platforms.

### Shell Assistant

Supercharge your command line experience. Simply describe your desired actions in natural language, and let AIChat translate your requests into precise shell commands. 

![aichat-execute](https://github.com/user-attachments/assets/0c77e901-0da2-4151-aefc-a2af96bbb004)

**OS-Aware Intelligence:** AIChat tailors commands to your specific operating system and shell environment.

### Chat-REPL

Bring a powerful Chat-REPL with features such as tab autocompletion, multi-line support, history search, configurable keybindings, and custom REPL prompt.

![aichat-repl](https://github.com/user-attachments/assets/218fab08-cdae-4c3b-bcf8-39b6651f1362)

### Multi-Form Input

Accept various forms of input, such as stdin, local files & dirs, and remote URLs.

```sh
# Use stdin
cat data.txt | aichat
# Use local file
aichat -f data.txt
# Use image file
aichat -f image.png ocr
# Use multi files
aichat -f file1 -f file2 explain
# Use local dirs
aichat -f dir/ summarize
# Use remote URLs
aichat -f https://example.com/page summarize
```

### Role

Define custom roles to tailor LLM behaviors, enhancing interactions and boosting productivity.

![aichat-role](https://github.com/user-attachments/assets/9e0bb48b-20aa-4ab0-8bff-9306dc1c44a2)

> The role consists of a prompt and model configuration.

### Session

Maintain context-aware conversations through sessions, ensuring continuity in interactions.

![aichat-session](https://github.com/user-attachments/assets/56583566-0f43-435f-95b3-730ae55df031)

> The left side uses a session, while the right side does not use a session.

### RAG (Chat with your documents)

Integrate external documents into your LLM conversations for more accurate and contextually relevant responses.


![aichat-rag](https://github.com/user-attachments/assets/359f0cb8-ee37-432f-a89f-96a2ebab01f6)

> Supports local files and directories, as well as remote URLs, including entry websites.

### Function Calling

Function calling supercharges LLMs by connecting them to external tools and data sources. This unlocks a world of possibilities, enabling LLMs to go beyond their core capabilities and tackle a wider range of tasks.

We have created a new repository [https://github.com/sigoden/llm-functions](https://github.com/sigoden/llm-functions) to help you make the most of this feature.

#### AI Tools

Integrate external tools to automate tasks, retrieve information, and perform actions directly within your workflow.

![aichat-tool](https://github.com/user-attachments/assets/7459a111-7258-4ef0-a2dd-624d0f1b4f92)

#### AI Agents (CLI version of OpenAI GPTs)

AI Agent = Instructions (Prompt) + Tools (Function Callings) + Documents (RAG).

![aichat-agent](https://github.com/user-attachments/assets/0b7e687d-e642-4e8a-b1c1-d2d9b2da2b6b)

### Local Server

AIChat comes with a built-in lightweight http server.

```
$ aichat --serve
Chat Completions API: http://127.0.0.1:8000/v1/chat/completions
Embeddings API:       http://127.0.0.1:8000/v1/embeddings
Rerank API:           http://127.0.0.1:8000/v1/rerank
LLM Playground:       http://127.0.0.1:8000/playground
LLM Arena:            http://127.0.0.1:8000/arena?num=2
```

#### Proxy LLM APIs

AIChat offers the ability to function as a proxy server for all LLMs. This allows you to interact with different LLMs using the familiar OpenAI API format, simplifying the process of accessing and utilizing these LLMs.

Test with curl:

```sh
curl -X POST -H "Content-Type: application/json" -d '{
  "model":"claude:claude-3-5-sonnet-20240620",
  "messages":[{"role":"user","content":"hello"}], 
  "stream":true
}' http://127.0.0.1:8000/v1/chat/completions
```

#### LLM Playground

The LLM Playground is a webapp that allows you to interact with any LLM supported by AIChat directly in your browser.

![aichat-llm-playground](https://github.com/user-attachments/assets/dd78614c-49d9-4f76-b8e1-a69f9756a102)

#### LLM Arena

The LLM Arena is a web-based platform where you can compare different LLMs side-by-side. 

![aichat-llm-arena](https://github.com/user-attachments/assets/ab2e0135-363d-44be-8d5f-bb4695fb2c48)

## Custom Themes

AIChat supports custom dark and light themes, which highlight response text and code blocks.

![aichat-themes](https://github.com/sigoden/aichat/assets/4012553/29fa8b79-031e-405d-9caa-70d24fa0acf8)

## Documentation

- [Configuration Guide](https://github.com/sigoden/aichat/wiki/Configuration-Guide)
- [Command-Line Guide](https://github.com/sigoden/aichat/wiki/Command-Line-Guide)
- [Chat-REPL Guide](https://github.com/sigoden/aichat/wiki/Chat-REPL-Guide)
- [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide)
- [RAG Guide](https://github.com/sigoden/aichat/wiki/RAG-Guide)
- [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables)
- [Custom Theme](https://github.com/sigoden/aichat/wiki/Custom-Theme)
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)
- [FAQ](https://github.com/sigoden/aichat/wiki/FAQ)

## License

Copyright (c) 2023-2024 aichat-developers.

AIChat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.