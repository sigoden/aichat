complete -F _aichat aichat

_aichat() {
  local cur prev words cword
  _init_completion || return

  local options_with_args=(
    '-m --model'
    '-r --role'
    '-s --session'
    '-f --file'
    '-w --wrap'
  )

  local options_without_args=(
    '-H --no-highlight'
    '-S --no-stream'
    '-e --execute'
    '-c --code'
    '--save-session'
    '--light-theme'
    '--dry-run'
    '--info'
    '--list-models'
    '--list-roles'
    '--list-sessions'
    '-h --help'
    '-V --version'
  )

  case "$prev" in
    -m|--model)
		  local IFS=$'\n'
      COMPREPLY=($(compgen -W "$(aichat --list-models)" -- "$cur"))
      return 0
      ;;
    -r|--role)
		  local IFS=$'\n'
      COMPREPLY=($(compgen -W "$(aichat --list-roles)" -- "$cur"))
      return 0
      ;;
    -s|--session)
		  local IFS=$'\n'
      COMPREPLY=($(compgen -W "$(aichat --list-sessions)" -- "$cur"))
      return 0
      ;;
    -f|--file)
      _filedir
      return 0
      ;;
    -w|--wrap)
      COMPREPLY=($(compgen -W "no auto" -- "$cur"))
      return 0
      ;;
  esac

  if [[ "$cur" == -* ]]; then
    COMPREPLY=($(compgen -W "${options_with_args[*]} ${options_without_args[*]}" -- "$cur"))
  fi
}
