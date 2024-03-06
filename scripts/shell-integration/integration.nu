def _aichat_nushell [] {
    let _prev = (commandline)
    if ($_prev != "") {
        print '⌛'
        commandline edit -r (aichat -e $_prev)
    }
}

$env.config.keybindings = ($env.config.keybindings | append {
        name: aichat_integration
        modifier: alt
        keycode: char_e
        mode: [emacs, vi_insert]
        event:[
            {
                send: executehostcommand,
                cmd: "_aichat_nushell"
            }
        ]
    }
)