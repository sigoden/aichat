#!/bin/bash

# Script to automate the setup of aichat on Pop!_OS

# Exit immediately if a command exits with a non-zero status.
set -e

# --- Helper Functions ---
info() {
    echo "[INFO] $1"
}

error() {
    echo "[ERROR] $1" >&2
    exit 1
}

# --- Install Dependencies ---
install_dependencies() {
    info "Updating package list..."
    sudo apt-get update

    info "Installing essential packages (git, build-essential, curl, libssl-dev, pkg-config)..."
    sudo apt-get install -y git build-essential curl libssl-dev pkg-config

    # Install Rust using rustup
    if ! command -v cargo &> /dev/null; then
        info "Rust not found. Installing Rust via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # Source cargo environment for the current script session
        source "$HOME/.cargo/env"
    else
        info "Rust (cargo) is already installed."
    fi
    info "Rust version: $(rustc --version)"
    info "Cargo version: $(cargo --version)"
}

# --- Install and Configure Ollama ---
install_ollama() {
    info "Installing Ollama..."
    if command -v ollama &> /dev/null; then
        info "Ollama is already installed. Checking for updates..."
        # Attempt to update if already installed, or instruct user
        # For now, let's assume if it's installed, it's managed by user or another script
        # A more robust check would be to compare versions or re-run their install script
        # which is idempotent.
        curl -fsSL https://ollama.com/install.sh | sh
    else
        curl -fsSL https://ollama.com/install.sh | sh
    fi
    
    info "Ollama version: $(ollama --version)"

    info "Starting Ollama service (if not already running)..."
    # The install script usually starts it, but good to ensure.
    # Systemd is typical on Pop!_OS
    if systemctl is-active --quiet ollama; then
        info "Ollama service is already active."
    else
        info "Attempting to start Ollama service..."
        sudo systemctl enable ollama
        sudo systemctl start ollama
        # Wait a bit for the service to start
        sleep 5
        if ! systemctl is-active --quiet ollama; then
            error "Ollama service failed to start. Please check 'sudo systemctl status ollama' or 'journalctl -u ollama'."
        fi
    fi

    # Pull a default model
    # Using a smaller model like orca-mini for faster setup.
    # Users can change this later.
    local default_ollama_model="orca-mini"
    info "Pulling default Ollama model ('$default_ollama_model'). This may take some time..."
    ollama pull "$default_ollama_model"
    info "Ollama model '$default_ollama_model' pulled."
    ollama list
}

# --- Build and Install aichat ---
build_aichat() {
    info "Building aichat from current directory (forked repository)..."
    # No git clone needed as we are in the forked repository.
    # Assuming Cargo.toml is at the root of this repository.
    
    info "Building aichat (release mode). This may take some time..."
    # Run cargo build from the current directory (repository root)
    cargo build --release 

    info "Installing aichat binary to $HOME/.local/bin..."
    mkdir -p "$HOME/.local/bin"
    # Path to target will be ./target/release/aichat if building from root
    cp target/release/aichat "$HOME/.local/bin/"

    # Ensure $HOME/.local/bin is in PATH (existing logic is fine)
    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        info "Adding $HOME/.local/bin to PATH for the current session and .bashrc/.zshrc."
        export PATH="$HOME/.local/bin:$PATH"
        
        if [ -f "$HOME/.bashrc" ]; then
            echo '' >> "$HOME/.bashrc"
            echo '# Add .local/bin to PATH for aichat' >> "$HOME/.bashrc"
            echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
        fi
        if [ -f "$HOME/.zshrc" ]; then
            echo '' >> "$HOME/.zshrc"
            echo '# Add .local/bin to PATH for aichat' >> "$HOME/.zshrc"
            echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.zshrc"
        fi
        info "Please source your .bashrc/.zshrc or open a new terminal to apply PATH changes permanently."
    fi
}

# --- Configure aichat ---
configure_aichat() {
    info "Configuring aichat with detected Ollama models..."
    local config_dir="$HOME/.config/aichat"
    local config_file="$config_dir/config.yaml"
    local default_ollama_model_short_name="orca-mini" # Used as the key in aichat config
    local default_ollama_model_for_aichat="ollama:$default_ollama_model_short_name"

    mkdir -p "$config_dir"

    # Ensure config_file exists, create a basic one if not
    if [ ! -f "$config_file" ]; then
        info "Creating default aichat config at $config_file..."
        echo "model: $default_ollama_model_for_aichat" > "$config_file"
        echo "clients:" >> "$config_file"
        echo "  - type: ollama" >> "$config_file"
        echo "    name: ollama" >> "$config_file" # Give the client a name for clarity
        echo "    models: {} # Will be populated by script" >> "$config_file"
    else
        info "Existing aichat config found at $config_file."
        # Set default model if no model line exists
        if ! grep -q "^model:" "$config_file"; then
            info "No default model set in $config_file. Setting to '$default_ollama_model_for_aichat'."
            # Prepend to the file
            sed -i "1s;^;model: $default_ollama_model_for_aichat\n;" "$config_file"
        fi
        # Ensure clients section exists
        if ! grep -q "^clients:" "$config_file"; then
            echo "clients:" >> "$config_file"
        fi
        # Ensure ollama client entry exists
        if ! grep -Pzq "clients:\n(.*\n)*? +- type: ollama" "$config_file"; then
            info "Ollama client not found in config. Adding it."
            # This append might not be perfectly indented if other clients exist.
            # A proper YAML parser would be better here.
            echo "  - type: ollama" >> "$config_file"
            echo "    name: ollama" >> "$config_file"
            echo "    models: {} # Will be populated by script" >> "$config_file"
        else
            # Ensure 'models:' subsection exists under ollama client
            # This is tricky with sed/grep. We'll assume if 'type: ollama' exists, 'models:' should be there or added.
            # The logic below will try to add it if it's missing from the ollama client block
            # This is a simplification; robust YAML editing in bash is very hard.
            # We find the line number of '- type: ollama'
            ollama_client_line_num=$(grep -n -- "- type: ollama" "$config_file" | cut -d: -f1 | head -n 1)
            if [ -n "$ollama_client_line_num" ]; then
                # Check if 'models:' exists between this line and the next '- type:' or end of file
                # Get lines from ollama_client_line_num to end of file
                config_tail=$(tail -n +"$ollama_client_line_num" "$config_file")
                # See if 'models:' is in the first part of this tail, before another potential client
                next_client_in_tail_line_num=$(echo "$config_tail" | tail -n +2 | grep -n -- "- type:" | cut -d: -f1 | head -n 1)

                ollama_client_block=""
                if [ -n "$next_client_in_tail_line_num" ]; then
                    ollama_client_block=$(echo "$config_tail" | head -n "$((next_client_in_tail_line_num -1))" )
                else
                    ollama_client_block="$config_tail"
                fi

                if ! echo "$ollama_client_block" | grep -q "models:"; then
                    info "Ollama client found, but 'models:' section is missing. Adding it."
                    # Insert 'models: {}' after 'name: ollama' or 'type: ollama'
                    # This uses awk for more precise insertion.
                    awk -v line_num="$ollama_client_line_num" '
                    NR==line_num {
                        print $0;
                        # Try to find if name: ollama exists to insert after it for better formatting
                        # This is still a heuristic
                        if ($0 ~ /name: ollama/) {
                             # Already printed $0
                        } else if (getline > 0 && $0 ~ /name: ollama/) {
                            print $0
                        }
                        print "    models: {} # Will be populated by script";
                        next;
                    }
                    {print $0}
                    ' "$config_file" > "$config_file.tmp" && mv "$config_file.tmp" "$config_file"
                fi
            fi
        fi
    fi

    info "Fetching list of installed Ollama models..."
    # Skip header line, get first field (NAME), remove potential ':latest' for aichat key
    ollama_models_output=$(ollama list | tail -n +2)
    if [ -z "$ollama_models_output" ]; then
        info "No Ollama models found installed locally via 'ollama list'."
    else
        echo "$ollama_models_output" | while IFS= read -r line; do
            full_model_name=$(echo "$line" | awk '{print $1}') # e.g., llama2:latest
            # aichat model key is typically the part before the colon
            aichat_model_key=$(echo "$full_model_name" | cut -d: -f1)

            info "Processing Ollama model: $full_model_name (aichat key: $aichat_model_key)"

            # Check if model key already exists in the ollama client's models section
            # This grep is imperfect for complex YAML but aims to find '  <key>:' under 'models:' in the ollama client block
            # It assumes 'models:' is followed by indented model keys.
            # A more robust check would involve a YAML parser.
            
            # Find the line number of '- type: ollama'
            ollama_client_line_start=$(grep -n -- "- type: ollama" "$config_file" | head -n 1 | cut -d: -f1)
            if [ -z "$ollama_client_line_start" ]; then
                error "Could not find Ollama client block ('- type: ollama') in $config_file even after attempting to add it."
                continue # Should not happen given earlier checks
            fi

            # Determine end of ollama client block
            # Look for next '- type:' or end of file starting from after ollama_client_line_start
            num_lines_in_file=$(wc -l < "$config_file")
            next_client_line_after_ollama=$(tail -n +"$((ollama_client_line_start + 1))" "$config_file" | grep -n -- "- type:" | head -n 1 | cut -d: -f1)
            
            ollama_client_line_end=$num_lines_in_file
            if [ -n "$next_client_line_after_ollama" ]; then
                ollama_client_line_end=$((ollama_client_line_start + next_client_line_after_ollama -1))
            fi
            
            # Extract the specific ollama client block
            ollama_block_content=$(sed -n "${ollama_client_line_start},${ollama_client_line_end}p" "$config_file")

            # Check if the model key exists within this block under a 'models:' section
            model_exists_in_block=false
            if echo "$ollama_block_content" | grep -q "models:"; then
                 # Check if the key exists as '  <key>:'
                 if echo "$ollama_block_content" | grep -Eq "^ *${aichat_model_key}:"; then
                     model_exists_in_block=true
                 fi
            fi

            if $model_exists_in_block; then
                info "Model '$aichat_model_key' (for $full_model_name) already configured in $config_file."
            else
                info "Adding model '$aichat_model_key' (for $full_model_name) to $config_file."
                # Find the 'models:' line within the ollama client block and append to it.
                # This is also tricky. A simpler approach for bash is to find the line with 'models:'
                # that is part of the ollama client, and append the model definition under it.
                # This assumes 'models:' line exists and is unique enough or correctly identified.
                
                # Find the line number of 'models:' within the ollama_block_content relative to start of block
                models_line_in_block_rel_num=$(echo "$ollama_block_content" | grep -n "models:" | head -n 1 | cut -d: -f1)

                if [ -n "$models_line_in_block_rel_num" ]; then
                    actual_models_line_num=$((ollama_client_line_start + models_line_in_block_rel_num -1))
                    
                    # Check if models: {} (empty map placeholder)
                    if grep -q "models: {}" "$config_file"; then
                        # Replace models: {} with the first model
                        sed -i "${actual_models_line_num}s;models: {};models:\n    ${aichat_model_key}:\n      name: ${full_model_name};" "$config_file"
                    else
                        # Append new model after the models line, using awk for better line targeting
                        awk -v line_num="$actual_models_line_num" -v key="$aichat_model_key" -v fname="$full_model_name" '
                        NR==line_num {
                            print $0;
                            print "    " key ":";
                            print "      name: " fname;
                            next;
                        }
                        {print $0}
                        ' "$config_file" > "$config_file.tmp" && mv "$config_file.tmp" "$config_file"
                    fi
                    info "Model '$aichat_model_key' added."
                else
                    # This case should ideally be handled by the earlier check that adds 'models: {}'
                    # if it's missing from an ollama client block.
                    error "Could not find 'models:' line under the Ollama client in $config_file to add $aichat_model_key. Manual configuration may be needed."
                fi
            fi
        done
    fi
    
    info "aichat configuration for Ollama models updated."
    info "You can list available Ollama models using 'ollama list'."
    info "Run aichat using 'aichat' or 'aichat --model ollama:your-ollama-model-name'."
}

# --- Setup Bash Completions ---
setup_bash_completions() {
    info "Setting up bash completions for aichat..."

    # Install bash-completion package
    if ! dpkg -s bash-completion &> /dev/null; then
        info "bash-completion package not found. Installing..."
        sudo apt-get install -y bash-completion
    else
        info "bash-completion package is already installed."
    fi

    # Create user completions directory if it doesn't exist
    local user_completions_dir="$HOME/.local/share/bash-completion/completions"
    info "Ensuring user completions directory exists: $user_completions_dir"
    mkdir -p "$user_completions_dir"

    # Copy aichat completion script
    # Assuming setup_popos.sh is run from the root of the aichat repository
    local aichat_completion_script_source="scripts/completions/aichat.bash"
    if [ -f "$aichat_completion_script_source" ]; then
        info "Copying aichat bash completion script to $user_completions_dir/aichat"
        cp "$aichat_completion_script_source" "$user_completions_dir/aichat"
    else
        warn "aichat bash completion script not found at $aichat_completion_script_source. Skipping copy."
    fi

    # Ensure .bashrc sources bash_completion
    local bash_completion_source_line_one="# Source global bash completion (if available)"
    local bash_completion_source_line_two_global="/usr/share/bash-completion/bash_completion" # Common global path
    local bash_completion_source_line_two_etc="/etc/bash_completion" # Another common global path
    
    # Check if .bashrc exists
    if [ -f "$HOME/.bashrc" ]; then
        # Check if either global completion script is already sourced
        if ! grep -qFxP "source $bash_completion_source_line_two_global" "$HOME/.bashrc" && \
           ! grep -qFxP ". $bash_completion_source_line_two_global" "$HOME/.bashrc" && \
           ! grep -qFxP "source $bash_completion_source_line_two_etc" "$HOME/.bashrc" && \
           ! grep -qFxP ". $bash_completion_source_line_two_etc" "$HOME/.bashrc"; then
            info "Adding bash_completion source to $HOME/.bashrc"
            echo "" >> "$HOME/.bashrc"
            echo "$bash_completion_source_line_one" >> "$HOME/.bashrc"
            # Add a conditional sourcing block
            echo "if [ -f $bash_completion_source_line_two_global ]; then" >> "$HOME/.bashrc"
            echo "    . $bash_completion_source_line_two_global" >> "$HOME/.bashrc"
            echo "elif [ -f $bash_completion_source_line_two_etc ]; then" >> "$HOME/.bashrc"
            echo "    . $bash_completion_source_line_two_etc" >> "$HOME/.bashrc"
            echo "fi" >> "$HOME/.bashrc"
        else
            info "bash_completion seems to be already sourced in $HOME/.bashrc."
        fi
    else
        warn "$HOME/.bashrc not found. Cannot configure bash_completion sourcing."
    fi
    info "Bash completion setup for aichat finished. Please source your .bashrc or open a new terminal."
}

# --- Main Execution ---
main() {
    info "Starting aichat setup for Pop!_OS..."

    install_dependencies
    install_ollama
    build_aichat # This was modified in the previous step
    configure_aichat
    setup_bash_completions # Add the call here

    info "aichat setup complete!"
    info "Ensure $HOME/.local/bin is in your PATH. You might need to open a new terminal or source your shell configuration file (e.g., source ~/.bashrc)."
    info "Try running 'aichat' or 'aichat --help'."
    info "To chat with the default Ollama model, try: aichat \"Hello, who are you?\""
}

# Run the main function
main
