# AIChat: All-in-one AI CLI Tool

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/mr3ZZUB9hG)

AIChat is an all-in-one AI CLI tool featuring Chat-REPL, Shell Assistant, RAG, AI Tools & Agents, and More. 

## Install

### Package Managers

- **Rust Developers:** `cargo install aichat`
- **Homebrew/Linuxbrew Users:** `brew install aichat`
- **Pacman Users**: `pacman -S aichat`
- **Windows Scoop Users:** `scoop install aichat`
- **Android Termux Users:** `pkg install aichat`

### Pre-built Binaries

Download pre-built binaries for macOS, Linux, and Windows from [GitHub Releases](https://github.com/sigoden/aichat/releases), extract them, and add the `aichat` binary to your `$PATH`.

## Get Started

Upon its first launch after installation, AIChat will guide you through the initialization of the configuration file.

![aichat-init-config](https://github.com/sigoden/aichat/assets/4012553/d83c4ac0-1693-4d3c-8a56-a6eabff4ca82)

You can tailor AIChat to your preferences by editing the configuration file.

The [config.example.yaml](https://github.com/sigoden/aichat/blob/main/config.example.yaml) file provides a comprehensive list of all configuration options with detailed explanations.

## Features

### Multi-Platform Support

Effortlessly connect with over 20 leading LLM platforms through a unified interface:

- **OpenAI:** GPT-4/GPT-3.5 (paid, chat, embedding, vision, function-calling)
- **Gemini:** Gemini-1.5/Gemini-1.0 (free, paid, chat, embedding, vision, function-calling)
- **Claude:** Claude-3.5/Claude-3 (paid, chat, vision, function-calling)
- **Ollama:** (free, local, chat, embedding, vision, function-calling)
- **Groq:** Llama-3.1/Gemma2 (free, chat, function-calling)
- **Azure-OpenAI:** GPT-4/GPT-3.5 (paid, chat, embedding, vision, function-calling)
- **VertexAI:** Gemini/Claude/Mistral (paid, chat, embedding, vision, function-calling)
- **Bedrock:** Llama3.1/Claude3.5/Mistral/Command-R+ (paid, chat, embedding, function-calling)
- **Mistral:** (paid, chat, embedding, function-calling)
- **AI21:** (paid, chat, function-calling)
- **Cohere:** Command-R/Command-R+ (paid, chat, embedding, reranker, function-calling)
- **Perplexity:** Llama-3/Mixtral (paid, chat, online)
- **Cloudflare:** (free, chat, embedding)
- **OpenRouter:** (paid, chat, function-calling)
- **Replicate:** (paid, chat)
- **Ernie:** (paid, chat, embedding, reranker, function-calling)
- **Qianwen:** Qwen (paid, chat, embedding, vision, function-calling)
- **Moonshot:** (paid, chat, function-calling)
- **Deepseek:** (paid, chat, function-calling)
- **ZhipuAI:** GLM-4 (paid, chat, embedding, vision, function-calling)
- **LingYiWanWu:** Yi-Large (paid, chat, vision, function-calling)
- **Jina:** (free, paid, embedding, reranker)
- **VoyageAI:** (paid, embedding, reranker)
- **OpenAI-Compatible Platforms** 

### CMD & REPL

AIChat supports both CMD and REPL modes to meet the needs and tastes of different users.

| CMD                         | REPL                   |
| --------------------------- | ---------------------- |
| `-m, --model <model>`       | `.model <model>`       |
| `-r, --role <role>`         | `.role <role>`         |
| `    --prompt <prompt>`     | `.prompt <text>`       |
| `-s, --session [<session>]` | `.session [<session>]` |
| `-a, --agent <agent>`       | `.agent <agent>`       |
| `-R, --rag <rag>`           | `.rag <rag>`           |
| `-f, --file <file/url>`     | `.file <file/url>`     |
| `    --info`                | `.info`                |


```sh
aichat                                          # Enter REPL 
aichat Tell a joke                              # Generate response

aichat -r role1                                 # Enter REPL with the role 'role1'
aichat -r role1 hello world                     # Generate response with role 'role1'

aichat -e install neovim                        # Execute command
aichat -c fibonacci in js                       # Generate code

cat data.toml | aichat -c to json > data.json   # Pipe Input/Output
output=$(aichat -S $input)                      # Run in the script

aichat -f data.txt                              # Use local file
aichat -f image.png Recognize text              # Use image file
aichat -f dir/file1 -f dir/file2 Summarize      # Use multi files
aichat -f dir/ Summarize                        # Use local dir
aichat -f https://example.com/readme Summarize  # Use website
```

### Shell Assistant

Supercharge your command line experience. Simply describe your desired actions in natural language, and let AIChat translate your requests into precise shell commands. 

![aichat-execute](https://github.com/sigoden/aichat/assets/4012553/f99bcd8f-26be-468f-a35e-197e65260f91)

**OS-Aware Intelligence:** AIChat tailors commands to your specific operating system and shell environment.

### Prompt & Role

Define custom roles to tailor LLM behaviors, enhancing interactions and boosting productivity.

![aichat-role](https://github.com/sigoden/aichat/assets/4012553/76004a01-3b29-4116-bbab-40b4978388f5)

### Session Management

Maintain context-aware conversations through sessions, ensuring continuity in interactions.

![aichat-session](https://github.com/sigoden/aichat/assets/4012553/1444c5c9-ea67-4ad2-80df-a76954e8cce0)

### RAG

Integrate external documents into your AI conversations for more accurate and contextually relevant responses.

![aichat-rag](https://github.com/user-attachments/assets/81b81409-460a-4aec-9e08-a3c3da5492d0)

### Function Calling

Function calling supercharges LLMs by connecting them to external tools and data sources. This unlocks a world of possibilities, enabling LLMs to go beyond their core capabilities and tackle a wider range of tasks.

We have created a new repository [https://github.com/sigoden/llm-functions](https://github.com/sigoden/llm-functions) to help you make the most of this feature.

#### AI Tools

Integrate external tools to automate tasks, retrieve information, and perform actions directly within your workflow.

![aichat-tool](https://github.com/user-attachments/assets/7459a111-7258-4ef0-a2dd-624d0f1b4f92)

#### AI Agents

While tools excel at specific tasks, agents offer a more sophisticated approach to problem-solving and user interaction.

Agent = Prompt (Role) + Tools (Function Callings) + Knowndge (RAG). It's also known as OpenAI's GPTs.

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
  "model":"claude:claude-3-opus-20240229",
  "messages":[{"role":"user","content":"hello"}], 
  "stream":true
}' http://127.0.0.1:8000/v1/chat/completions
```

#### LLM Playground

The LLM Playground is a webapp that allows you to interact with any LLM supported by AIChat directly in your browser.

![aichat-llm-playground](https://github.com/sigoden/aichat/assets/4012553/d2334c03-9a07-41a4-a326-e4ee37477ce3)

#### LLM Arena

The LLM Arena is a web-based platform where you can compare different LLMs side-by-side. 

![aichat-llm-arena](https://github.com/sigoden/aichat/assets/4012553/eb1eab0c-4685-4142-89c6-089714b4822c)

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