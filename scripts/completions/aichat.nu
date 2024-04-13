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
    --model(-m): string@"nu-complete aichat model"    # Choose a LLM model
    --role(-r): string@"nu-complete aichat role"      # Choose a role
    --session(-s): string@"nu-complete aichat role"   # Create or reuse a session
    --save-session                                    # Whether to save the session
    --execute(-e)                                     # Execute commands using natural language
    --code(-c)                                        # Generate only code
    --file(-f): string                                # Attach files to the message
    --no-highlight(-H)                                # Disable syntax highlighting
    --no-stream(-S)                                   # No stream output
    --wrap(-w): string                                # Specify the text-wrapping mode (no, auto, <max-width>)
    --light-theme                                     # Use light theme
    --dry-run                                         # Run in dry run mode
    --info                                            # Print related information
    --list-models                                     # List all available models
    --list-roles                                      # List all available roles
    --list-sessions                                   # List all available sessions
    ...text: string                                   # Input text
    --help(-h)                                        # Print help
    --version(-V)                                     # Print version
  ]

}

export use completions *
