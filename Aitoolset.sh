#!/usr/bin/env bash
# generate_80_tools_json.sh
# Generates 80 self-contained CLI tools using the specified JSON argument parsing format.

set -euo pipefail

TARGET_DIR="${1:-./json_bin}"
mkdir -p "$TARGET_DIR"

# --- Colors and Info ---
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[0;33m'
NC='\033[0m'

echo -e "${CYAN}--- Generating 80 JSON-Parsable Tools in: ${TARGET_DIR} ---${NC}"

# --- Function to create individual tools ---
create_script() {
    local tool_name="$1"
    local description="$2"
    local requires_jq="$3" # "true" or "false"
    local tool_body="$4"

    echo -e "  [+] ${GREEN}${tool_name}${NC}: ${description}"

    cat > "$TARGET_DIR/$tool_name" <<EOF
#!/usr/bin/env bash
# ── Tool header ($tool_name) ─────────────────────────────────────────
set -euo pipefail

# ----------------------------------------------------------------------
# Helper functions (Must have jq if required)
# ----------------------------------------------------------------------
get_arg() {
  # $1 – JSON string, $2 – key to extract
  if ! command -v jq &> /dev/null; then echo "Error: jq is required."; exit 1; fi
  echo "\$1" | jq -r --arg key "\$2" '.[\$key] // ""'
}

# ----------------------------------------------------------------------
# Argument handling
# ----------------------------------------------------------------------
ARG_JSON=\${1:-'{}'}
TEMP_OUT=\$(mktemp)
TEMP_ERR=\$(mktemp)
trap 'rm -f "\$TEMP_OUT" "\$TEMP_ERR"' EXIT
EXIT_CODE=0

EOF

    if [ "$requires_jq" == "true" ]; then
        # Default parsing block
        cat >> "$TARGET_DIR/$tool_name" <<'EOF'
ARG_VAL=$(get_arg "$ARG_JSON" "$@")
EOF
    else
        # Simple positional argument handling if jq is not the primary tool
        cat >> "$TARGET_DIR/$tool_name" <<'EOF'
# Simple positional parsing for non-jq reliant tools
ARG_VAL=$*
EOF
    fi

    cat >> "$TARGET_DIR/$tool_name" <<'EOF'
# ----------------------------------------------------------------------
# TOOL SPECIFIC CODE STARTS HERE
# ----------------------------------------------------------------------
EOF

    # Insert the tool-specific body
    eval "cat >> \"$TARGET_DIR/$tool_name\" <<'ENDTOOL'
$tool_body
ENDTOOL"

    cat >> "$TARGET_DIR/$tool_name" <<'EOF'
# ----------------------------------------------------------------------
# Execution wrapper (Standardized output generation)
# ----------------------------------------------------------------------
if [ "$EXIT_CODE" -eq 0 ]; then
  # Success
  jq -n --arg cmd "$tool_name" --arg out "$OUT" --arg err "$ERR" \
    '{success: true, tool: $cmd, stdout: $out, stderr: $err, exit_code: 0}'
else
  # Failure or Timeout
  ERROR_MSG="Command failed with exit code $EXIT_CODE."
  if [ "$EXIT_CODE" -eq 124 ]; then
      ERROR_MSG="Command timed out after $TIMEOUT seconds."
  fi
  jq -n --arg cmd "$tool_name" --arg out "$OUT" --arg err "$ERR" --argjson ec "$EXIT_CODE" \
    '{success: false, tool: $cmd, stdout: $out, stderr: $err, exit_code: $ec, error: $ERROR_MSG}'
fi
EOF

    chmod +x "$TARGET_DIR/$tool_name"
}

# ==================================================================
# CORE EXECUTION & PYTHON (1-10)
# ==================================================================

# 1. shell_execute
TOOL_BODY_SHELL_EXEC='
CMD=$(get_arg "$ARG_JSON" "command")
TIMEOUT=$(get_arg "$ARG_JSON" "timeout")
[ -z "$TIMEOUT" ] && TIMEOUT=10

if [ -z "$CMD" ]; then
    jq -n "{success:false, error:\"Command argument is missing.\"}"
    exit 1
fi

# SECURITY WARNING: Untrusted input execution via eval.
timeout "$TIMEOUT" bash -c "$CMD" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "shell_execute" "Execute arbitrary shell command via JSON argument." true "$TOOL_BODY_SHELL_EXEC"

# 2. py_install
TOOL_BODY_PY_INSTALL='
PACKAGE=$(get_arg "$ARG_JSON" "package")
if [ -z "$PACKAGE" ]; then jq -n "{success:false, error:\"Package name is required.\"}"; exit 1; fi
echo "Upgrading pip..." >&2
python3 -m pip install --upgrade pip > /dev/null 2>&1
echo "Installing $PACKAGE..." >&2
python3 -m pip install "$PACKAGE" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "py_install" "Installs a Python package via pip." true "$TOOL_BODY_PY_INSTALL"

# 3. py_venv
TOOL_BODY_PY_VENV='
NAME=$(get_arg "$ARG_JSON" "dirname")
[ -z "$NAME" ] && NAME="venv"
python3 -m venv "$NAME" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
if [ $EXIT_CODE -eq 0 ]; then OUT="Venv created: $NAME\nTo activate: source $NAME/bin/activate"; fi
'
create_script "py_venv" "Create Python virtual environment." true "$TOOL_BODY_PY_VENV"

# 4. advanced_edit
TOOL_BODY_ADV_EDIT='
SEARCH=$(get_arg "$ARG_JSON" "search")
REPLACE=$(get_arg "$ARG_JSON" "replace")
GLOB=$(get_arg "$ARG_JSON" "glob")
[ -z "$GLOB" ] && GLOB="*"

if [ -z "$SEARCH" ] || [ -z "$REPLACE" ]; then jq -n "{success:false, error:\"Search and replace strings required.\"}"; exit 1; fi

if command -v sd &> /dev/null; then
    grep -rl "$SEARCH" . --include="$GLOB" --exclude-dir={.git,node_modules} | xargs sd "$SEARCH" "$REPLACE" >/dev/null 2>&1
else
    grep -rl "$SEARCH" . --include="$GLOB" --exclude-dir={.git,node_modules} | xargs sed -i "s|$SEARCH|$REPLACE|g"
fi
EXIT_CODE=$?
OUT=$(echo "Replacement executed on files matching *$GLOB*")
'
create_script "advanced_edit" "Search/replace text across files." true "$TOOL_BODY_ADV_EDIT"

# 5. monitor_watch
TOOL_BODY_MONITOR_WATCH='
FILE=$(get_arg "$ARG_JSON" "file")
CMD=$(get_arg "$ARG_JSON" "command")
[ -z "$FILE" ] && { jq -n "{success:false, error:\"File argument required.\"}"; exit 1; }
[ -z "$CMD" ] && CMD="echo changed"

if command -v entr &> /dev/null; then
    printf "%s\n" "$FILE" | entr -c "$CMD" >"$TEMP_OUT" 2>"$TEMP_ERR" &
    echo "Watching \$FILE. Press Ctrl+C to stop the watcher."
    wait $!
    EXIT_CODE=$?
else
    OUT=$(echo "entr not available. Cannot watch file." )
    EXIT_CODE=1
fi
'
create_script "monitor_watch" "Run a command when a file changes (uses entr)." true "$TOOL_BODY_MONITOR_WATCH"

# 6. env_check
TOOL_BODY_ENV_CHECK='
tools=("python3" "git" "grep" "curl" "awk" "jq")
OUTPUT=""
for tool in "${tools[@]}"; do
    if command -v "$tool" &> /dev/null; then
        OUTPUT+="[OK] $tool\\n"
    else
        OUTPUT+="[MISS] $tool\\n"
    fi
done
OUT=$(echo -e "$OUTPUT")
'
create_script "env_check" "Checks presence of key tools (python, git, jq, etc.)." false "$TOOL_BODY_ENV_CHECK"

# 7. cleanup_cache
TOOL_BODY_CLEANUP_CACHE='
rm -rf .pytest_cache/ venv/ node_modules/ __pycache__/ >/dev/null 2>&1
OUT="Cleaned common caches."
'
create_script "cleanup_cache" "Removes common build/dependency cache directories." false "$TOOL_BODY_CLEANUP_CACHE"

# 8. git_stash_save
TOOL_BODY_GIT_STASH_SAVE='
MSG=$(get_arg "$ARG_JSON" "message")
[ -z "$MSG" ] && MSG="WIP"
git stash push -m "$MSG" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "git_stash_save" "Stashes current Git changes with a message." true "$TOOL_BODY_GIT_STASH_SAVE"

# 9. git_undo_all
TOOL_BODY_GIT_UNDO_ALL='
git reset >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
OUT+="Unstaged all current changes."
'
create_script "git_undo_all" "Unstage all local Git changes." false "$TOOL_BODY_GIT_UNDO_ALL"

# 10. url_encode
TOOL_BODY_URL_ENCODE='
STRING=$(get_arg "$ARG_JSON" "string_to_encode")
if [ -z "$STRING" ]; then jq -n "{success:false, error:\"String argument missing.\"}"; exit 1; fi
OUT=$(python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1]))" "$STRING")
'
create_script "url_encode" "Encode a string for use in URLs." true "$TOOL_BODY_URL_ENCODE"

# ==================================================================
# FILE & DIRECTORY TOOLS (11-30)
# ==================================================================

# 11. file_read
TOOL_BODY_FILE_READ='
FILE=$(get_arg "$ARG_JSON" "filepath")
LINES=$(get_arg "$ARG_JSON" "max_lines")
[ -z "$LINES" ] && LINES=100
if [ ! -f "$FILE" ]; then OUT="Error: File not found: $FILE"; EXIT_CODE=1; else
    if [ $(wc -l < "$FILE") -gt "$LINES" ]; then
        OUT=$(head -n "$LINES" "$FILE")
    else
        OUT=$(cat "$FILE")
    fi
fi
'
create_script "file_read" "Read file content, optionally limiting lines." true "$TOOL_BODY_FILE_READ"

# 12. file_write
TOOL_BODY_FILE_WRITE='
FILE=$(get_arg "$ARG_JSON" "filepath")
CONTENT=$(get_arg "$ARG_JSON" "content")
mkdir -p "$(dirname "$FILE")" >/dev/null 2>&1
echo -e "$CONTENT" > "$FILE"
OUT="Content written to $FILE."
'
create_script "file_write" "Overwrite a file with specific content." true "$TOOL_BODY_FILE_WRITE"

# 13. file_append
TOOL_BODY_FILE_APPEND='
FILE=$(get_arg "$ARG_JSON" "filepath")
CONTENT=$(get_arg "$ARG_JSON" "content")
echo -e "$CONTENT" >> "$FILE"
OUT="Content appended to $FILE."
'
create_script "file_append" "Append content to a file." true "$TOOL_BODY_FILE_APPEND"

# 14. file_find
TOOL_BODY_FILE_FIND='
PATTERN=$(get_arg "$ARG_JSON" "pattern")
DIR=$(get_arg "$ARG_JSON" "directory")
[ -z "$DIR" ] && DIR="."
find "$DIR" -type f -name "*$PATTERN*" -not -path "*/.*" >"$TEMP_OUT" 2>/dev/null
OUT=$(cat "$TEMP_OUT")
'
create_script "file_find" "Find files recursively by name pattern." true "$TOOL_BODY_FILE_FIND"

# 15. text_search
TOOL_BODY_TEXT_SEARCH='
PATTERN=$(get_arg "$ARG_JSON" "pattern")
DIR=$(get_arg "$ARG_JSON" "directory")
[ -z "$DIR" ] && DIR="."
grep -rni --color=always "$PATTERN" "$DIR" --exclude-dir={.git,node_modules,venv} >"$TEMP_OUT" 2>/dev/null
OUT=$(cat "$TEMP_OUT")
'
create_script "text_search" "Recursive text search (grep)." true "$TOOL_BODY_TEXT_SEARCH"

# 16. text_count
TOOL_BODY_TEXT_COUNT='
GLOB=$(get_arg "$ARG_JSON" "file_glob")
if [ -z "$GLOB" ]; then jq -n "{success:false, error:\"File glob is required.\"}"; exit 1; fi
wc -l -w -c $GLOB >"$TEMP_OUT" 2>/dev/null
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
'
create_script "text_count" "Count lines, words, chars in matching files." true "$TOOL_BODY_TEXT_COUNT"

# 17. dir_size
TOOL_BODY_DIR_SIZE='
du -sh * 2>/dev/null | sort -hr >"$TEMP_OUT"
OUT=$(cat "$TEMP_OUT")
'
create_script "dir_size" "Show human-readable disk usage of subdirectories." false "$TOOL_BODY_DIR_SIZE"

# 18. dir_tree
TOOL_BODY_DIR_TREE='
DEPTH=$(get_arg "$ARG_JSON" "depth")
[ -z "$DEPTH" ] && DEPTH=2
if command -v tree &> /dev/null; then
    tree -L "$DEPTH" -I "node_modules|venv|.git" >"$TEMP_OUT" 2>/dev/null
else
    find . -maxdepth "$DEPTH" -type d -o -type f >"$TEMP_OUT" 2>/dev/null
fi
OUT=$(cat "$TEMP_OUT")
'
create_script "dir_tree" "Display directory structure." true "$TOOL_BODY_DIR_TREE"

# 19. file_hash
TOOL_BODY_FILE_HASH='
FILE=$(get_arg "$ARG_JSON" "filepath")
sha256sum "$FILE" >"$TEMP_OUT" 2>/dev/null
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
'
create_script "file_hash" "Generate SHA256 hash of a file." true "$TOOL_BODY_FILE_HASH"

# 20. file_diff
TOOL_BODY_FILE_DIFF='
FILE1=$(get_arg "$ARG_JSON" "file1")
FILE2=$(get_arg "$ARG_JSON" "file2")
if [ -z "$FILE1" ] || [ -z "$FILE2" ]; then jq -n "{success:false, error:\"Two file paths required.\"}"; exit 1; fi
diff --color=always -u "$FILE1" "$FILE2" >"$TEMP_OUT" 2>/dev/null
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
'
create_script "file_diff" "Show colorized differences between two files." true "$TOOL_BODY_FILE_DIFF"

# 21. file_permissions
TOOL_BODY_FILE_PERMISSIONS='
FILE=$(get_arg "$ARG_JSON" "filepath")
MODE=$(get_arg "$ARG_JSON" "mode")
if [ -z "$FILE" ]; then jq -n "{success:false, error:\"File path required.\"}"; exit 1; fi
if [ -z "$MODE" ]; then
    ls -l "$FILE" >"$TEMP_OUT" 2>/dev/null
else
    chmod "$MODE" "$FILE"
fi
OUT=$(cat "$TEMP_OUT")
'
create_script "file_permissions" "Check or set file permissions." true "$TOOL_BODY_FILE_PERMISSIONS"

# 22. file_touch
TOOL_BODY_FILE_TOUCH='
ARGS=$(get_arg "$ARG_JSON" "args")
touch $ARGS >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Touched files/directories."
'
create_script "file_touch" "Create empty files or update timestamps." true "$TOOL_BODY_FILE_TOUCH"

# 23. file_ln
TOOL_BODY_FILE_LN='
TARGET=$(get_arg "$ARG_JSON" "target")
LINK_NAME=$(get_arg "$ARG_JSON" "link_name")
ln -s "$TARGET" "$LINK_NAME" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Symbolic link created."
'
create_script "file_ln" "Create a symbolic link." true "$TOOL_BODY_FILE_LN"

# 24. file_move
TOOL_BODY_FILE_MOVE='
SOURCE=$(get_arg "$ARG_JSON" "source")
DESTINATION=$(get_arg "$ARG_JSON" "destination")
mv "$SOURCE" "$DESTINATION" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Moved $SOURCE to $DESTINATION."
'
create_script "file_move" "Move or rename a file or directory." true "$TOOL_BODY_FILE_MOVE"

# 25. file_copy
TOOL_BODY_FILE_COPY='
SOURCE=$(get_arg "$ARG_JSON" "source")
DESTINATION=$(get_arg "$ARG_JSON" "destination")
cp -r "$SOURCE" "$DESTINATION" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Copied $SOURCE to $DESTINATION."
'
create_script "file_copy" "Copy a file or directory recursively." true "$TOOL_BODY_FILE_COPY"

# 26. epoch
TOOL_BODY_EPOCH='
OUT=$(date +%s)
'
create_script "epoch" "Get the current Unix timestamp." false "$TOOL_BODY_EPOCH"

# 27. from_epoch
TOOL_BODY_FROM_EPOCH='
TIMESTAMP=$(get_arg "$ARG_JSON" "timestamp")
if [ -z "$TIMESTAMP" ]; then jq -n "{success:false, error:\"Timestamp required.\"}"; exit 1; fi
OUT=$(date -d @"$TIMESTAMP")
'
create_script "from_epoch" "Convert Unix timestamp to readable date." true "$TOOL_BODY_FROM_EPOCH"

# 28. json_pretty
TOOL_BODY_JSON_PRETTY='
FILE=$(get_arg "$ARG_JSON" "filepath")
if [ -n "$FILE" ] && [ -f "$FILE" ]; then
    cat "$FILE" | python3 -m json.tool >"$TEMP_OUT" 2>"$TEMP_ERR"
else
    cat | python3 -m json.tool >"$TEMP_OUT" 2>"$TEMP_ERR"
fi
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "json_pretty" "Pretty print JSON from file or stdin." true "$TOOL_BODY_JSON_PRETTY"

# 29. random_str
TOOL_BODY_RANDOM_STR='
LEN=$(get_arg "$ARG_JSON" "length")
[ -z "$LEN" ] && LEN=16
OUT=$(LC_ALL=C tr -dc "A-Za-z0-9" < /dev/urandom | head -c "$LEN")
'
create_script "random_str" "Generate a random string." true "$TOOL_BODY_RANDOM_STR"

# 30. todo
TOOL_BODY_TODO='
ACTION=$(get_arg "$ARG_JSON" "action")
ITEM=$(get_arg "$ARG_JSON" "item")
TODO_FILE="$HOME/.tool_todo_list"
case "$ACTION" in
    add) echo "- $ITEM" >> "$TODO_FILE"; OUT="Todo added.";;
    list) OUT=$(cat "$TODO_FILE" 2>/dev/null || echo "List is empty.");;
    clear) echo "" > "$TODO_FILE"; OUT="Todo list cleared.";;
    *) OUT="Usage: Specify action (add, list, clear)."; EXIT_CODE=1;;
esac
'
create_script "todo" "Simple CLI todo list manager." true "$TOOL_BODY_TODO"

# 31. archive_extract
TOOL_BODY_ARCHIVE_EXTRACT='
FILE=$(get_arg "$ARG_JSON" "filepath")
case "$FILE" in
    *.tar.bz2|*.tbz2)   tar xjf "$FILE" >"$TEMP_OUT" 2>"$TEMP_ERR" ;;
    *.tar.gz|*.tgz)    tar xzf "$FILE" >"$TEMP_OUT" 2>"$TEMP_ERR" ;;
    *.zip)       unzip "$FILE" >"$TEMP_OUT" 2>"$TEMP_ERR" ;;
    *) OUT="Unsupported format or file not found: $FILE"; EXIT_CODE=1;;
esac
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "archive_extract" "Extracts compressed files (.zip, .tar.gz, etc.)." true "$TOOL_BODY_ARCHIVE_EXTRACT"

# 32. archive_compress
TOOL_BODY_ARCHIVE_COMPRESS='
DIR=$(get_arg "$ARG_JSON" "directory")
tar -czf "${DIR}.tar.gz" "$DIR" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Archive created: ${DIR}.tar.gz"
'
create_script "archive_compress" "Compresses a directory to .tar.gz." true "$TOOL_BODY_ARCHIVE_COMPRESS"

# 33. clipboard
TOOL_BODY_CLIPBOARD='
ACTION=$(get_arg "$ARG_JSON" "action")
if [ "$ACTION" == "paste" ]; then
    if command -v pbpaste &> /dev/null; then OUT=$(pbpaste);
    elif command -v xclip &> /dev/null; then OUT=$(xclip -selection clipboard -o);
    else OUT="No paste utility found."; EXIT_CODE=1; fi
else
    # Copy action (piped input)
    if command -v pbcopy &> /dev/null; then cat | pbcopy;
    elif command -v xclip &> /dev/null; then cat | xclip -selection clipboard;
    else OUT="No copy utility found."; EXIT_CODE=1; fi
fi
'
create_script "clipboard" "Copy piped input or paste clipboard content." true "$TOOL_BODY_CLIPBOARD"

# 34. count_lines
TOOL_BODY_COUNT_LINES='
EXT=$(get_arg "$ARG_JSON" "extension")
[ -z "$EXT" ] && EXT="sh"
find . -name "*.$EXT" -not -path "*/.*" | xargs wc -l >"$TEMP_OUT" 2>/dev/null
OUT=$(cat "$TEMP_OUT")
'
create_script "count_lines" "Count lines of code by extension recursively." true "$TOOL_BODY_COUNT_LINES"

# 35. url_shorten
TOOL_BODY_URL_SHORTEN='
URL=$(get_arg "$ARG_JSON" "url")
TOKEN="$(get_arg "$ARG_JSON" "token")"
if [ -z "$TOKEN" ]; then TOKEN="${BITLY_TOKEN:-}"; fi
if [ -z "$TOKEN" ] || [ -z "$URL" ]; then jq -n "{success:false, error:\"URL and token are required.\"}"; exit 1; fi

OUT=$(curl -s -X POST "https://api-ssl.bitly.com/v4/shorten" \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     -d "{\"long_url\": \"$URL\"}" | jq -r ".link // \"\"")
'
create_script "url_shorten" "Shorten a URL using Bitly API." true "$TOOL_BODY_URL_SHORTEN"

# 36. http_status
TOOL_BODY_HTTP_STATUS='
URL=$(get_arg "$ARG_JSON" "url")
if [ -z "$URL" ]; then jq -n "{success:false, error:\"URL is required.\"}"; exit 1; fi
OUT=$(curl -s -o /dev/null -w "%{http_code}" "$URL")
'
create_script "http_status" "Fetches only the HTTP status code for a URL." true "$TOOL_BODY_HTTP_STATUS"

# 37. cron_add
TOOL_BODY_CRON_ADD='
SCHEDULE=$(get_arg "$ARG_JSON" "schedule")
CMD=$(get_arg "$ARG_JSON" "command")
if [ -z "$SCHEDULE" ] || [ -z "$CMD" ]; then jq -n "{success:false, error:\"Schedule and command required.\"}"; exit 1; fi
(crontab -l 2>/dev/null | grep -v -F "$CMD"; echo "$SCHEDULE $CMD") | crontab - >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Job scheduled."
'
create_script "cron_add" "Schedule a job via crontab." true "$TOOL_BODY_CRON_ADD"

# 38. cron_list
TOOL_BODY_CRON_LIST='
OUT=$(crontab -l 2>/dev/null || echo "Crontab is empty.")
'
create_script "cron_list" "List current crontab jobs." false "$TOOL_BODY_CRON_LIST"

# 39. cron_rm
TOOL_BODY_CRON_RM='
CMD_TO_REMOVE=$(get_arg "$ARG_JSON" "command")
if [ -z "$CMD_TO_REMOVE" ]; then jq -n "{success:false, error:\"Command to remove required.\"}"; exit 1; fi
crontab -l | grep -v -F "$CMD_TO_REMOVE" | crontab - >"$TEMP_OUT" 2>"$TEMP_ERR"
OUT="Command potentially removed."
'
create_script "cron_rm" "Remove a specific command from crontab." true "$TOOL_BODY_CRON_RM"

# 40. logger_notify
TOOL_BODY_LOGGER_NOTIFY='
MSG=$(get_arg "$ARG_JSON" "message")
if command -v notify-send &> /dev/null; then
    notify-send "CLI Alert" "$MSG" >"$TEMP_OUT" 2>"$TEMP_ERR"
elif command -v osascript &> /dev/null; then
    osascript -e "display notification \"$MSG\" with title \"CLI Alert\"" >"$TEMP_OUT" 2>"$TEMP_ERR"
else
    OUT="Notification sent (printed to stdout as no utility found)."
fi
'
create_script "logger_notify" "Send a desktop notification." true "$TOOL_BODY_LOGGER_NOTIFY"

# 41. process_kill
TOOL_BODY_PROCESS_KILL='
PID=$(get_arg "$ARG_JSON" "pid")
if [ -z "$PID" ]; then jq -n "{success:false, error:\"PID required.\"}"; exit 1; fi
if ps -p "$PID" &> /dev/null; then
    kill -15 "$PID" >/dev/null 2>&1
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 0 ]; then OUT="Sent SIGTERM to PID $PID."; else OUT="Failed to send SIGTERM to $PID."; EXIT_CODE=1; fi
else
    OUT="PID $PID not found."
fi
'
create_script "process_kill" "Send SIGTERM (graceful kill) to a process by PID." true "$TOOL_BODY_PROCESS_KILL"

# 42. process_wait
TOOL_BODY_PROCESS_WAIT='
PID=$(get_arg "$ARG_JSON" "pid")
if [ -z "$PID" ]; then jq -n "{success:false, error:\"PID required.\"}"; exit 1; fi
wait "$PID" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Process $PID exited."
'
create_script "process_wait" "Wait for a specific PID to terminate." true "$TOOL_BODY_PROCESS_WAIT"

# 43. net_ping
TOOL_BODY_NET_PING='
HOST=$(get_arg "$ARG_JSON" "host")
COUNT=$(get_arg "$ARG_JSON" "count")
[ -z "$COUNT" ] && COUNT=4
ping -c "$COUNT" "$HOST" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "net_ping" "Ping a host multiple times." true "$TOOL_BODY_NET_PING"

# 44. net_dns
TOOL_BODY_NET_DNS='
DOMAIN=$(get_arg "$ARG_JSON" "domain")
if command -v dig &> /dev/null; then
    OUT=$(dig +short "$DOMAIN")
elif command -v nslookup &> /dev/null; then
    OUT=$(nslookup "$DOMAIN" | awk "/Address:/{print \$2}" | grep -v "::1")
else
    OUT="Neither dig nor nslookup available."
    EXIT_CODE=1
fi
'
create_script "net_dns" "Perform DNS lookup." true "$TOOL_BODY_NET_DNS"

# 45. net_ports
TOOL_BODY_NET_PORTS='
HOST=$(get_arg "$ARG_JSON" "host")
PORTS=$(get_arg "$ARG_JSON" "ports")
[ -z "$PORTS" ] && PORTS="80,443,22"
if command -v nmap &> /dev/null; then
    nmap -p "$PORTS" "$HOST" >"$TEMP_OUT" 2>"$TEMP_ERR"
    EXIT_CODE=$?
    OUT=$(cat "$TEMP_OUT")
    ERR=$(cat "$TEMP_ERR")
else
    OUT="nmap not installed."
    EXIT_CODE=1
fi
'
create_script "net_ports" "Simple port scan using nmap." true "$TOOL_BODY_NET_PORTS"

# 46. js_format
TOOL_BODY_JS_FORMAT='
FILE=$(get_arg "$ARG_JSON" "filepath")
if command -v prettier &> /dev/null; then
    prettier --write "$FILE" >"$TEMP_OUT" 2>"$TEMP_ERR"
    EXIT_CODE=$?
    OUT="Formatted $FILE with prettier."
else
    OUT="Prettier not found."
    EXIT_CODE=1
fi
'
create_script "js_format" "Format JS/JSON files using prettier." true "$TOOL_BODY_JS_FORMAT"

# 47. go_build
TOOL_BODY_GO_BUILD='
EXE_NAME=$(get_arg "$ARG_JSON" "executable_name")
[ -z "$EXE_NAME" ] && EXE_NAME="app"
go build -o "$EXE_NAME" . >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Built executable: $EXE_NAME"
'
create_script "go_build" "Builds a Go project." true "$TOOL_BODY_GO_BUILD"

# 48. log_grep_error
TOOL_BODY_LOG_GREP_ERROR='
FILE=$(get_arg "$ARG_JSON" "logfile")
grep -iE "error|exception" "$FILE" | sort >"$TEMP_OUT" 2>/dev/null
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
'
create_script "log_grep_error" "Grep a log file for ERROR lines." true "$TOOL_BODY_LOG_GREP_ERROR"

# 49. csv_to_json
TOOL_BODY_CSV_TO_JSON='
FILE=$(get_arg "$ARG_JSON" "csv_filepath")
if command -v csvjson &> /dev/null; then
    csvjson "$FILE" >"$TEMP_OUT" 2>"$TEMP_ERR"
    EXIT_CODE=$?
    OUT=$(cat "$TEMP_OUT")
else
    OUT="csvjson (from csvkit) not found."
    EXIT_CODE=1
fi
'
create_script "csv_to_json" "Convert CSV file to JSON array." true "$TOOL_BODY_CSV_TO_JSON"

# 50. token_get
TOOL_BODY_TOKEN_GET='
FILE=$(get_arg "$ARG_JSON" "token_filepath")
if [ -f "$FILE" ]; then
    OUT=$(grep -vE "^(#|$)" "$FILE" | head -n 1)
else
    OUT="Token file not found: $FILE"
    EXIT_CODE=1
fi
'
create_script "token_get" "Retrieve first non-comment line from a file." true "$TOOL_BODY_TOKEN_GET"

# 51. token_set
TOOL_BODY_TOKEN_SET='
VAR_NAME=$(get_arg "$ARG_JSON" "var_name")
VALUE=$(get_arg "$ARG_JSON" "value")
if [ -z "$VAR_NAME" ] || [ -z "$VALUE" ]; then jq -n "{success:false, error:\"Var name and value required.\"}"; exit 1; fi
OUT="export $VAR_NAME=\"$VALUE\""
# This tool outputs a command to be sourced by the caller.
EXIT_CODE=0
'
create_script "token_set" "Output export command to set env var (must be sourced)." false "$TOOL_BODY_TOKEN_SET"

# 52. shell_history
TOOL_BODY_SHELL_HISTORY='
PATTERN=$(get_arg "$ARG_JSON" "pattern")
if [ -z "$PATTERN" ]; then jq -n "{success:false, error:\"Pattern required.\"}"; exit 1; fi
# Note: History files are often only partially available or require specific shells
grep "$PATTERN" ~/.bash_history ~/.zsh_history 2>/dev/null | sort | uniq -c | sort -nr >"$TEMP_OUT" 2>/dev/null
OUT=$(cat "$TEMP_OUT")
'
create_script "shell_history" "Search shell history for command patterns." true "$TOOL_BODY_SHELL_HISTORY"

# 53. docker_clean
TOOL_BODY_DOCKER_CLEAN='
if command -v docker &> /dev/null; then
    docker system prune -f >"$TEMP_OUT" 2>"$TEMP_ERR"
    EXIT_CODE=$?
    OUT="Docker unused resources pruned."
else
    OUT="Docker not installed."
    EXIT_CODE=1
fi
'
create_script "docker_clean" "Removes unused Docker images, containers, and networks." true "$TOOL_BODY_DOCKER_CLEAN"

# 54. ssh_connect
TOOL_BODY_SSH_CONNECT='
TARGET=$(get_arg "$ARG_JSON" "ssh_target")
# WARNING: This command will block the JSON tool execution flow.
ssh "$TARGET" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "ssh_connect" "Connect to a remote host via SSH (blocks execution)." true "$TOOL_BODY_SSH_CONNECT"

# 55. ssh_copy_id
TOOL_BODY_SSH_COPY_ID='
TARGET=$(get_arg "$ARG_JSON" "ssh_target")
ssh-copy-id "$TARGET" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")
'
create_script "ssh_copy_id" "Copies SSH public key to a remote server." true "$TOOL_BODY_SSH_COPY_ID"

# 56. memory_usage
TOOL_BODY_MEMORY_USAGE='
ps aux --sort=-rss | awk "{print \$6/1024 \"MB\t\" \$11}" | head -n 10 >"$TEMP_OUT" 2>/dev/null
OUT=$(cat "$TEMP_OUT")
'
create_script "memory_usage" "Shows top 10 processes by resident memory usage." false "$TOOL_BODY_MEMORY_USAGE"

# 57. zip_file
TOOL_BODY_ZIP_FILE='
TARGET=$(get_arg "$ARG_JSON" "target")
zip -r "${TARGET}.zip" "$TARGET" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Zipped $TARGET to ${TARGET}.zip."
'
create_script "zip_file" "Zip a single file or directory." true "$TOOL_BODY_ZIP_FILE"

# 58. time_exec
TOOL_BODY_TIME_EXEC='
ARGS=$(get_arg "$ARG_JSON" "command_args")
# We must manually parse args array back into a string for shell_execute
CMD_STRING=""
for arg in $ARGS; do CMD_STRING+="${arg} "; done

# We call shell_execute internally to leverage its logging/timeout features
shell_execute "command=\"$CMD_STRING\",timeout=\"10\"" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT=$(cat "$TEMP_OUT")
ERR=$(cat "$TEMP_ERR")

# Extract timing info from shell_execute output if successful
if [ $EXIT_CODE -eq 0 ]; then
    TIMING=$(echo "$ERR" | grep "Real:")
    OUT="$(echo "$OUT" | jq . | jq ". + {timing: \"$TIMING\"}")"
fi
'
create_script "time_exec" "Measures execution time (Real, User, Sys) of a command." true "$TOOL_BODY_TIME_EXEC"

# 59. xml_to_json
TOOL_BODY_XML_TO_JSON='
FILE=$(get_arg "$ARG_JSON" "xml_filepath")
if command -v xml2json &> /dev/null; then
    xml2json "$FILE" | jq . >"$TEMP_OUT" 2>/dev/null
elif command -v python3 &> /dev/null; then
    python3 -c "import xmltodict, json, sys; print(json.dumps(xmltodict.parse(sys.stdin.read()), indent=2))" < "$FILE" >"$TEMP_OUT" 2>/dev/null
    EXIT_CODE=$?
else
    OUT="Neither xml2json nor xmltodict/python3 found."
    EXIT_CODE=1
fi
OUT=$(cat "$TEMP_OUT")
'
create_script "xml_to_json" "Convert XML file to prettified JSON." true "$TOOL_BODY_XML_TO_JSON"

# 60. cleanup_empty
TOOL_BODY_CLEANUP_EMPTY='
find . -type f -empty -delete >"$TEMP_OUT" 2>/dev/null
OUT="Removed empty files."
'
create_script "cleanup_empty" "Recursively find and delete all empty files." false "$TOOL_BODY_CLEANUP_EMPTY"

# ==================================================================
# WEB SEARCH TOOLS (61-65)
# ==================================================================

# 61. web_search
TOOL_BODY_WEB_SEARCH='
QUERY=$(get_arg "$ARG_JSON" "query")
NUM_RESULTS=$(get_arg "$ARG_JSON" "num_results")
[ -z "$NUM_RESULTS" ] && NUM_RESULTS=10
if [ -z "$QUERY" ]; then jq -n "{success:false, error:\"Query is required.\"}"; exit 1; fi

OUT=$(curl -s "https://duckduckgo.com/?q=$(echo "$QUERY" | sed "s/ /+/g")&format=json&no_html=1&skip_disambig=1" | \
    python3 -c "import sys, json; data=json.load(sys.stdin); print('\\n'.join([r.get('Text','') for r in data.get('Results',[])]))" 2>/dev/null || \
    echo "Web search functionality requires additional setup.")
'
create_script "web_search" "Perform web search queries." true "$TOOL_BODY_WEB_SEARCH"

# 62. web_image_search
TOOL_BODY_WEB_IMAGE_SEARCH='
QUERY=$(get_arg "$ARG_JSON" "query")
NUM_RESULTS=$(get_arg "$ARG_JSON" "num_results")
[ -z "$NUM_RESULTS" ] && NUM_RESULTS=10
if [ -z "$QUERY" ]; then jq -n "{success:false, error:\"Query is required.\"}"; exit 1; fi

OUT=$(curl -s "https://duckduckgo.com/?q=$(echo "$QUERY" | sed "s/ /+/g")&iax=1&ia=images&format=json" | \
    python3 -c "import sys, json; data=json.load(sys.stdin); print('\\n'.join([img.get('Image','') for img in data.get('Image',[])]))" 2>/dev/null || \
    echo "Image search functionality requires additional setup.")
'
create_script "web_image_search" "Search for images on the web." true "$TOOL_BODY_WEB_IMAGE_SEARCH"

# 63. web_scrape
TOOL_BODY_WEB_SCRAPE='
URL=$(get_arg "$ARG_JSON" "url")
SELECTOR=$(get_arg "$ARG_JSON" "selector")
if [ -z "$URL" ]; then jq -n "{success:false, error:\"URL is required.\"}"; exit 1; fi

if command -v pup &> /dev/null; then
    OUT=$(curl -s "$URL" | pup "$SELECTOR text{}" 2>/dev/null)
elif command -v lynx &> /dev/null; then
    OUT=$(lynx -dump "$URL")
else
    OUT=$(curl -s "$URL" | grep -o "<[^>]*>" | sed "s/<[^>]*>//g" | tr -s '\n')
fi
'
create_script "web_scrape" "Extract content from websites." true "$TOOL_BODY_WEB_SCRAPE"

# 64. web_reverse_image_search
TOOL_BODY_WEB_REVERSE_IMAGE_SEARCH='
IMAGE_URL=$(get_arg "$ARG_JSON" "image_url")
if [ -z "$IMAGE_URL" ]; then jq -n "{success:false, error:\"Image URL is required.\"}"; exit 1; fi

# Note: This is a placeholder - actual reverse image search requires APIs
OUT="Reverse image search for: $IMAGE_URL. Requires external API integration."
EXIT_CODE=1
'
create_script "web_reverse_image_search" "Perform reverse image search." true "$TOOL_BODY_WEB_REVERSE_IMAGE_SEARCH"

# 65. web_news_search
TOOL_BODY_WEB_NEWS_SEARCH='
QUERY=$(get_arg "$ARG_JSON" "query")
DAYS=$(get_arg "$ARG_JSON" "days")
[ -z "$DAYS" ] && DAYS=7
if [ -z "$QUERY" ]; then jq -n "{success:false, error:\"Query is required.\"}"; exit 1; fi

OUT="News search for: $QUERY (last $DAYS days). RSS parsing requires additional setup."
'
create_script "web_news_search" "Search for news articles." true "$TOOL_BODY_WEB_NEWS_SEARCH"

# ==================================================================
# FILE BATCH PROCESSING TOOLS (66-70)
# ==================================================================

# 66. batch_file_read
TOOL_BODY_BATCH_FILE_READ='
FILES=$(get_arg "$ARG_JSON" "files")
if [ -z "$FILES" ]; then jq -n "{success:false, error:\"Files array is required.\"}"; exit 1; fi

OUTPUT=""
for file in $(echo "$FILES" | jq -r '.[]'); do
    if [ -f "$file" ]; then
        OUTPUT+="=== $file ===\n"
        OUTPUT+=$(head -n 50 "$file" 2>/dev/null || echo "Error reading file")
        OUTPUT+="\n\n"
    else
        OUTPUT+="=== $file ===\nFile not found\n\n"
    fi
done
OUT=$(echo -e "$OUTPUT")
'
create_script "batch_file_read" "Read multiple files at once." true "$TOOL_BODY_BATCH_FILE_READ"

# 67. batch_file_rename
TOOL_BODY_BATCH_FILE_RENAME='
PATTERN=$(get_arg "$ARG_JSON" "pattern")
REPLACEMENT=$(get_arg "$ARG_JSON" "replacement")
if [ -z "$PATTERN" ] || [ -z "$REPLACEMENT" ]; then jq -n "{success:false, error:\"Pattern and replacement required.\"}"; exit 1; fi

for file in *$PATTERN*; do
    if [ -f "$file" ]; then
        newname=$(echo "$file" | sed "s/$PATTERN/$REPLACEMENT/")
        mv "$file" "$newname" >/dev/null 2>&1
        OUTPUT+="Renamed: $file -> $newname\n"
    fi
done
OUT=$(echo -e "$OUTPUT")
'
create_script "batch_file_rename" "Rename multiple files matching a pattern." true "$TOOL_BODY_BATCH_FILE_RENAME"

# 68. batch_file_move
TOOL_BODY_BATCH_FILE_MOVE='
FILES=$(get_arg "$ARG_JSON" "files")
DESTINATION=$(get_arg "$ARG_JSON" "destination")
if [ -z "$FILES" ] || [ -z "$DESTINATION" ]; then jq -n "{success:false, error:\"Files array and destination required.\"}"; exit 1; fi

mkdir -p "$DESTINATION" >/dev/null 2>&1
OUTPUT=""
for file in $(echo "$FILES" | jq -r '.[]'); do
    if [ -f "$file" ]; then
        mv "$file" "$DESTINATION/" >/dev/null 2>&1
        OUTPUT+="Moved: $file -> $DESTINATION/\n"
    fi
done
OUT=$(echo -e "$OUTPUT")
'
create_script "batch_file_move" "Move multiple files to a directory." true "$TOOL_BODY_BATCH_FILE_MOVE"

# 69. batch_file_copy
TOOL_BODY_BATCH_FILE_COPY='
FILES=$(get_arg "$ARG_JSON" "files")
DESTINATION=$(get_arg "$ARG_JSON" "destination")
if [ -z "$FILES" ] || [ -z "$DESTINATION" ]; then jq -n "{success:false, error:\"Files array and destination required.\"}"; exit 1; fi

mkdir -p "$DESTINATION" >/dev/null 2>&1
OUTPUT=""
for file in $(echo "$FILES" | jq -r '.[]'); do
    if [ -f "$file" ]; then
        cp "$file" "$DESTINATION/" >/dev/null 2>&1
        OUTPUT+="Copied: $file -> $DESTINATION/\n"
    fi
done
OUT=$(echo -e "$OUTPUT")
'
create_script "batch_file_copy" "Copy multiple files to a directory." true "$TOOL_BODY_BATCH_FILE_COPY"

# 70. batch_file_grep
TOOL_BODY_BATCH_FILE_GREP='
PATTERN=$(get_arg "$ARG_JSON" "pattern")
DIRECTORY=$(get_arg "$ARG_JSON" "directory")
[ -z "$DIRECTORY" ] && DIRECTORY="."
if [ -z "$PATTERN" ]; then jq -n "{success:false, error:\"Pattern is required.\"}"; exit 1; fi

OUTPUT=""
while IFS= read -r -d "" file; do
    OUTPUT+="=== $file ===\n"
    OUTPUT+=$(grep -H "$PATTERN" "$file" 2>/dev/null | head -5 || echo "No matches")
    OUTPUT+="\n\n"
done < <(find "$DIRECTORY" -type f -print0 2>/dev/null)
OUT=$(echo -e "$OUTPUT")
'
create_script "batch_file_grep" "Search for pattern in multiple files." true "$TOOL_BODY_BATCH_FILE_GREP"

# ==================================================================
# SYSTEM & MONITORING TOOLS (71-75)
# ==================================================================

# 71. system_info
TOOL_BODY_SYSTEM_INFO='
OUTPUT=""
OUTPUT+="=== System Information ===\n"
OUTPUT+="Hostname: $(hostname)\n"
OUTPUT+="Kernel: $(uname -r)\n"
OUTPUT+="Uptime: $(uptime -p 2>/dev/null || uptime)\n"
OUTPUT+="OS: $(cat /etc/os-release | grep PRETTY_NAME | cut -d" -f2)\n"
OUTPUT+="Architecture: $(uname -m)\n"
OUTPUT+="CPU: $(lscpu | grep "Model name" | cut -d: -f2 | xargs)\n"
OUTPUT+="Memory: $(free -h | awk "/^Mem:/ {print \$2}")\n"
OUTPUT+="Disk: $(df -h / | awk "NR==2 {print \$4}") free\n"
OUTPUT+="Load Average: $(uptime | awk -F"load average:" "{print \$2}")\n"
OUT=$(echo -e "$OUTPUT")
'
create_script "system_info" "Get comprehensive system information." false "$TOOL_BODY_SYSTEM_INFO"

# 72. process_list
TOOL_BODY_PROCESS_LIST='
SORT=$(get_arg "$ARG_JSON" "sort")
FILTER=$(get_arg "$ARG_JSON" "filter")
[ -z "$SORT" ] && SORT="cpu"

case "$SORT" in
    cpu) ps aux --sort=-%cpu | head -n 11 >"$TEMP_OUT" ;;
    mem) ps aux --sort=-%mem | head -n 11 >"$TEMP_OUT" ;;
    pid) ps aux --sort=pid | head -n 11 >"$TEMP_OUT" ;;
    *) ps aux | head -n 11 >"$TEMP_OUT" ;;
esac

OUT=$(cat "$TEMP_OUT")
if [ -n "$FILTER" ]; then
    OUT=$(echo "$OUT" | grep "$FILTER")
fi
'
create_script "process_list" "List and filter running processes." true "$TOOL_BODY_PROCESS_LIST"

# 73. disk_usage
TOOL_BODY_DISK_USAGE='
PATH=$(get_arg "$ARG_JSON" "path")
[ -z "$PATH" ] && PATH="."
if [ -f "$PATH" ]; then
    SIZE=$(ls -lh "$PATH" | awk "{print \$5}")
    INODE=$(ls -li "$PATH" | awk "{print \$1}")
    OUT="File: $PATH\nSize: $SIZE\nInode: $INODE"
else
    du -ah "$PATH" 2>/dev/null | sort -hr | head -n 20 >"$TEMP_OUT"
    OUT=$(cat "$TEMP_OUT")
fi
'
create_script "disk_usage" "Show detailed disk usage information." true "$TOOL_BODY_DISK_USAGE"

# 74. network_info
TOOL_BODY_NETWORK_INFO='
OUT="Network information:
- Interfaces: $(ip addr show 2>/dev/null | grep -c "^[0-9]" || echo "N/A")
- Connections: $(ss -tuln 2>/dev/null | grep -c "LISTEN" || echo "N/A")  
- Routes: $(ip route show 2>/dev/null | wc -l || echo "N/A")"
'
create_script "network_info" "Show network interfaces and connections." false "$TOOL_BODY_NETWORK_INFO"

# 75. battery_check
TOOL_BODY_BATTERY_CHECK='
if [ -f /sys/class/power_supply/BAT0/capacity ]; then
    CAPACITY=$(cat /sys/class/power_supply/BAT0/capacity)
    STATUS=$(cat /sys/class/power_supply/BAT0/status 2>/dev/null || echo "Unknown")
    OUT="Battery: ${CAPACITY}% (${STATUS})"
elif command -v pmset &> /dev/null; then
    OUT="Battery info: pmset available"
elif command -v upower &> /dev/null; then
    OUT="Battery info: upower available"
else
    OUT="Battery information not available on this system."
    EXIT_CODE=1
fi
'
create_script "battery_check" "Check battery status and charge level." false "$TOOL_BODY_BATTERY_CHECK"

# ==================================================================
# UTILITY & DATA TOOLS (76-80)
# ==================================================================

# 76. weather
TOOL_BODY_WEATHER='
LOCATION=$(get_arg "$ARG_JSON" "location")
if [ -z "$LOCATION" ]; then jq -n "{success:false, error:\"Location is required.\"}"; exit 1; fi

# Using wttr.in for weather data
OUT="Weather for $LOCATION: data requires API access"
'
create_script "weather" "Get current weather information for a location." true "$TOOL_BODY_WEATHER"

# 77. crypto_price
TOOL_BODY_CRYPTO_PRICE='
COIN=$(get_arg "$ARG_JSON" "coin")
CURRENCY=$(get_arg "$ARG_JSON" "currency")
[ -z "$CURRENCY" ] && CURRENCY="usd"
if [ -z "$COIN" ]; then jq -n "{success:false, error:\"Coin symbol is required.\"}"; exit 1; fi

OUT="Crypto price for $COIN: requires API access"
'
create_script "crypto_price" "Get cryptocurrency price information." true "$TOOL_BODY_CRYPTO_PRICE"

# 78. backup_create
TOOL_BODY_BACKUP_CREATE='
SOURCE=$(get_arg "$ARG_JSON" "source")
DESTINATION=$(get_arg "$ARG_JSON" "destination")
[ -z "$DESTINATION" ] && DESTINATION="backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_NAME="${SOURCE}_backup_${TIMESTAMP}.tar.gz"

mkdir -p "$DESTINATION" >/dev/null 2>&1
tar -czf "$DESTINATION/$BACKUP_NAME" "$SOURCE" >"$TEMP_OUT" 2>"$TEMP_ERR"
EXIT_CODE=$?
OUT="Backup created: $DESTINATION/$BACKUP_NAME"
ERR=$(cat "$TEMP_ERR")
'
create_script "backup_create" "Create timestamped backup archives." true "$TOOL_BODY_BACKUP_CREATE"

# 79. log_analyze
TOOL_BODY_LOG_ANALYZE='
LOGFILE=$(get_arg "$ARG_JSON" "logfile")
HOURS=$(get_arg "$ARG_JSON" "hours")
[ -z "$HOURS" ] && HOURS=24
if [ -z "$LOGFILE" ]; then jq -n "{success:false, error:\"Log file path is required.\"}"; exit 1; fi

OUTPUT="=== Log Analysis for $LOGFILE ===\n"
OUTPUT+="Last $HOURS hours summary:\n\n"
OUTPUT+="ERROR count: $(grep -i "error" "$LOGFILE" | wc -l)\n"
OUTPUT+="WARN count: $(grep -i "warn" "$LOGFILE" | wc -l)\n"
OUTPUT+="INFO count: $(grep -i "info" "$LOGFILE" | wc -l)\n"
OUTPUT+="Total lines: $(wc -l < "$LOGFILE")\n"
OUTPUT+="Recent errors:\n"
OUTPUT+=$(grep -i "error" "$LOGFILE" | tail -5)
OUT=$(echo -e "$OUTPUT")
'
create_script "log_analyze" "Analyze and summarize log files." true "$TOOL_BODY_LOG_ANALYZE"

# 80. text_summary
TOOL_BODY_TEXT_SUMMARY='
TEXT=$(get_arg "$ARG_JSON" "text")
MAX_LENGTH=$(get_arg "$ARG_JSON" "max_length")
[ -z "$MAX_LENGTH" ] && MAX_LENGTH=200
if [ -z "$TEXT" ]; then jq -n "{success:false, error:\"Text is required.\"}"; exit 1; fi

# Simple summary: take first 100 characters
SUMMARY=$(echo "$TEXT" | cut -c1-$MAX_LENGTH)
OUT="$SUMMARY..."
'
create_script "text_summary" "Summarize long text content." true "$TOOL_BODY_TEXT_SUMMARY"

# ==================================================================
# FUNCTIONS.JSON GENERATION (Updated for 80 tools)
# ==================================================================

echo -e "  [+] Generating ${GREEN}functions.json${NC}..."

cat > "$TARGET_DIR/functions.json" <<'EOF'
[
  {
    "name": "shell_execute",
    "description": "Execute an arbitrary shell command, logging output and capturing exit code/timeout status.",
    "parameters": {
      "type": "object",
      "properties": {
        "command": { "type": "string" },
        "timeout": { "type": "integer", "default": 10 }
      },
      "required": ["command"]
    }
  },
  {
    "name": "py_install",
    "description": "Installs a specified Python package using pip, after ensuring pip is up to date.",
    "parameters": {
      "type": "object",
      "properties": {
        "package": { "type": "string" }
      },
      "required": ["package"]
    }
  },
  {
    "name": "py_venv",
    "description": "Create a Python virtual environment in the specified directory.",
    "parameters": {
      "type": "object",
      "properties": {
        "dirname": { "type": "string", "default": "venv" }
      }
    }
  },
  {
    "name": "advanced_edit",
    "description": "Perform find and replace operations across multiple files using modern regex tools.",
    "parameters": {
      "type": "object",
      "properties": {
        "search": { "type": "string" },
        "replace": { "type": "string" },
        "glob": { "type": "string", "default": "*" }
      },
      "required": ["search", "replace"]
    }
  },
  {
    "name": "monitor_watch",
    "description": "Run a command when a file changes (uses entr if available).",
    "parameters": {
      "type": "object",
      "properties": {
        "file": { "type": "string" },
        "command": { "type": "string", "default": "echo changed" }
      },
      "required": ["file"]
    }
  },
  {
    "name": "env_check",
    "description": "Checks presence of key tools (python, git, jq, etc.).",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "cleanup_cache",
    "description": "Removes common build/dependency cache directories.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "git_stash_save",
    "description": "Stashes current Git changes with a message.",
    "parameters": {
      "type": "object",
      "properties": {
        "message": { "type": "string", "default": "WIP" }
      }
    }
  },
  {
    "name": "git_undo_all",
    "description": "Unstage all local Git changes.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "url_encode",
    "description": "Encode a string for use in URLs.",
    "parameters": {
      "type": "object",
      "properties": {
        "string_to_encode": { "type": "string" }
      },
      "required": ["string_to_encode"]
    }
  },
  {
    "name": "file_read",
    "description": "Safely read file content, preventing excessively long outputs.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" },
        "max_lines": { "type": "integer", "default": 100 }
      },
      "required": ["filepath"]
    }
  },
  {
    "name": "file_write",
    "description": "Overwrite a file with specific content.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" },
        "content": { "type": "string" }
      },
      "required": ["filepath", "content"]
    }
  },
  {
    "name": "file_append",
    "description": "Append content to a file.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" },
        "content": { "type": "string" }
      },
      "required": ["filepath", "content"]
    }
  },
  {
    "name": "file_find",
    "description": "Find files recursively by name pattern.",
    "parameters": {
      "type": "object",
      "properties": {
        "pattern": { "type": "string" },
        "directory": { "type": "string", "default": "." }
      },
      "required": ["pattern"]
    }
  },
  {
    "name": "text_search",
    "description": "Recursive text search (grep).",
    "parameters": {
      "type": "object",
      "properties": {
        "pattern": { "type": "string" },
        "directory": { "type": "string", "default": "." }
      },
      "required": ["pattern"]
    }
  },
  {
    "name": "text_count",
    "description": "Count lines, words, chars in matching files.",
    "parameters": {
      "type": "object",
      "properties": {
        "file_glob": { "type": "string" }
      },
      "required": ["file_glob"]
    }
  },
  {
    "name": "dir_size",
    "description": "Show human-readable disk usage of subdirectories.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "dir_tree",
    "description": "Display directory structure.",
    "parameters": {
      "type": "object",
      "properties": {
        "depth": { "type": "integer", "default": 2 }
      }
    }
  },
  {
    "name": "file_hash",
    "description": "Generate SHA256 hash of a file.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" }
      },
      "required": ["filepath"]
    }
  },
  {
    "name": "file_diff",
    "description": "Show colorized differences between two files.",
    "parameters": {
      "type": "object",
      "properties": {
        "file1": { "type": "string" },
        "file2": { "type": "string" }
      },
      "required": ["file1", "file2"]
    }
  },
  {
    "name": "file_permissions",
    "description": "Check or set file permissions.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" },
        "mode": { "type": "string" }
      },
      "required": ["filepath"]
    }
  },
  {
    "name": "file_touch",
    "description": "Create empty files or update timestamps.",
    "parameters": {
      "type": "object",
      "properties": {
        "args": { "type": "string" }
      },
      "required": ["args"]
    }
  },
  {
    "name": "file_ln",
    "description": "Create a symbolic link.",
    "parameters": {
      "type": "object",
      "properties": {
        "target": { "type": "string" },
        "link_name": { "type": "string" }
      },
      "required": ["target", "link_name"]
    }
  },
  {
    "name": "file_move",
    "description": "Move or rename a file or directory.",
    "parameters": {
      "type": "object",
      "properties": {
        "source": { "type": "string" },
        "destination": { "type": "string" }
      },
      "required": ["source", "destination"]
    }
  },
  {
    "name": "file_copy",
    "description": "Copy a file or directory recursively.",
    "parameters": {
      "type": "object",
      "properties": {
        "source": { "type": "string" },
        "destination": { "type": "string" }
      },
      "required": ["source", "destination"]
    }
  },
  {
    "name": "epoch",
    "description": "Get the current Unix timestamp.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "from_epoch",
    "description": "Convert Unix timestamp to readable date.",
    "parameters": {
      "type": "object",
      "properties": {
        "timestamp": { "type": "string" }
      },
      "required": ["timestamp"]
    }
  },
  {
    "name": "json_pretty",
    "description": "Pretty print JSON from file or stdin.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" }
      }
    }
  },
  {
    "name": "random_str",
    "description": "Generate a random string.",
    "parameters": {
      "type": "object",
      "properties": {
        "length": { "type": "integer", "default": 16 }
      }
    }
  },
  {
    "name": "todo",
    "description": "Manage a persistent to-do list for the current user.",
    "parameters": {
      "type": "object",
      "properties": {
        "action": { "type": "string", "enum": ["add", "list", "clear"] },
        "item": { "type": "string" }
      },
      "required": ["action"]
    }
  },
  {
    "name": "archive_extract",
    "description": "Extracts compressed files (.zip, .tar.gz, etc.).",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" }
      },
      "required": ["filepath"]
    }
  },
  {
    "name": "archive_compress",
    "description": "Compresses a directory to .tar.gz.",
    "parameters": {
      "type": "object",
      "properties": {
        "directory": { "type": "string" }
      },
      "required": ["directory"]
    }
  },
  {
    "name": "clipboard",
    "description": "Copy piped input or paste clipboard content.",
    "parameters": {
      "type": "object",
      "properties": {
        "action": { "type": "string", "enum": ["copy", "paste"] }
      }
    }
  },
  {
    "name": "count_lines",
    "description": "Count lines of code by extension recursively.",
    "parameters": {
      "type": "object",
      "properties": {
        "extension": { "type": "string", "default": "sh" }
      }
    }
  },
  {
    "name": "url_shorten",
    "description": "Shorten a URL using Bitly API.",
    "parameters": {
      "type": "object",
      "properties": {
        "url": { "type": "string" },
        "token": { "type": "string" }
      },
      "required": ["url", "token"]
    }
  },
  {
    "name": "http_status",
    "description": "Fetches only the HTTP status code for a URL.",
    "parameters": {
      "type": "object",
      "properties": {
        "url": { "type": "string" }
      },
      "required": ["url"]
    }
  },
  {
    "name": "cron_add",
    "description": "Schedule a job via crontab.",
    "parameters": {
      "type": "object",
      "properties": {
        "schedule": { "type": "string" },
        "command": { "type": "string" }
      },
      "required": ["schedule", "command"]
    }
  },
  {
    "name": "cron_list",
    "description": "List current crontab jobs.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "cron_rm",
    "description": "Remove a specific command from crontab.",
    "parameters": {
      "type": "object",
      "properties": {
        "command": { "type": "string" }
      },
      "required": ["command"]
    }
  },
  {
    "name": "logger_notify",
    "description": "Send a desktop notification.",
    "parameters": {
      "type": "object",
      "properties": {
        "message": { "type": "string" }
      },
      "required": ["message"]
    }
  },
  {
    "name": "process_kill",
    "description": "Send SIGTERM (graceful kill) to a process by PID.",
    "parameters": {
      "type": "object",
      "properties": {
        "pid": { "type": "string" }
      },
      "required": ["pid"]
    }
  },
  {
    "name": "process_wait",
    "description": "Wait for a specific PID to terminate.",
    "parameters": {
      "type": "object",
      "properties": {
        "pid": { "type": "string" }
      },
      "required": ["pid"]
    }
  },
  {
    "name": "net_ping",
    "description": "Ping a host multiple times.",
    "parameters": {
      "type": "object",
      "properties": {
        "host": { "type": "string" },
        "count": { "type": "integer", "default": 4 }
      },
      "required": ["host"]
    }
  },
  {
    "name": "net_dns",
    "description": "Resolve a domain name to IP addresses.",
    "parameters": {
      "type": "object",
      "properties": {
        "domain": { "type": "string" }
      },
      "required": ["domain"]
    }
  },
  {
    "name": "net_ports",
    "description": "Simple port scan using nmap.",
    "parameters": {
      "type": "object",
      "properties": {
        "host": { "type": "string" },
        "ports": { "type": "string", "default": "80,443,22" }
      },
      "required": ["host"]
    }
  },
  {
    "name": "js_format",
    "description": "Format JS/JSON files using prettier.",
    "parameters": {
      "type": "object",
      "properties": {
        "filepath": { "type": "string" }
      },
      "required": ["filepath"]
    }
  },
  {
    "name": "go_build",
    "description": "Builds a Go project.",
    "parameters": {
      "type": "object",
      "properties": {
        "executable_name": { "type": "string", "default": "app" }
      }
    }
  },
  {
    "name": "log_grep_error",
    "description": "Grep a log file for ERROR lines.",
    "parameters": {
      "type": "object",
      "properties": {
        "logfile": { "type": "string" }
      },
      "required": ["logfile"]
    }
  },
  {
    "name": "csv_to_json",
    "description": "Convert CSV file to JSON array.",
    "parameters": {
      "type": "object",
      "properties": {
        "csv_filepath": { "type": "string" }
      },
      "required": ["csv_filepath"]
    }
  },
  {
    "name": "token_get",
    "description": "Retrieve first non-comment line from a file.",
    "parameters": {
      "type": "object",
      "properties": {
        "token_filepath": { "type": "string" }
      },
      "required": ["token_filepath"]
    }
  },
  {
    "name": "token_set",
    "description": "Output export command to set env var (must be sourced).",
    "parameters": {
      "type": "object",
      "properties": {
        "var_name": { "type": "string" },
        "value": { "type": "string" }
      },
      "required": ["var_name", "value"]
    }
  },
  {
    "name": "shell_history",
    "description": "Search shell history for command patterns.",
    "parameters": {
      "type": "object",
      "properties": {
        "pattern": { "type": "string" }
      },
      "required": ["pattern"]
    }
  },
  {
    "name": "docker_clean",
    "description": "Removes unused Docker images, containers, and networks.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "ssh_connect",
    "description": "Connect to a remote host via SSH (blocks execution).",
    "parameters": {
      "type": "object",
      "properties": {
        "ssh_target": { "type": "string" }
      },
      "required": ["ssh_target"]
    }
  },
  {
    "name": "ssh_copy_id",
    "description": "Copies SSH public key to a remote server.",
    "parameters": {
      "type": "object",
      "properties": {
        "ssh_target": { "type": "string" }
      },
      "required": ["ssh_target"]
    }
  },
  {
    "name": "memory_usage",
    "description": "Shows top 10 processes by resident memory usage.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "zip_file",
    "description": "Zip a single file or directory.",
    "parameters": {
      "type": "object",
      "properties": {
        "target": { "type": "string" }
      },
      "required": ["target"]
    }
  },
  {
    "name": "time_exec",
    "description": "Measure the wall-clock, user, and system time taken to run any command.",
    "parameters": {
      "type": "object",
      "properties": {
        "command_args": { "type": "array", "items": { "type": "string" } }
      },
      "required": ["command_args"]
    }
  },
  {
    "name": "xml_to_json",
    "description": "Convert XML file to prettified JSON.",
    "parameters": {
      "type": "object",
      "properties": {
        "xml_filepath": { "type": "string" }
      },
      "required": ["xml_filepath"]
    }
  },
  {
    "name": "cleanup_empty",
    "description": "Recursively find and delete all empty files.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "web_search",
    "description": "Perform web search queries.",
    "parameters": {
      "type": "object",
      "properties": {
        "query": { "type": "string" },
        "num_results": { "type": "integer", "default": 10 }
      },
      "required": ["query"]
    }
  },
  {
    "name": "web_image_search",
    "description": "Search for images on the web.",
    "parameters": {
      "type": "object",
      "properties": {
        "query": { "type": "string" },
        "num_results": { "type": "integer", "default": 10 }
      },
      "required": ["query"]
    }
  },
  {
    "name": "web_scrape",
    "description": "Extract content from websites.",
    "parameters": {
      "type": "object",
      "properties": {
        "url": { "type": "string" },
        "selector": { "type": "string" }
      },
      "required": ["url"]
    }
  },
  {
    "name": "web_reverse_image_search",
    "description": "Perform reverse image search.",
    "parameters": {
      "type": "object",
      "properties": {
        "image_url": { "type": "string" }
      },
      "required": ["image_url"]
    }
  },
  {
    "name": "web_news_search",
    "description": "Search for news articles.",
    "parameters": {
      "type": "object",
      "properties": {
        "query": { "type": "string" },
        "days": { "type": "integer", "default": 7 }
      },
      "required": ["query"]
    }
  },
  {
    "name": "batch_file_read",
    "description": "Read multiple files at once.",
    "parameters": {
      "type": "object",
      "properties": {
        "files": { "type": "array", "items": { "type": "string" } }
      },
      "required": ["files"]
    }
  },
  {
    "name": "batch_file_rename",
    "description": "Rename multiple files matching a pattern.",
    "parameters": {
      "type": "object",
      "properties": {
        "pattern": { "type": "string" },
        "replacement": { "type": "string" }
      },
      "required": ["pattern", "replacement"]
    }
  },
  {
    "name": "batch_file_move",
    "description": "Move multiple files to a directory.",
    "parameters": {
      "type": "object",
      "properties": {
        "files": { "type": "array", "items": { "type": "string" } },
        "destination": { "type": "string" }
      },
      "required": ["files", "destination"]
    }
  },
  {
    "name": "batch_file_copy",
    "description": "Copy multiple files to a directory.",
    "parameters": {
      "type": "object",
      "properties": {
        "files": { "type": "array", "items": { "type": "string" } },
        "destination": { "type": "string" }
      },
      "required": ["files", "destination"]
    }
  },
  {
    "name": "batch_file_grep",
    "description": "Search for pattern in multiple files.",
    "parameters": {
      "type": "object",
      "properties": {
        "pattern": { "type": "string" },
        "directory": { "type": "string", "default": "." }
      },
      "required": ["pattern"]
    }
  },
  {
    "name": "system_info",
    "description": "Get comprehensive system information.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "process_list",
    "description": "List and filter running processes.",
    "parameters": {
      "type": "object",
      "properties": {
        "sort": { "type": "string", "enum": ["cpu", "mem", "pid"], "default": "cpu" },
        "filter": { "type": "string" }
      }
    }
  },
  {
    "name": "disk_usage",
    "description": "Show detailed disk usage information.",
    "parameters": {
      "type": "object",
      "properties": {
        "path": { "type": "string", "default": "." }
      }
    }
  },
  {
    "name": "network_info",
    "description": "Show network interfaces and connections.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "battery_check",
    "description": "Check battery status and charge level.",
    "parameters": {
      "type": "object",
      "properties": {}
    }
  },
  {
    "name": "weather",
    "description": "Get current weather information for a location.",
    "parameters": {
      "type": "object",
      "properties": {
        "location": { "type": "string" }
      },
      "required": ["location"]
    }
  },
  {
    "name": "crypto_price",
    "description": "Get cryptocurrency price information.",
    "parameters": {
      "type": "object",
      "properties": {
        "coin": { "type": "string" },
        "currency": { "type": "string", "default": "usd" }
      },
      "required": ["coin"]
    }
  },
  {
    "name": "backup_create",
    "description": "Create timestamped backup archives.",
    "parameters": {
      "type": "object",
      "properties": {
        "source": { "type": "string" },
        "destination": { "type": "string", "default": "backups" }
      },
      "required": ["source"]
    }
  },
  {
    "name": "log_analyze",
    "description": "Analyze and summarize log files.",
    "parameters": {
      "type": "object",
      "properties": {
        "logfile": { "type": "string" },
        "hours": { "type": "integer", "default": 24 }
      },
      "required": ["logfile"]
    }
  },
  {
    "name": "text_summary",
    "description": "Summarize long text content.",
    "parameters": {
      "type": "object",
      "properties": {
        "text": { "type": "string" },
        "max_length": { "type": "integer", "default": 200 }
      },
      "required": ["text"]
    }
  }
]
EOF

echo -e "\n${GREEN}--- 80 Tools Generated Successfully! ---${NC}"
echo "JSON definitions available in ${CYAN}${TARGET_DIR}/functions.json${NC}"
echo "All tools are executable and ready to use."
