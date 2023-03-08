# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

A powerful ChatGPT command line tool that allows easy chat with ChatGPT-3.5 in a terminal.

![demo](https://user-images.githubusercontent.com/4012553/223645914-f397b95f-1a30-4eda-a6a8-5bd0c2903add.gif)

## Install

### With cargo

```
cargo install --force aichat
```

### Binaries on macOS, Linux, Windows

Download from [Github Releases](https://github.com/sigoden/aichat/releases), unzip and add opscan to your $PATH.

## Features

- Predefine AI [roles](#roles)
- Use GPT prompt easily
- Powerful [Chat REPL](#chat-repl)
- syntax highlighting markdown and other 200 languages.
- Stream output with hand typing effect
- Multiline input support and emacs-like editing experience
- Proxy support
- Save chat messages

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
api_key: "<YOUR SECRET API KEY>"  # Request via https://platform.openai.com/account/api-keys
temperature: 1.0                  # optional, see https://platform.openai.com/docs/api-reference/chat/create#chat/create-temperature
save: true                        # optional, If set to true, aichat will save chat messages to message.md
highlight: true                   # optional, Set false to turn highlight
proxy: "socks5://127.0.0.1:1080"  # optional, set proxy server. e.g. http://127.0.0.1:8080 or socks5://127.0.0.1:1080
```

> You can specify the configuration directory through `$AICHAT_CONFIG_DIR`

> You can use `.info` to view the current configuration file path

### Roles

We can let ChatGPT play a certain role through `prompt` to make it better generate what we want.

We can predefine a batch of roles in `roles.yaml`.

For example, we define a role.

```yaml
- name: shell
  prompt: >
    I want you to act as a linux shell expert.
    I want you to answer only with bash code.
    Do not write explanations.
  # temperature: 0.3
```

Let ChatGPT answer questions in the role of a linux shell expert.

```
〉.role shell

〉 extract encrypted zipfile app.zip to /tmp/app
```
```bash
mkdir /tmp/app
unzip -P PASSWORD app.zip -d /tmp/app
```

## CLI

```
A powerful chatgpt cli.

Usage: aichat [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...  Input text

Options:
  -H, --no-highlight  Turn off highlight
  -S, --no-stream     No stream output
      --list-roles    List all roles
  -r, --role <ROLE>   Select a role
  -h, --help          Print help
  -V, --version       Print version
```
### Command mode

```sh
aichat math 3.8x4 
```

control highlighting and streaming

```sh
aichat how to post a json in rust         # highlight, streaming output
aichat -H -S how to post a json in rust   # no highlight, output all at once
```

pipe input/output
```sh
# convert toml to json
cat data.toml | aichat turn toml below to json > data.json
```
### Chat mode

Enter Chat REPL if no text input.
```
$ aichat
Welcome to aichat 0.5.0
Type ".help" for more information.
〉
```

## Chat REPL

aichat has a powerful Chat REPL.

Tle Chat REPL supports:
- emacs keybinding
- command autocompletion
- history search
- fish-style history autosuggestion hints
- edit/past multiline input
- undo support
- clipboard integration

Chat REPL also provide many commands.

```
Welcome to aichat 0.5.0
Type ".help" for more information.
.info           Print the information
.set            Modify the configuration temporarily
.prompt         Add a GPT prompt
.role           Select a role
.clear role     Clear the currently selected role
.history        Print the history
.clear history  Clear the history
.editor         Enter editor mode for multiline input
.help           Print this help message
.exit           Exit the REPL

Press Ctrl+C to abort session, Ctrl+D to exit the REPL
```

### `.info` - view current configuration information.

```
〉.info
config_file         /home/alice/.config/aichat/config.yaml
roles_file          /home/alice/.config/aichat/roles.yaml
messages_file       /home/alice/.config/aichat/messages.md
role                -
api_key             sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
temperature         -
save                true
highlight           true
proxy               -
dry_run             false
```

### `.set` - modify the configuration temporarily

```
〉.set highlight false
〉.set save false
〉.set temperature 1.2
```

### `.prompt` - use GPT prompt

When you set up a prompt, every message sent later will carry the prompt.

```
〉.prompt {
:::     I want you to translate the sentences I wrote into emojis.
:::     I will write the sentence, and you will express it with emojis.
:::     I don't want you to reply with anything but emoji.
::: }
Done

〉You are a genius
👉🧠💡👨‍🎓

〉I'm embarrassed
🙈😳
```

`.prompt` actually creates a temporary role called `%TEMP%` internally, so **run `.clear role` to clear the prompt**.

When you are satisfied with the prompt, add it to `roles.yaml` for later use.

### `.role` - let the ai play a role

Select a role.
```
〉.role shell
```

Unselect a role.
```
〉.clear role
```

Use `.info` to check current selected role.

### `.editor` - input/paste multiline text

Type `.editor {` to enter editor mode, you can input/paste multiline text, quite editor mode with `}`

```
〉.editor {convert json below to toml
::: {
:::   "an": [
:::     "arbitrarily",
:::     "nested"
:::   ],
:::   "data": "structure"
::: }
::: }
```

## License

Copyright (c) 2023 aichat-developers.

aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
