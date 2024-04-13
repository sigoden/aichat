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
'-m+[Choose a LLM model]:MODEL:->models' \
'--model=[Choose a LLM model]:MODEL:->models' \
'-r+[Choose a role]:ROLE:->roles' \
'--role=[Choose a role]:ROLE:->roles' \
'-s+[Create or reuse a session]:SESSION:->sessions' \
'--session=[Create or reuse a session]:SESSION:->sessions' \
'*-f+[Attach files to the message]:FILE:_files' \
'*--file=[Attach files to the message]:FILE:_files' \
'-w+[Specify the text-wrapping mode (no, auto, <max-width>)]:WRAP: ' \
'--wrap=[Specify the text-wrapping mode (no, auto, <max-width>)]:WRAP: ' \
'--save-session[Whether to save the session]' \
'-e[Execute commands using natural language]' \
'--execute[Execute commands using natural language]' \
'-c[Generate only code]' \
'--code[Generate only code]' \
'-H[Disable syntax highlighting]' \
'--no-highlight[Disable syntax highlighting]' \
'-S[No stream output]' \
'--no-stream[No stream output]' \
'--light-theme[Use light theme]' \
'--dry-run[Run in dry run mode]' \
'--info[Print related information]' \
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
