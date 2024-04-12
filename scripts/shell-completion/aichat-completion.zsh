#compdef aichat

if (( CURRENT < 2 )); then
  return
fi

# Function to escape colons in array elements
function escape_colons_in_array() {
  local -a array=("$@")
  local -a escaped_array=()
  for element in "${array[@]}"; do
    # Replace ':' with '\:' in each element
    escaped_array+=("${element//:/\\:}")
  done
  echo "${escaped_array[@]}"
}

_arguments -s -S : \
  '(-m --model)'{-m,--model}'[Choose a LLM model]:model:'"($(escape_colons_in_array "${(@f)"$(aichat --list-models)"}"))" \
  '(-r --role)'{-r,--role}'[Choose a role]:role:'"($(escape_colons_in_array "${(@f)"$(aichat --list-roles)"}"))" \
  '(-s --session)'{-s,--session}'[Create or reuse a session]:session:'"($(escape_colons_in_array "${(@f)"$(aichat --list-sessions)"}"))" \
  '(-f --file)'{-f,--file}'[Attach files to the message to be sent]:file:_files' \
  '(-H --no-highlight)'{-H,--no-highlight}'[Disable syntax highlighting]' \
  '(-S --no-stream)'{-S,--no-stream}'[No stream output]' \
  '(-w --wrap)'{-w,--wrap}'[Specify the text-wrapping mode among no, auto or <textwidth>]:wrap mode:'"(no auto)" \
  '(-e --execute)'{-e,--execute}'[Execute commands using natural language]' \
  '(-c --code)'{-c,--code}'[Generate only code]' \
  '--save-session[Whether to save the session]' \
  '--light-theme[Use light theme]' \
  '--dry-run[Run in dry run mode]' \
  '--info[Print related information]' \
  '--list-models[List all available models]' \
  '--list-roles[List all available roles]' \
  '--list-sessions[List all available sessions]' \
  '(-h --help)'{-h,--help}'[Print help]' \
  '(-V --version)'{-V,--version}'[Print version]' \
  '*::arg:->args'
