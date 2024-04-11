# AIChat

[![CI](https://github.com/sigoden/aichat/actions/workflows/ci.yaml/badge.svg)](https://github.com/sigoden/aichat/actions/workflows/ci.yaml)
[![Crates](https://img.shields.io/crates/v/aichat.svg)](https://crates.io/crates/aichat)
[![Discord](https://img.shields.io/discord/1226737085453701222?label=Discord)](https://discord.gg/NYmfN6CA)

All-in-one chat and copilot CLI that integrates 10+ AI platforms.

Command Mode:

![command mode](https://github.com/sigoden/aichat/assets/4012553/2ab27e1b-4078-4ea3-a98f-591b36491685)

Chat REPL mode:

![chat-repl mode](https://github.com/sigoden/aichat/assets/4012553/13427d54-efd5-4f4c-b17b-409edd30dfa3)

## Features

- Supports [chat-REPL](#chat-repl)
- Supports [roles](#roles)
- Supports sessions (context-aware conversation)
- Supports image analysis (vision)
- [Shell commands](#shell-commands): Execute commands using natural language
- [Shell integration](#shell-integration): AI-powered shell autocompletion
- [Custom theme](https://github.com/sigoden/aichat/wiki/Custom-Theme)
- Stream/non-stream output

## Integrated platforms

- OpenAI: GPT3.5/GPT4 (paid, vision)
- Azure-OpenAI (paid)
- OpenAI-Compatible platforms
- Gemini: Gemini-1.0/Gemini-1.5 (free, vision)
- VertexAI (paid, vision)
- Claude: Claude3 (vision, paid)
- Mistral (paid)
- Cohere (paid)
- Ollama (free, local)
- Ernie (paid)
- Qianwen (paid, vision)
- Moonshot (paid)

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

For Android Termux user
```sh
pkg install aichat
```

### Binaries for macOS, Linux, and Windows

Download it from [GitHub Releases](https://github.com/sigoden/aichat/releases), unzip, and add aichat to your `$PATH`.

## Config

On first launch, aichat will guide you through the configuration.

```
> No config file, create a new one? Yes
> AI Platform: openai
> API Key: <your_api_key_here>
```

Feel free to adjust the configuration according to your needs.

```yaml
model: openai:gpt-3.5-turbo      # LLM model
temperature: 1.0                 # LLM temperature
save: true                       # Whether to save the message
save_session: null               # Whether to save the session, if null, asking
highlight: true                  # Set false to turn highlight
light_theme: false               # Whether to use a light theme
wrap: no                         # Specify the text-wrapping mode (no, auto, <max-width>)
wrap_code: false                 # Whether wrap code block
ctrlc_exit: false                # Whether to exit REPL when Ctrl+C is pressed
auto_copy: false                 # Automatically copy the last output to the clipboard
keybindings: emacs               # REPL keybindings. values: emacs, vi
prelude: ''                      # Set a default role or session (role:<name>, session:<name>)
compress_threshold: 1000         # Compress session if tokens exceed this value (valid when >=1000)

clients:
  - type: openai
    api_key: sk-xxx

  - type: openai-compatible
    name: localai
    api_base: http://127.0.0.1:8080/v1
    models:
      - name: llama2
        max_input_tokens: 8192
```

Please review the [config.example.yaml](config.example.yaml) to see all available configuration options.

There are some configurations that can be set through environment variables, see [Environment Variables](https://github.com/sigoden/aichat/wiki/Environment-Variables).

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
  -f, --file <FILE>          Attach files to the message
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
aichat                                          # Start in REPL mode

aichat -e install nvim                          # Execute
aichat -c fibonacci in js                       # Code

aichat -s                                       # REPL + New session
aichat -s session1                              # REPL + New/Reuse 'session1'

aichat --info                                   # System info
aichat -r role1 --info                          # Role info
aichat -s session1 --info                       # Session info

cat data.toml | aichat -c to json > data.json   # Pipe stdio/stdout

aichat -f data.toml -c to json > data.json      # Attach files

aichat -f a.png -f b.png diff images            # Attach images
```

### Shell commands

Simply input what you want to do in natural language, and aichat will prompt and run the command that achieves your intent.

```
aichat -e <text>...
```

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

### Generating code

By using the `--code` or `-c` parameter, you can specifically request pure code output, for instance:

```
aichat --code a echo server in node.js
```

```js
const net = require('net');

const server = net.createServer(socket => {
  socket.on('data', data => {
    socket.write(data);
  });

  socket.on('end', () => {
    console.log('Client disconnected');
  });
});

server.listen(3000, () => {
  console.log('Server running on port 3000');
});
```

Since it is valid js code, we can redirect the output to a file:
```
aichat --code a echo server in node.js > echo-server.js 
node echo-server.js
```

**The `-c/--code` option ensures the extraction of code from Markdown.**

## Chat REPL

Aichat has a powerful Chat REPL.

The REPL supports:

- Tab autocompletion
- [Custom REPL Prompt](https://github.com/sigoden/aichat/wiki/Custom-REPL-Prompt)
- Emacs/Vi keybinding
- Edit/paste multi-line text
- Open an editor to modify the current prompt
- History
- Undo support

### `.help` - print help message

```
> .help
.help                    Print this help message
.info                    Print system info
.model                   Switch LLM model
.role                    Use a role
.info role               Show the role info
.exit role               Leave current role
.session                 Start a context-aware chat session
.info session            Show the session info
.save session            Save the session to the file
.clear messages          Clear messages in the session
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
save_session        true
highlight           true
light_theme         false
wrap                no
wrap_code           false
auto_copy           false
keybindings         emacs
prelude             -
compress_threshold  1000
config_file         /home/alice/.config/aichat/config.yaml
roles_file          /home/alice/.config/aichat/roles.yaml
messages_file       /home/alice/.config/aichat/messages.md
sessions_dir        /home/alice/.config/aichat/sessions
```

### `.model` - choose a model

```
> .model openai:gpt-4
> .model ollama:llama2
```

> You can easily enter model name using autocomplete.

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
.set temperature 1.2
.set compress_threshold 1000
.set dry_run true
.set highlight false
.set save false
.set save_session true
.set auto_copy true
```

### Roles

We can define a batch of roles in `roles.yaml`.

> Retrieve the location of `roles.yaml` through the REPL `.info` command or CLI `--info` option.

For example, we can define a role:

```yaml
- name: shell
  prompt: >
    I want you to act as a Linux shell expert.
    I want you to answer only with bash code.
    Do not provide explanations.
```

Let LLM answer questions in the role of a Linux shell expert.

```
> .role shell

shell>  extract encrypted zipfile app.zip to /tmp/app
mkdir /tmp/app
unzip -P PASSWORD app.zip -d /tmp/app
```

For more details about roles, please visit [Role Guide](https://github.com/sigoden/aichat/wiki/Role-Guide).

## License

Copyright (c) 2023-2024 aichat-developers.

Aichat is made available under the terms of either the MIT License or the Apache License 2.0, at your option.

See the LICENSE-APACHE and LICENSE-MIT files for license details.
