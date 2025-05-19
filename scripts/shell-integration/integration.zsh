_aichat_zsh() {
    if [[ -n "$BUFFER" ]]; then
        local _old=$BUFFER
        BUFFER+="⌛"
        zle -I && zle redisplay
        BUFFER=$(aichat -r '%shell%' "$_old")
        zle end-of-line
    fi
}
zle -N _aichat_zsh
bindkey '\ee' _aichat_zsh
