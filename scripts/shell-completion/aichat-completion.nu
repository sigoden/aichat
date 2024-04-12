def aichat-completion [
  line: string,
  pos: int,
  command: string,
  flag: string
] {
  match $flag {
    '--model' { (aichat --list-models | lines) }
    '--role' { (aichat --list-roles | lines) }
    '--session' { (aichat --list-sessions | lines) }
    '--file' { ls | get path | str replace -r '^.*/' '' }
    '--wrap' { ['no', 'auto'] }
    { ['--model', '--role', '--session', '--file', '--no-highlight', '--save-session', '-e', '--execute', '-c', '--code', '--no-stream', '--wrap', '--light-theme', '--dry-run', '--info', '--list-models', '--list-roles', '--list-sessions', '--help', '--version'] }
  }
}

completion add aichat aichat-completion
