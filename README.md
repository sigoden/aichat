# AIChat: All-in-one AI CLI Tool

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/mr3ZZUB9hG)

AIChat is an all-in-one AI CLI tool featuring Chat-REPL, Shell Assistant, RAG, Tool Use, AI Agent, and More.

## Install

### Package Managers

- **Rust Developers:** `cargo install aichat`
- **Homebrew/Linuxbrew Users:** `brew install aichat`
- **Pacman Users**: `yay -S aichat`
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

### 20+ Platforms

AIChat offers users a wide and diverse selection of Large Language Models (LLMs):

- **OpenAI:** GPT-4/GPT-3.5 (paid, vision, embedding, function-calling)
- **Gemini:** Gemini-1.5/Gemini-1.0 (free, paid, vision, embedding, function-calling)
- **Claude:** Claude-3.5/Claude-3 (paid, vision, function-calling)
- **Ollama:** (free, local, embedding)
- **Groq:** Llama-3/Mixtral/Gemma (free, function-calling)
- **Azure-OpenAI:** GPT-4/GPT-3.5 (paid, vision, embedding, function-calling)
- **VertexAI:** Gemini-1.5/Gemini-1.0 (paid, vision, embedding, function-calling)
- **VertexAI-Claude:** Claude-3.5/Claude-3 (paid, vision)
- **Bedrock:** Llama-3/Claude-3.5/Claude-3/Mistral (paid, vision)
- **Mistral** (paid, embedding, function-calling)
- **Cohere:** Command-R/Command-R+ (paid, embedding, reranker, function-calling)
- **Perplexity:** Llama-3/Mixtral (paid)
- **Cloudflare:** (free, vision, embedding)
- **OpenRouter:** (paid, vision, function-calling)
- **Replicate:** (paid)
- **Ernie:** (paid, embedding, reranker, function-calling)
- **Qianwen:** Qwen (paid, vision, embedding, function-calling)
- **Moonshot:** (paid, function-calling)
- **Deepseek:** (paid)
- **ZhipuAI:** GLM-4 (paid, vision, function-calling)
- **LingYiWanWu:** Yi-* (paid, vision)
- **OpenAI-Compatible Platforms** 

### Shell Assistant

Simply input what you want to do in natural language, and aichat will prompt and run the command that achieves your intent.

![aichat-execute](https://github.com/sigoden/aichat/assets/4012553/f99bcd8f-26be-468f-a35e-197e65260f91)

**AIChat is aware of OS and shell you are using, it will provide shell command for specific system you have.**

### Role

Customizable roles allow users to tailor the behavior of LLMs, enhancing productivity and ensuring the tool aligns with specific needs and workflows.

![aichat-role](https://github.com/sigoden/aichat/assets/4012553/76004a01-3b29-4116-bbab-40b4978388f5)

### Session

By default, AIChat behaves in a one-off request/response manner.
With sessions, AIChat conducts context-aware conversations.

![aichat-session](https://github.com/sigoden/aichat/assets/4012553/1444c5c9-ea67-4ad2-80df-a76954e8cce0)

### Retrieval-Augmented Generation (RAG)

Seamlessly integrates document interactions into your chat experience.

![aichat-rag](https://github.com/sigoden/aichat/assets/4012553/6f3e5908-9c95-4d7d-aa9c-7e973ecf9354)

### Function Calling

Function calling supercharges LLMs by connecting them to external tools and data sources. This unlocks a world of possibilities, enabling LLMs to go beyond their core capabilities and tackle a wider range of tasks.

We have created a new repository [https://github.com/sigoden/llm-functions](https://github.com/sigoden/llm-functions) to help you make the most of this feature.

#### Tool Use

Here's a glimpse of How to use the tools.

![aichat-tool-use](https://github.com/sigoden/aichat/assets/4012553/c1b6b136-bbd3-4028-9b01-7d728390c0bf)

#### AI Agent

Agent = Prompt (Role) + Tools (Function Callings) + Knowndge (RAG). It's also known as OpenAI's GPTs.

Here's a glimpse of how to use the agents.

![aichat-agent](https://github.com/sigoden/aichat/assets/4012553/7308a423-2ee5-4847-be1b-a53538bc98dc)

### Local Server

AIChat comes with a built-in lightweight http server.

```
$ aichat --serve
Chat Completions API: http://127.0.0.1:8000/v1/chat/completions
Embeddings API:       http://127.0.0.1:8000/v1/embeddings
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

## Wiki

- [Command-Line Guide](https://github.com/sigoden/aichat/wiki/Command-Line-Guide)
- [Chat-REPL Guide](https://github.com/sigoden/aichat/wiki/Chat-REPL-Guide)
- [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide)
- [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables)
- [Custom Theme](https://github.com/sigoden/aichat/wiki/Custom-Theme)
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)

## License

Copyright (c) 2023-2024 aichat-developers.

AIChat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.