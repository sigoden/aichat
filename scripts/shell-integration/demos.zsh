# Demo commands for aichat (zsh)
# Source this file in your .zshrc:
#   source /path/to/demos.zsh

# cmd — natural language to shell command (wraps aichat -e)
cmd() {
    aichat -e "$*"
}

# oneliner — complex pipe chains from plain English
oneliner() {
    aichat -r oneliner "$*"
}

# explain — explain a command, error, or code snippet
# Usage: explain "some command"
#        some-command 2>&1 | explain
explain() {
    if [[ -n "$1" ]]; then
        aichat -r explain "$*"
    elif [[ ! -t 0 ]]; then
        aichat -r explain
    else
        echo 'Usage: explain "command or error"'
        echo '       some-command | explain'
        return 1
    fi
}

# naming — suggest names for code elements
naming() {
    if [[ -z "$1" ]]; then
        echo 'Usage: naming "describe what it does"'
        return 1
    fi
    aichat -r naming "$*"
}

# wtd — what's this directory?
# Usage: wtd [directory]
wtd() {
    local target="${1:-.}"
    if [[ ! -d "$target" ]]; then
        echo "Error: '$target' is not a directory" >&2
        return 1
    fi
    local snapshot=""
    snapshot+="Directory: $(cd "$target" && pwd)"$'\n'
    snapshot+="$(ls -la "$target" 2>/dev/null | head -30)"$'\n'
    for f in README.md README readme.md Package.swift package.json Cargo.toml go.mod pyproject.toml Makefile Dockerfile; do
        if [[ -f "$target/$f" ]]; then
            snapshot+=$'\n'"--- $f (first 5 lines) ---"$'\n'
            snapshot+="$(head -5 "$target/$f" 2>/dev/null)"$'\n'
        fi
    done
    if git -C "$target" rev-parse --git-dir &>/dev/null 2>&1; then
        snapshot+=$'\n'"--- git ---"$'\n'
        snapshot+="branch: $(git -C "$target" branch --show-current 2>/dev/null)"$'\n'
        snapshot+="last commit: $(git -C "$target" log --oneline -1 2>/dev/null)"$'\n'
    fi
    echo "$snapshot" | aichat -r wtd
}

# port — what's using this port?
port() {
    if [[ -z "$1" ]] || ! [[ "$1" =~ ^[0-9]+$ ]]; then
        echo 'Usage: port <port-number>'
        return 1
    fi
    local info
    info=$(lsof -i :"$1" -P -n 2>/dev/null)
    if [[ -z "$info" ]]; then
        echo "Nothing is using port $1."
        return 0
    fi
    echo "Port $1:"$'\n'"$info" | aichat -r port
}

# gitsum — summarize recent git activity
# Usage: gitsum [count]
gitsum() {
    if ! git rev-parse --git-dir &>/dev/null 2>&1; then
        echo "Error: not a git repository" >&2
        return 1
    fi
    local count="${1:-10}"
    local log branch authors
    log=$(git log --oneline -"$count" 2>/dev/null)
    branch=$(git branch --show-current 2>/dev/null)
    authors=$(git log --format='%an' -"$count" 2>/dev/null | sort | uniq -c | sort -rn | head -5)
    if [[ -z "$log" ]]; then
        echo "No commits found."
        return 0
    fi
    printf "Branch: %s\nRecent %s commits:\n%s\n\nAuthors:\n%s\n" \
        "$branch" "$count" "$log" "$authors" | aichat -r gitsum
}

# mac-narrator — your Mac's inner monologue
mac-narrator() {
    local snapshot
    snapshot=$(
        ps -eo pid,%cpu,%mem,comm -r 2>/dev/null | head -8
        echo "---"
        vm_stat 2>/dev/null | head -5
        echo "---"
        df -h / 2>/dev/null | tail -1
        echo "---"
        uptime 2>/dev/null
    )
    echo "$snapshot" | aichat -r mac-narrator
}
