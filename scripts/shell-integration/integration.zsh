_aichat_zsh() {
    if [[ -z "$BUFFER" ]]; then
        return 1
    fi

    local _aichat_quoted_arg=${(qq)BUFFER}

    BUFFER="aichat -e $_aichat_quoted_arg"

    zle accept-line
}
zle -N _aichat_zsh
bindkey '\ee' _aichat_zsh
