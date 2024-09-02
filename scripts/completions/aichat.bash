_aichat() {
    local cur prev words cword i opts cmd
    COMPREPLY=()

    _get_comp_words_by_ref -n : cur prev words cword

    for i in ${words[@]}
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="aichat"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        aichat)
            opts="-m -r -s -a -R -e -c -f -S -h -V --model --prompt --role --session --save-session --agent --rag --serve --execute --code --file --no-stream --dry-run --info --list-models --list-roles --list-sessions --list-agents --list-rags --help --version"
            if [[ ${cur} == -* || ${cword} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi

            case "${prev}" in
                -m|--model)
                    COMPREPLY=($(compgen -W "$("$1" --list-models)" -- "${cur}"))
                    __ltrim_colon_completions "$cur"
                    return 0
                    ;;
                --prompt)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -r|--role)
                    COMPREPLY=($(compgen -W "$("$1" --list-roles)" -- "${cur}"))
                    __ltrim_colon_completions "$cur"
                    return 0
                    ;;
                -s|--session)
                    COMPREPLY=($(compgen -W "$("$1" --list-sessions)" -- "${cur}"))
                    __ltrim_colon_completions "$cur"
                    return 0
                    ;;
                -a|--agent)
                    COMPREPLY=($(compgen -W "$("$1" --list-agents)" -- "${cur}"))
                    __ltrim_colon_completions "$cur"
                    return 0
                    ;;
                -R|--rag)
                    COMPREPLY=($(compgen -W "$("$1" --list-rags)" -- "${cur}"))
                    __ltrim_colon_completions "$cur"
                    return 0
                    ;;
                -f|--file)
                    local oldifs
                    if [[ -v IFS ]]; then
                        oldifs="$IFS"
                    fi
                    IFS=$'\n'
                    COMPREPLY=($(compgen -f "${cur}"))
                    if [[ -v oldifs ]]; then
                        IFS="$oldifs"
                    fi
                    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
                        compopt -o filenames
                    fi
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _aichat -o nosort -o bashdefault -o default aichat
else
    complete -F _aichat -o bashdefault -o default aichat
fi
