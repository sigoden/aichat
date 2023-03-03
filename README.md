# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

Chat with OpenAI GPT-3.5 in the terminal.

![demo](https://user-images.githubusercontent.com/4012553/222600858-3fb60051-2bf2-4505-92ff-649356cdb1f6.gif)

## Install

### With cargo

```
cargo install --force aichat
```

### Binaries on macOS, Linux, Windows

Download from [Github Releases](https://github.com/sigoden/aichat/releases), unzip and add opscan to your $PATH.

## Config

When starting for the first time, aichat will prompt to set `api_key`, after setting, it will automatically create the configuration file at `$HOME/.aichat.toml`. Of course, you can also manually set the configuration file. 

```toml
api_key = "<YOUR SECRET API KEY>"        # Request via https://platform.openai.com/account/api-keys
temperature = 1.0                        # optional, see https://platform.openai.com/docs/api-reference/chat/create#chat/create-temperature
proxy = "socks5://127.0.0.1:1080"        # optional, set proxy server. e.g. http://127.0.0.1:8080 or socks5://127.0.0.1:1080
```

> We provide a [sample configuration file](.aichat.example.toml). 

## Roles

We can let ChatGPT play a certain role through `prompt` to make it better generate what we want. See [awesome-chatgpt-prompts](https://github.com/f/awesome-chatgpt-prompts) for details.

In aichat, we can predefine a batch of roles in the configuration. For example, we define a javascript-console role as follows.

```toml
[[roles]]
name = "javascript-console"
prompt = "I want you to act as a javascript console. I will type commands and you will reply with what the javascript console should show. I want you to only reply with the terminal output inside one unique code block, and nothing else. do not write explanations. do not type commands unless I instruct you to do so. when i need to tell you something in english, i will do so by putting text inside curly brackets {like this}. My first command is:"
```

Let ChaGPT answer questions in the role of a javascript-console.

```
aichat --role javascript-console console.log("Hello World")
```

In interactive mode, we do this:

```
〉.role javascript-console
〉console.log("Hello world")
```

## License

Copyright (c) 2023 aichat-developers.

aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.