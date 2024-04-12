function __fish_aichat_complete
  complete -c aichat -s m -l model -d "Choose a LLM model" -xa '(aichat --list-models)'
  complete -c aichat -s r -l role -d "Choose a role" -xa '(aichat --list-roles)'
  complete -c aichat -s s -l session -d "Create or reuse a session" -xa '(aichat --list-sessions)'
  complete -c aichat -s w -l wrap -d "Specify the text-wrapping mode among no, auto or <textwidth>" -xa "no auto"
  complete -c aichat -s f -l file -d "Attach files to the message to be sent" -xa "_files"
  complete -c aichat -s H -l no-highlight -d "Disable syntax highlighting"
  complete -c aichat -s S -l no-stream -d "No stream output"
  complete -c aichat -s e -l execute -d "Execute commands using natural language"
  complete -c aichat -s c -l code -d "Generate only code"
  complete -c aichat -l save-session -d "Whether to save the session"
  complete -c aichat -l light-theme -d "Use light theme"
  complete -c aichat -l dry-run -d "Run in dry run mode"
  complete -c aichat -l info -d "Print related information"
  complete -c aichat -l list-models -d "List all available models"
  complete -c aichat -l list-roles -d "List all available roles"
  complete -c aichat -l list-sessions -d "List all available sessions"
  complete -c aichat -s h -l help -d "Print help"
  complete -c aichat -s V -l version -d "Print version"
  complete -c aichat -n "__fish_use_subcommand" -xa "args"
end

complete -f -c aichat -a '(__fish_aichat_complete)'
