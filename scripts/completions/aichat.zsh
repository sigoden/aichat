#compdef aichat

autoload -U is-at-least

_aichat() {
    typeset -A opt_args
    typeset -a _arguments_options
    local ret=1

    if is-at-least 5.2; then
        _arguments_options=(-s -S -C)
    else
        _arguments_options=(-s -C)
    fi

    local context curcontext="$curcontext" state line
     local common=(
'-m+[Select a LLM model]:MODEL:->models' \
'--prompt=[Use the system prompt]:PROMPT: ' \
'--model=[Select a LLM model]:MODEL:->models' \
'-r+[Select a role]:ROLE:->roles' \
'--role=[Select a role]:ROLE:->roles' \
'-s+[Start or join a session]:SESSION:->sessions' \
'--session=[Start or join a session]:SESSION:->sessions' \
'*-f+[Include files with the message]:FILE:_files' \
'*--file=[Include files with the message]:FILE:_files' \
'-w+[Control text wrapping (no, auto, <max-width>)]:WRAP: ' \
'--wrap=[Control text wrapping (no, auto, <max-width>)]:WRAP: ' \
'--save-session[Forces the session to be saved]' \
'--serve[Serve the LLM API and WebAPP]' \
'-e[Execute commands in natural language]' \
'--execute[Execute commands in natural language]' \
'-c[Output code only]' \
'--code[Output code only]' \
'-H[Turn off syntax highlighting]' \
'--no-highlight[Turn off syntax highlighting]' \
'-S[Turns off stream mode]' \
'--no-stream[Turns off stream mode]' \
'--light-theme[Use light theme]' \
'--dry-run[Display the message without sending it]' \
'--info[Display information]' \
'--list-models[List all available models]' \
'--list-roles[List all available roles]' \
'--list-sessions[List all available sessions]' \
'-h[Print help]' \
'--help[Print help]' \
'-V[Print version]' \
'--version[Print version]' \
'*::text -- Input text:' \
    )


    _arguments "${_arguments_options[@]}" $common \
        && ret=0 
    case $state in
        models|roles|sessions)
            local -a values expl
            values=( ${(f)"$(_call_program values aichat --list-$state)"} )
            _wanted values expl $state compadd -a values && ret=0
            ;;
    esac
    return ret
}

(( $+functions[_aichat_commands] )) ||
_aichat_commands() {
    local commands; commands=()
    _describe -t commands 'aichat commands' commands "$@"
}

if [ "$funcstack[1]" = "_aichat" ]; then
    _aichat "$@"
else
    compdef _aichat aichat
fi
