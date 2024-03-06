# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)

Use GPT-4(V), Gemini, LocalAI, Ollama and other LLMs in the terminal.

AIChat in chat REPL mode:

![chat-repl mode](https://github.com/sigoden/aichat/assets/4012553/13427d54-efd5-4f4c-b17b-409edd30dfa3)

AIChat in command mode:

![command mode](https://github.com/sigoden/aichat/assets/4012553/db96a38a-2f14-4127-91e7-4d111aba6ca9)

## Install

### Use a package management tool

For Rust programmer
```sh
cargo install aichat
```

For macOS Homebrew or a Linuxbrew user
```sh
brew install aichat
```

For Windows Scoop user
```sh
scoop install aichat
```

For Android termux user
```sh
pkg install aichat
```

### Binaries for macOS, Linux, Windows

Download it from [GitHub Releases](https://github.com/sigoden/aichat/releases), unzip and add aichat to your $PATH.

## Features
- Support most of the LLM platforms
  - OpenAI (paid, vision)
  - Gemini (free, vision)
  - Claude (paid)
  - Mistral (paid)
  - LocalAI (free, local, vision)
  - Ollama (free, local)
  - Azure-OpenAI (paid)
  - VertexAI (paid, vision)
  - Ernie (paid)
  - Qianwen (paid, vision)
- Support [REPL Mode](#chat-repl) and [Command Mode](#command)
- Support [Roles](#roles)
- Support context-aware conversation (session)
- Support multimodal models (vision)
- Execute commands using natural language
- Syntax highlighting for markdown and 200+ languages in code blocks
- Save messages/sessions
- Stream/Non-stream output
- With proxy
- With dark/light theme

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
compress_threshold: 1000         # Compress session if tokens exceed this value (valid when >=1000)

clients:
  - type: openai
    api_key: sk-xxx

  - type: localai
    api_base: http://localhost:8080/v1
    models:
      - name: gpt4all-j
        max_input_tokens: 8192
```

Take a look at the [config.example.yaml](config.example.yaml) for the complete configuration details.

There are some configurations that can be set through environment variables. For more information, please refer to the [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables) page.

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
> .role shell

shell>  extract encrypted zipfile app.zip to /tmp/app
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
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)
- Tab autocomplete
- Edit/paste multiline text
- Open an editor to modify the current prompt
- Undo support

### `.help` - print help message

```
> .help
.help                    Print this help message
.info                    Print system info
.model                   Switch LLM model
.role                    Use a role
.info role               Show role info
.exit role               Leave current role
.session                 Start a context-aware chat session
.info session            Show session info
.exit session            End the current session
.file                    Attach files to the message and then submit it
.set                     Modify the configuration parameters
.copy                    Copy the last reply to the clipboard
.exit                    Exit the REPL

Type ::: to begin multi-line editing, type ::: to end it.
Press Ctrl+O to open an editor to modify the current prompt.
Press Ctrl+C to abort readline, Ctrl+D to exit the REPL

```

### `.info` - view information

```
> .info
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
> .role emoji
```

Send message with the role:

```
emoji> hello
👋
```

Leave current role:

```
emoji> .exit role

> hello
Hello there! How can I assist you today?
```

Show role info:

```
emoji> .info role
name: emoji
prompt: I want you to translate the sentences I write into emojis. I will write the sentence, and you will express it with emojis. I just want you to express it with emojis. I don't want you to reply with anything but emoji. When I need to tell you something in English, I will do it by wrapping it in curly brackets like {like this}.
temperature: null
```

Temporarily use a role to send a message.
```
> ::: .role emoji
hello world
:::
👋🌍

> 
```

### `.session` - context-aware conversation

By default, aichat behaves in a one-off request/response manner.

You should run aichat with `-s/--session` or use the `.session` command to start a session.


```
> .session

temp) 1 to 5, odd only                                                                    0
1, 3, 5

temp) to 7                                                                        19(0.46%)
1, 3, 5, 7

temp) .exit session                                                               42(1.03%)
? Save session? (y/N)  

```

The prompt on the right side is about the current usage of tokens and the proportion of tokens used, 
compared to the maximum number of tokens allowed by the model.


### `.file` - attach files to the message 

```
Usage: .file <file>... [-- text...]

.file message.txt
.file config.yaml -- convert to toml
.file a.jpg b.jpg -- What’s in these images?
.file https://ibb.co/a.png https://ibb.co/b.png -- what is the difference?
```

> Only the current model that supports vision can process images submitted through `.file` command.

### `.set` - modify the configuration temporarily

```
> .set temperature 1.2
> .set dry_run true
> .set highlight false
> .set save false
> .set auto_copy true
> .set compress_threshold 1000
```

## Command

```
Usage: aichat [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...  Input text

Options:
  -m, --model <MODEL>        Choose a LLM model
  -r, --role <ROLE>          Choose a role
  -s, --session [<SESSION>]  Create or reuse a session
  -e, --execute              Execute commands using natural language
  -c, --code                 Generate only code
  -f, --file <FILE>...       Attach files to the message to be sent
  -H, --no-highlight         Disable syntax highlighting
  -S, --no-stream            No stream output
  -w, --wrap <WRAP>          Specify the text-wrapping mode (no, auto, <max-width>)
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
aichat -s                                    # Start REPL with a new session
aichat -s temp                               # Reuse temp session
aichat -r shell -s                           # Create a session with a role
aichat -m openai:gpt-4-32k -s                # Create a session with a model
aichat -s temp unzip a file                  # Run session in command mode

aichat -r shell unzip a file                 # Use role in command mode
aichat -s shell unzip a file                 # Use session in command mode

cat data.toml | aichat -c to json            # Read stdin

aichat --file a.png b.png -- diff images     # Attach files
aichat --file screenshot.png -r ocr          # Attach files with a role

aichat --list-models                         # List all available models
aichat --list-roles                          # List all available roles
aichat --list-sessions                       # List all available models

aichat --info                                # system-wide information
aichat -s temp --info                        # Show session details
aichat -r shell --info                       # Show role info

$(echo "$data" | aichat -c to json)          # Use aichat in a script
```

### Execute commands using natural language

Simply input what you want to do in natural language, and aichat will prompt and run the command that achieves your intent.

```
aichat -s <text>...
```

![aichat-execute](https://github.com/sigoden/aichat/assets/4012553/37714a89-2841-41c4-a989-759642f46676)

Aichat is aware of OS and `$SHELL` you are using, it will provide shell command for specific system you have. For instance, if you ask `aichat` to update your system, it will return a command based on your OS. Here's an example using macOS:

```sh
aichat -e update my system
# sudo softwareupdate -i -a
# ? [e]xecute, [d]escribe, [a]bort:  (e)  
```

The same prompt, when used on Ubuntu, will generate a different suggestion:
```sh
 aichat -e update my system
# sudo apt update && sudo apt upgrade -y
# ? [e]xecute, [d]escribe, [a]bort:  (e)  
```

We can still use pipes to pass input to aichat and generate shell commands:

```sh
aichat -e POST localhost with < data.json
# curl -X POST -H "Content-Type: application/json" -d '{"a": 1, "b": 2}' localhost
# ? [e]xecute, [d]escribe, [a]bort:  (e)  
```

We can also pipe the output of aichat which will disable interactive mode.
```sh
aichat -e find all json files in current folder | pbcopy
```

### Shell integration

This is a **very handy feature**, which allows you to use `aichat` shell completions directly in your terminal, without the need to type `aichat` with prompt and arguments. This feature puts `aichat` completions directly into terminal buffer (input line), allowing for immediate editing of suggested commands.

![aichat-integration](https://github.com/sigoden/aichat/assets/4012553/873ebf23-226c-412e-a34f-c5aaa7017524)

To install shell integration, go to [./scripts/shell-integration](https://github.com/sigoden/aichat/tree/main/scripts/shell-integration) to download the script and source the script in rc file. After that restart your shell. You can invoke the completion with `alt+e` hotkey.

## License

Copyright (c) 2023-2024 aichat-developers.

Aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
