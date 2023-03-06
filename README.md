# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

A powerful ChatGPT command line tool that allows easy chat with ChatGPT-3.5 in a terminal.

![demo](https://user-images.githubusercontent.com/4012553/223005945-3450cbde-b383-434b-9049-d61877f76a4f.gif)


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
- Markdown highlight
- Stream output
- Multiline input
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
```

Let ChatGPT answer questions in the role of a linux shell expert.

```
〉.role shell

〉resize app.png to 256x256
convert app.png -resize 256x256 app.png

〉 extract password protected app.zip to /tmp/app
unzip -P password app.zip -d /tmp/app
```

## CLI

```
A powerful chatgpt cli.

Usage: aichat [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...  Input text

Options:
  -H, --no-highlight  Turn off highlight
  -L, --list-roles    List all roles
  -r, --role <ROLE>   Select a role
  -h, --help          Print help
  -V, --version       Print version
```

```sh
aichat calculate 25.6 + 32.5
```

```sh
aichat -r shell flip the image horizontally
```

Enter Chat REPL if no text input.
```
$ aichat
Welcome to aichat 0.4.0
Type ".help" for more information.
〉
```

aichat can accept pipe.
```sh
# convert toml to json
cat Cargo.toml | aichat -H turn toml below to json
```

## Chat REPL

aichat has a powerful Chat REPL.

Tle Chat REPL supports:
- emacs keybinding
- command autocompletion
- history search
- fish-style history autosuggestion hints
- mulitline input
- undo support
- clipboard integration

Chat REPL also provide many commands.

```
Welcome to aichat 0.4.0
Type ".help" for more information.
〉.help
.info           Print the information
.set            Modify the configuration temporarily
.role           Select a role
.clear role     Clear the currently selected role
.prompt         Add prompt, aka create a temporary role
.history        Print the history
.clear history  Clear the history
.clear screen   Clear the screen
.multiline      Enter multiline editor mode
.copy           Copy last reply message
.help           Print this help message
.exit           Exit the REPL

Press Ctrl+C to abort session, Ctrl+D to exit the REPL
```

- View current configuration information.

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


- Modify the configuration temporarily

```
〉.set highlight false
〉.set save false
〉.set temperature 1.2
```

- Input multiline text

```
〉.multiline {convert json below to toml
::: {
:::   "an": [
:::     "arbitrarily",
:::     "nested"
:::   ],
:::   "data": "structure"
::: }
::: }
```

- Use GPT Prompt

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

`.prompt` actually creates a temporary role called `%TEMP%` internally, so you run `.clear role` to clear the prompt.

When you are satisfied with the prompt, add it to `roles.yaml` for later use.

- Copy last reply message

The message may be highlighted, and when copied, you will find that they are different from the original Markdown text.

At this point you need to use `.copy` to copy the original text to the clipboard.


## License

Copyright (c) 2023 aichat-developers.

aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
