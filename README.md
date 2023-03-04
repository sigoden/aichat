# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

Chat with ChatGPT-3.5 in the terminal.

![demo](https://user-images.githubusercontent.com/4012553/222897111-dd5015a0-abc1-4c65-a0fb-d491aba3c427.gif)

## Install

### With cargo

```
cargo install --force aichat
```

### Binaries on macOS, Linux, Windows

Download from [Github Releases](https://github.com/sigoden/aichat/releases), unzip and add opscan to your $PATH.

## Features

- Compared to the browser, the terminal starts faster and needs less resources.
- Interactive chat and imperative query.
- Support highlight.
- Predefine prompts for role playing.
- History query/completion.
- Persist chat messages.
- Support proxy.
- Written in rust, single executable file, cross-platform.

## Config

On first launch, aichat will guide you through configuration.

```
> No config file, create a new one? Yes
> Openai API Key: sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
> Use proxy? Yes
> Set proxy: socks5://127.0.0.1:1080
> Save chat messages Yes
```

After setting, it will automatically create the configuration file. Of course, you can also manually set the configuration file. 

```yaml
api_key: "<YOUR SECRET API KEY>"        # Request via https://platform.openai.com/account/api-keys
temperature: 1.0                        # optional, see https://platform.openai.com/docs/api-reference/chat/create#chat/create-temperature
save: true                              # optional, If set to true, aichat will save chat messages to message.md
no_highlight: false                     # optional, Whether to disable highlight
proxy: "socks5://127.0.0.1:1080"        # optional, set proxy server. e.g. http://127.0.0.1:8080 or socks5://127.0.0.1:1080
```

The default config dir is as follows, You can override config dir with `$AICHAT_CONFIG_DIR`.

- Linux:   `/home/alice/.config/aichat`
- Windows: `C:\Users\Alice\AppData\Roaming\aichat`
- MacOS:   `/Users/Alice/Library/Application Support`

aichat may generate the following files in the config dir:

- `config.yaml`: configuration file.
- `roles.yaml`: role definition file.
- `history.txt`: repl history file.
- `messages.md`: chat messages file.

### Roles

We can let ChatGPT play a certain role through `prompt` to make it better generate what we want. See [awesome-chatgpt-prompts](https://github.com/f/awesome-chatgpt-prompts) for details.

We can predefine a batch of roles in `roles.yaml`. For example, we define a emoji translator as follows.

```yaml
- name: emoji
  prompt: >
    I want you to translate the sentences I wrote into emojis. I will write the sentence, and you will express it with emojis.
    I just want you to express it with emojis. I don't want you to reply with anything but emoji.
    When I need to tell you something in English, I will do it by wrapping it in curly brackets like {like this}.
    My first sentence is:
```

Let ChatGPT answer questions in the role of a emoji translator

```
$ aichat --role emoji I am very angry
😠💢👿
```

In interactive chat, we do this:

```
〉.role emoji

〉I am very angry
```

## License

Copyright (c) 2023 aichat-developers.

aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
