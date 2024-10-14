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
'-m[Select a LLM model]:MODEL:->models' \
'--model[Select a LLM model]:MODEL:->models' \
'--prompt[Use the system prompt]:PROMPT: ' \
'-r[Select a role]:ROLE:->roles' \
'--role[Select a role]:ROLE:->roles' \
'-s[Start or join a session]:SESSION:->sessions' \
'--session[Start or join a session]:SESSION:->sessions' \
'--empty-session[Ensure the session is empty]' \
'--save-session[Force the session to be saved]' \
'-a[Start a agent]:AGENT:->agents' \
'--agent[Start a agent]:AGENT:->agents' \
'-R[Start a RAG]:RAG:->rags' \
'--rag[Start a RAG]:RAG:->rags' \
'--serve[Serve the LLM API and WebAPP]' \
'-e[Execute commands in natural language]' \
'--execute[Execute commands in natural language]' \
'-c[Output code only]' \
'--code[Output code only]' \
'*-f[Include files with the message]:FILE:_files' \
'*--file[Include files with the message]:FILE:_files' \
'-S[Turn off stream mode]' \
'--no-stream[Turn off stream mode]' \
'--dry-run[Display the message without sending it]' \
'--info[Display information]' \
'--list-models[List all available chat models]' \
'--list-roles[List all roles]' \
'--list-sessions[List all sessions]' \
'--list-agents[List all agents]' \
'--list-rags[List all RAGs]' \
'-h[Print help]' \
'--help[Print help]' \
'-V[Print version]' \
'--version[Print version]' \
'*::text -- Input text:' \
    )


    _arguments "${_arguments_options[@]}" $common \
        && ret=0 
    case $state in
        models|roles|sessions|agents|rags)
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
