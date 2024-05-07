module completions {

  def "nu-complete aichat completions" [] {
    [ "bash" "zsh" "fish" "powershell" "nushell" ]
  }

  def "nu-complete aichat model" [] {
    ^aichat --list-models |
    | lines 
    | parse "{value}" 
  }

  def "nu-complete aichat role" [] {
    ^aichat --list-roles |
    | lines 
    | parse "{value}" 
  }

  def "nu-complete aichat session" [] {
    ^aichat --list-sessions |
    | lines 
    | parse "{value}" 
  }

  # All-in-one chat and copilot CLI that integrates 10+ AI platforms
  export extern aichat [
    --model(-m): string@"nu-complete aichat model"    # Select a LLM model
    --prompt                                          # Use the system prompt
    --role(-r): string@"nu-complete aichat role"      # Select a role
    --session(-s): string@"nu-complete aichat role"   # Start or join a session
    --save-session                                    # Forces the session to be saved
    --serve                                           # Serve the LLM API and WebAPP
    --execute(-e)                                     # Execute commands in natural language
    --code(-c)                                        # Output code only
    --file(-f): string                                # Include files with the message
    --no-highlight(-H)                                # Turn off syntax highlighting
    --no-stream(-S)                                   # Turns off stream mode
    --wrap(-w): string                                # Control text wrapping (no, auto, <max-width>)
    --light-theme                                     # Use light theme
    --dry-run                                         # Display the message without sending it
    --info                                            # Display information
    --list-models                                     # List all available models
    --list-roles                                      # List all available roles
    --list-sessions                                   # List all available sessions
    ...text: string                                   # Input text
    --help(-h)                                        # Print help
    --version(-V)                                     # Print version
  ]

}

export use completions *
