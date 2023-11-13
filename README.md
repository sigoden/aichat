# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

Use ChatGPT, LocalAI and other LLMs in the terminal.

AIChat in chat mode:

![chat mode](https://user-images.githubusercontent.com/4012553/226499667-4c6b261a-d897-41c7-956b-979b69da5982.gif)

AIChat in command mode:

![command mode](https://user-images.githubusercontent.com/4012553/226499595-0b536c82-b039-4571-a077-0c40ad57f7db.png)

## Install

### With cargo

```
cargo install --force aichat
```

### Binaries for macOS, Linux, Windows

Download it from [GitHub Releases](https://github.com/sigoden/aichat/releases), unzip and add aichat to your $PATH.

## Support Models

- OpenAI: gpt-3.5/gpt-4
- LocalAI: opensource models 
- Azure-OpenAI: user deployed gpt3.5/gpt4
- PaLM: chat-bison-001 
- Ernie: eb-instant/ernie-bot/ernie-bot-4
- Qianwen: qwen-turbo/qwen-plus

## Features

- Support chat and command modes
- Use [Roles](#roles)
- Powerful [Chat REPL](#chat-repl)
- Context-aware conversation/session
- Syntax highlighting markdown and 200 other languages
- Stream output with hand-typing effect
- Support proxy 
- Dark/light theme
- Save chat messages/sessions

## Config

On first launch, aichat will guide you through the configuration.

```
> No config file, create a new one? Yes
> AI Platform: openai
> API Key: sk-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

Feel free to adjust the configuration according to your needs.

```yaml
model: openai:gpt-3.5-turbo      # LLM model
temperature: 1.0                 # GPT temperature, between 0 and 2
save: true                       # Whether to save the message
highlight: true                  # Set false to turn highlight
light_theme: false               # Whether to use a light theme
wrap: no                         # Specify the text-wrapping mode (no, auto, <max-width>)
wrap_code: false                 # Whether wrap code block
auto_copy: false                 # Automatically copy the last output to the clipboard
keybindings: emacs               # REPL keybindings. values: emacs, vi
prelude: ''                      # Set a default role or session (role:<name>, session:<name>)

clients:
  - type: openai
    api_key: sk-xxx
    organization_id:

  - type: localai
    api_base: http://localhost:8080/v1
    models:
      - name: gpt4all-j
        max_tokens: 8192
```

Check out [config.example.yaml](config.example.yaml) for all configuration items.

There are some configurations that can be set through environment variables. Please see the [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables) for details.

### Roles

We can define a batch of roles in `roles.yaml`.

> We can get the location of `roles.yaml` through the repl's `.info` command or cli's `--info` option.

For example, we can define a role:

```yaml
- name: shell
  prompt: >
    I want you to act as a Linux shell expert.
    I want you to answer only with bash code.
    Do not provide explanations.
```

Let ChatGPT answer questions in the role of a Linux shell expert.

```
〉.role shell

shell〉 extract encrypted zipfile app.zip to /tmp/app
mkdir /tmp/app
unzip -P PASSWORD app.zip -d /tmp/app
```

AIChat with roles will be a universal tool.

```
$ aichat --role shell extract encrypted zipfile app.zip to /tmp/app
unzip -P password app.zip -d /tmp/app

$ cat README.md | aichat --role spellcheck
```

For more details about roles, please visit [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide).

## Chat REPL

aichat has a powerful Chat REPL.

The Chat REPL supports:

- Emacs/Vi keybinding
- Command autocompletion
- Edit/paste multiline input
- Undo support

### `.help` - print help message

```
〉.help
.help                    Print this help message
.info                    Print system info
.model                   Switch LLM model
.role                    Use a role
.info role               Show role info
.exit role               Leave current role
.session                 Start a context-aware chat session
.info session            Show session info
.exit session            End the current session
.set                     Modify the configuration parameters
.copy                    Copy the last reply to the clipboard
.read                    Read files into the message and submit
.exit                    Exit the REPL

Type ::: to begin multi-line editing, type ::: to end it.
Press Ctrl+C to abort readline, Ctrl+D to exit the REPL

```

### `.info` - view information

```
〉.info
model               openai:gpt-3.5-turbo
temperature         -
dry_run             false
save                true
highlight           true
light_theme         false
wrap                no
wrap_code           false
auto_copy           false
keybindings         emacs
prelude             -
config_file         /home/alice/.config/aichat/config.yaml
roles_file          /home/alice/.config/aichat/roles.yaml
messages_file       /home/alice/.config/aichat/messages.md
sessions_dir        /home/alice/.config/aichat/sessions
```

### `.model` - choose a model

```
> .model openai:gpt-4
> .model localai:gpt4all-j
```

> You can easily enter enter model name using autocomplete.

### `.role` - let the AI play a role

Select a role:

```
〉.role emoji
```

Send message with the role:

```
emoji〉hello
👋
```

Leave current role:

```
emoji〉.exit role

〉hello
Hello there! How can I assist you today?
```

Show role info:

```
emoji〉.info role
name: emoji
prompt: I want you to translate the sentences I write into emojis. I will write the sentence, and you will express it with emojis. I just want you to express it with emojis. I don't want you to reply with anything but emoji. When I need to tell you something in English, I will do it by wrapping it in curly brackets like {like this}.
temperature: null
```

Temporarily use a role to send a message.
```
〉::: .role emoji
hello world
:::
👋🌍

〉
```

### `.session` - context-aware conversation

By default, aichat behaves in a one-off request/response manner.

You should run aichat with `-s/--session` or use the `.session` command to start a session.


```
〉.session

temp）1 to 5, odd only                                                                    0
1, 3, 5

temp）to 7                                                                        19(0.46%)
1, 3, 5, 7

temp）.exit session                                                               42(1.03%)
? Save session? (y/N)  

```

The prompt on the right side is about the current usage of tokens and the proportion of tokens used, 
compared to the maximum number of tokens allowed by the model.


### `.set` - modify the configuration temporarily

```
〉.set temperature 1.2
〉.set dry_run true
〉.set highlight false
〉.set save false
〉.set auto_copy true
```

## Command Line

```
Usage: aichat [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...  Input text

Options:
  -m, --model <MODEL>        Choose a LLM model
  -r, --role <ROLE>          Choose a role
  -s, --session [<SESSION>]  Create or reuse a session
  -H, --no-highlight         Disable syntax highlighting
  -S, --no-stream            No stream output
  -w, --wrap <WRAP>          Specify the text-wrapping mode (no*, auto, <max-width>)
      --light-theme          Use light theme
      --dry-run              Run in dry run mode
      --info                 Print related information
      --list-models          List all available models
      --list-roles           List all available roles
      --list-sessions        List all available sessions
  -h, --help                 Print help
  -V, --version              Print version
```

Here are some practical examples:

```sh
aichat -s                                    # Start REPL with a new temp session
aichat -s temp                               # Reuse temp session
aichat -r shell -s                           # Create a session with a role
aichat -m openai:gpt-4-32k -s                # Create a session with a model
aichat -s sh unzip a file                    # Run session in command mode

aichat -r shell unzip a file                 # Use role in command mode
aichat -s shell unzip a file                 # Use session in command mode

cat config.json | aichat convert to yaml     # Read stdin
cat config.json | aichat -r convert:yaml     # Read stdin with a role
cat config.json | aichat -s i18n             # Read stdin with a session

aichat --list-models                         # List all available models
aichat --list-roles                          # List all available roles
aichat --list-sessions                       # List all available models

aichat --info                                # system-wide information
aichat -s temp --info                        # Show session details
aichat -r shell --info                       # Show role info

$(echo "$data" | aichat -S -H to json)       # Use aichat in a script
```

## License

Copyright (c) 2023 aichat-developers.

aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
