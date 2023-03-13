# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

Chat with gpt-3.5/chatgpt in terminal.

![demo](https://user-images.githubusercontent.com/4012553/223645914-f397b95f-1a30-4eda-a6a8-5bd0c2903add.gif)

## Install

### With cargo

```
cargo install --force aichat
```

### Binaries on macOS, Linux, Windows

Download it from [Github Releases](https://github.com/sigoden/aichat/releases), unzip and add aichat to your $PATH.

## Features

- Predefine AI [roles](#roles)
- Use GPT prompt easily
- Powerful [Chat REPL](#chat-repl)
- Context-ware conversation
- syntax highlighting markdown and other 200 languages.
- Stream output with hand typing effect
- Multiline input support and emacs-like editing experience
- Support proxy
- Support dark/light theme
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
save: true                        # optional, If set true, aichat will save chat messages to message.md
highlight: true                   # optional, Set false to turn highlight
proxy: "socks5://127.0.0.1:1080"  # optional, set proxy server. e.g. http://127.0.0.1:8080 or socks5://127.0.0.1:1080
conversation_first: false         # optional, If set true, start a conversation immediately upon repl
light_theme: false                # optional, If set true, use light theme
```

> You can use `.info` to view the current configuration file path and roles file path.

> You can use [Enviroment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables) to customize certain configuration items.

### Roles

We can let ChatGPT play a certain role through `prompt` to make it better generate what we want.

We can predefine a batch of roles in `roles.yaml`.

> We can get the location of `roles.yaml` through the `.info` command.

For example, we define a role

```yaml
- name: shell
  prompt: >
    I want you to act as a linux shell expert.
    I want you to answer only with bash code.
    Do not provide explanations.
  # temperature: 0.3
```

Let ChatGPT answer questions in the role of a linux shell expert.
```
〉.role shell

shell〉 extract encrypted zipfile app.zip to /tmp/app
---
mkdir /tmp/app
unzip -P PASSWORD app.zip -d /tmp/app
---
```

We have provided many [Role Examples](https://github.com/sigoden/aichat/wiki/Role-Examples).

## CLI

```
A powerful chatgpt cli.

Usage: aichat [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...  Input text

Options:
  -p, --prompt <PROMPT>  Set a GPT prompt
  -H, --no-highlight     Disable syntax highlightiing
  -S, --no-stream        No stream output
      --list-roles       List all roles
  -r, --role <ROLE>      Select a role
  -h, --help             Print help
  -V, --version          Print version
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

The Chat REPL supports:
- Emacs keybinding
- Command autocompletion
- History search
- Fish-style history autosuggestion hints
- Edit/past multiline input
- Undo support
- Clipboard integration

### multi-line editing mode

**Type `{` or `(` at the beginning of the line to enter the multi-line editing mode.** In this mode you can type or paste multiple lines of text. Type the corresponding `}` or `)` at the end of the line to exit the mode and submit the content.

```
〉{ convert json below to toml
{
  "an": [
    "arbitrarily",
    "nested"
  ],
  "data": "structure"
}}
```


### `.help` - Print help message

```
〉.help
.info                    Print the information
.set                     Modify the configuration temporarily
.prompt                  Add a GPT prompt
.role                    Select a role
.clear role              Clear the currently selected role
.conversation            Start a conversation.
.clear conversation      End current conversation.
.history                 Print the history
.clear history           Clear the history
.help                    Print this help message
.exit                    Exit the REPL

Type `{` to enter the multi-line editing mode, type '}' to exit the mode.
Press Ctrl+C to abort readline, Ctrl+D to exit the REPL

```

### `.info` - view current configuration information.

```
〉.info
config_file         /home/alice/.config/aichat/config.yaml
roles_file          /home/alice/.config/aichat/roles.yaml
messages_file       /home/alice/.config/aichat/messages.md
api_key             sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
temperature         -
save                true
highlight           true
proxy               -
conversation_first  false
light_theme         false
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
〉{ .prompt
I want you to translate the sentences I wrote into emojis.
I will write the sentence, and you will express it with emojis.
I just want you to express it with emojis.
I want you to reply only with emojis.
}
Done

Ｐ〉You are a genius
👉🧠💡👨‍🎓

Ｐ〉I'm embarrassed
🙈😳
```

`.prompt` actually creates a temporary role called `%TEMP%` internally, so **run `.clear role` to clear the prompt**.

When you are satisfied with the prompt, add it to `roles.yaml` for later use.

### `.role` - let the ai play a role

Select a role.

```
〉.role emoji
name: emoji
prompt: I want you to translate the sentences I wrote into emojis. I will write the sentence, and you will express it with emojis. I just want you to express it with emojis. I don't want you to reply with anything but emoji. When I need to tell you something in English, I will do it by wrapping it in curly brackets like {like this}.
temperature: null
```

AI play the role we specified
```
emoji〉hello
👋
```

Clear current selected role
```
emoji〉.clear role

〉hello 
Hello there! How can I assist you today?
```

### `.conversation` - start a context-aware conversation

By default, aichat behaves in a one-off request/response manner.

You can run `.conversation` to enter context-aware mode, or set `config.conversation_first` true to start a conversation immediately upon repl.

```
〉.conversation

＄list 1 to 5, one per line                                                              4089
1
2
3
4
5

＄reverse the list                                                                       4065
5
4
3
2
1

＄.clear conversation                                                                    4043

〉
```

When enter conversation mode, prompt `〉` will change to `＄`, A number will appear on the right, which means how many tokens left to use.
Once the number becomes zero, you need to start a new conversation.

## License

Copyright (c) 2023 aichat-developers.

aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
