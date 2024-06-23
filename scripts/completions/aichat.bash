_aichat() {
    local i cur prev opts cmd
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    cmd=""
    opts=""

    for i in ${COMP_WORDS[@]}
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
            opts="-m -r -s -a -R -e -c -f -S -w -H -h -V --model --prompt --role --session --save-session --agent --rag --serve --execute --code --file --no-stream --wrap --no-highlight --light-theme --dry-run --info --list-models --list-roles --list-sessions --list-agents --list-rags --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi

            # hacking -m or --model completion value that contains colon
            if [ "$cur" == ":" ] || [ "$prev" == ":" ]; then
                local option client
                if [ "$cur" = ":" ]; then
                    option="${COMP_WORDS[COMP_CWORD-2]}"
                    client="$prev"
                else
                    option="${COMP_WORDS[COMP_CWORD-3]}"
                    client="${COMP_WORDS[COMP_CWORD-2]}"
                fi
                if [ "$option" == "-m" ] || [ "$flag" == "--model" ]; then
                    if [ "$cur" == ":" ]; then
                        cur=""
                    fi
                    COMPREPLY=($(compgen -W "$("$1" --list-models | sed -n '/'"${client}"'/ s/'"${client%:*}"'://p')" -- "${cur}"))
                    return 0
                fi
            fi

            case "${prev}" in
                -m|--model)
                    COMPREPLY=($(compgen -W "$("$1" --list-models)" -- "${cur}"))
                    return 0
                    ;;
                --prompt)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -r|--role)
                    COMPREPLY=($(compgen -W "$("$1" --list-roles)" -- "${cur}"))
                    return 0
                    ;;
                -s|--session)
                    COMPREPLY=($(compgen -W "$("$1" --list-sessions)" -- "${cur}"))
                    return 0
                    ;;
                -a|--agent)
                    COMPREPLY=($(compgen -W "$("$1" --list-agents)" -- "${cur}"))
                    return 0
                    ;;
                -R|--rag)
                    COMPREPLY=($(compgen -W "$("$1" --list-rags)" -- "${cur}"))
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
                -w|--wrap)
                    COMPREPLY=($(compgen -f "${cur}"))
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
