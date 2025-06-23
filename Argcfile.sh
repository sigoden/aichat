#!/usr/bin/env bash
set -e

# @meta dotenv
# @env DRY_RUN Dry run mode

# @cmd Test configuration initialization
# @env AICHAT_CONFIG_DIR=tmp/test-init-config
# @arg args~
test-init-config() {
    unset OPENAI_API_KEY
    mkdir -p "$AICHAT_CONFIG_DIR"
    config_file="$AICHAT_CONFIG_DIR/config.yaml"
    if [[ -f "$config_file" ]]; then
        rm -f "$config_file"
    fi
    cargo run -- "$@"
}

# @cmd Test running without configuration file
# @env AICHAT_PROVIDER!
# @env AICHAT_CONFIG_DIR=tmp/test-provider-env
# @arg args~
test-no-config() {
    mkdir -p "$AICHAT_CONFIG_DIR"
    rm -rf "$AICHAT_CONFIG_DIR/config.yaml"
    cargo run -- "$@"
}

# @cmd Test function calling
# @option -m --model[?`_choice_model`]
# @option -p --preset[=weather|multi-weathers]
# @flag -S --no-stream
# @arg text~
test-function-calling() {
    args=(--role %functions%)
    if [[ -n "$argc_model"  ]]; then
      args+=("--model" "$argc_model")
    fi
    if [[ -n "$argc_no_stream" ]]; then
        args+=("-S")
    fi
    if [[ -z "$argc_text" ]]; then
        case "$argc_preset" in
        multi-weathers)
            text="what is the weather in London and Pairs?"
            ;;
        weather|*)
            text="what is the weather in London?"
            ;;
        esac
    else
        text="${argc_text[*]}"
    fi
    cargo run -- "${args[@]}" "$text"
}

# @cmd Test clients
# @arg clients+[`_choice_client`]
test-clients() {
    for c in "${argc_clients[@]}"; do
        echo "### $c stream"
        aichat -m "$c" 1 + 2 = ?
        echo "### $c non-stream"
        aichat -m "$c" -S 1 + 2 = ?
    done
}

# @cmd Test proxy server
# @option -m --model[?`_choice_model`]
# @flag -S --no-stream
# @arg text~
test-server() {
    args=()
    if [[ -n "$argc_no_stream" ]]; then
        args+=("-S")
    fi
    argc chat-llm "${args[@]}" \
    --api-base http://localhost:8000/v1 \
    --model "${argc_model:-default}" \
    "$@"
}

# @cmd Chat with any LLM api 
# @flag -S --no-stream
# @arg provider_model![?`_choice_provider_model`]
# @arg text~
chat() {
    if [[ "$argc_provider_model" == *':'* ]]; then
        model="${argc_provider_model##*:}"
        argc_provider="${argc_provider_model%:*}"
    else
        argc_provider="${argc_provider_model}"
    fi
    for provider_config in "${OPENAI_COMPATIBLE_PROVIDERS[@]}"; do
        if [[ "$argc_provider" == "${provider_config%%,*}" ]]; then
            _retrieve_api_base
            break
        fi
    done
    if [[ -n "$api_base" ]]; then
        env_prefix="$(echo "$argc_provider" | tr '[:lower:]' '[:upper:]')"
        api_key_env="${env_prefix}_API_KEY"
        api_key="${!api_key_env}" 
        if [[ -z "$model" ]]; then
            model="$(echo "$provider_config" | cut -d, -f2)"
        fi
        if [[ -z "$model" ]]; then
            model_env="${env_prefix}_MODEL"
            model="${!model_env}"
        fi
        argc chat-openai-compatible \
            --api-base "$api_base" \
            --api-key "$api_key" \
            --model "$model" \
            "${argc_text[@]}"
    else
        argc chat-$argc_provider "${argc_text[@]}"
    fi
}

# @cmd List models by openai-compatible api
# @flag --name-only Print model name only
# @arg provider![`_choice_provider`]
models() {
    for provider_config in "${OPENAI_COMPATIBLE_PROVIDERS[@]}"; do
        if [[ "$argc_provider" == "${provider_config%%,*}" ]]; then
            _retrieve_api_base
            break
        fi
    done
    if [[ -n "$api_base" ]]; then
        env_prefix="$(echo "$argc_provider" | tr '[:lower:]' '[:upper:]')"
        api_key_env="${env_prefix}_API_KEY"
        api_key="${!api_key_env}" 
        jq_args=()
        if [[ -n "$argc_name_only" ]]; then
            case "$argc_provider" in
                cloudflare)
                    jq_args+=(-r '.result[].name')
                    ;;
                github)
                    jq_args+=(-r '.[].name')
                    ;;
                *)
                    jq_args+=(-r '.data[].id')
                    ;;
            esac
        fi
        _openai_compatible_models | jq "${jq_args[@]}"
    else
        if ! cat "$0" | grep -q "^models-$argc_provider"; then
            _die "error: provider '$argc_provider' does not have a models api"
        fi
        cli_args=()
        if [[ -n "$argc_name_only" ]]; then
            cli_args+=(--name-only)
        fi
        argc models-$argc_provider "${cli_args[@]}"
    fi
}

# @cmd Chat with openai-compatible api
# @option --api-base! $$ 
# @option --api-key! $$
# @option -m --model! $$
# @flag -S --no-stream
# @arg text~
chat-openai-compatible() {
    _wrapper curl -i "$argc_api_base/chat/completions" \
-X POST \
-H "Content-Type: application/json" \
-H "Authorization: Bearer $argc_api_key" \
-d "$(_build_body openai "$@")"
}

# @cmd List models by openai-compatible api
# @option --api-base! $$
# @option --api-key! $$
# @flag --name-only Print model name only
models-openai-compatible() {
    jq_args=()
    if [[ -n "$argc_name_only" ]]; then
        jq_args+=(-r '.data[].id')
    fi
    _openai_compatible_models | jq "${jq_args[@]}"
}

# @cmd Chat with azure-openai api
# @option --api-url! $$ 
# @option --api-key! $$
# @option -m --model! $$
# @flag -S --no-stream
# @arg text~
chat-azure-openai() {
    _wrapper curl -i "$argc_api_url" \
-X POST \
-H "Content-Type: application/json" \
-H "api-key: $argc_api_key" \
-d "$(_build_body openai "$@")"
}

# @cmd Chat with gemini api
# @env GEMINI_API_KEY!
# @option -m --model=gemini-1.5-pro-latest $GEMINI_MODEL
# @flag -S --no-stream
# @arg text~
chat-gemini() {
    method="streamGenerateContent"
    if [[ -n "$argc_no_stream" ]]; then
        method="generateContent"
    fi
    _wrapper curl -i "https://generativelanguage.googleapis.com/v1beta/models/${argc_model}:${method}?key=${GEMINI_API_KEY}" \
-i -X POST \
-H 'Content-Type: application/json' \
-d "$(_build_body gemini "$@")" 
}

# @cmd List gemini models
# @env GEMINI_API_KEY!
# @flag --name-only Print model name only
models-gemini() {
    jq_args=()
    if [[ -n "$argc_name_only" ]]; then
        jq_args+=(-r '.models[].name')
    fi
    _wrapper curl -fsSL "https://generativelanguage.googleapis.com/v1beta/models?key=${GEMINI_API_KEY}" \
-H 'Content-Type: application/json' \
    | jq "${jq_args[@]}"
}

# @cmd Chat with claude api
# @env CLAUDE_API_KEY!
# @option -m --model=claude-3-haiku-20240307 $CLAUDE_MODEL
# @flag -S --no-stream
# @arg text~
chat-claude() {
    _wrapper curl -i https://api.anthropic.com/v1/messages \
-X POST \
-H 'content-type: application/json' \
-H 'anthropic-version: 2023-06-01' \
-H 'anthropic-beta: tools-2024-05-16' \
-H "x-api-key: $CLAUDE_API_KEY" \
-d "$(_build_body claude "$@")"
}

# @cmd List claude models
# @env CLAUDE_API_KEY!
# @flag --name-only Print model name only
models-claude() {
    jq_args=()
    if [[ -n "$argc_name_only" ]]; then
        jq_args+=(-r '.data[].id')
    fi
    _wrapper curl -fsSL "https://api.anthropic.com/v1/models" \
-H 'Content-Type: application/json' \
-H 'anthropic-version: 2023-06-01' \
-H "x-api-key: $CLAUDE_API_KEY" \
    | jq "${jq_args[@]}"
}

# @cmd Chat with cohere api
# @env COHERE_API_KEY!
# @option -m --model=command-r-08-2024 $COHERE_MODEL
# @flag -S --no-stream
# @arg text~
chat-cohere() {
    _wrapper curl -i https://api.cohere.ai/v2/chat \
-X POST \
-H 'Content-Type: application/json' \
-H "Authorization: Bearer $COHERE_API_KEY" \
-d "$(_build_body cohere "$@")"
}

# @cmd List cohere models
# @env COHERE_API_KEY!
# @flag --name-only Print model name only
models-cohere() {
    jq_args=()
    if [[ -n "$argc_name_only" ]]; then
        jq_args+=(-r '.models[].name')
    fi
    _wrapper curl -fsSL https://api.cohere.ai/v1/models \
-H "Authorization: Bearer $COHERE_API_KEY" \
    | jq "${jq_args[@]}"
}

# @cmd Chat with vertexai api
# @env require-tools gcloud
# @env VERTEXAI_PROJECT_ID!
# @env VERTEXAI_LOCATION!
# @option -m --model=gemini-1.5-flash-002 $VERTEXAI_GEMINI_MODEL
# @flag -S --no-stream
# @arg text~
chat-vertexai() {
    api_key="$(gcloud auth print-access-token)"
    func="streamGenerateContent"
    if [[ -n "$argc_no_stream" ]]; then
        func="generateContent"
    fi
    url=https://$VERTEXAI_LOCATION-aiplatform.googleapis.com/v1/projects/$VERTEXAI_PROJECT_ID/locations/$VERTEXAI_LOCATION/publishers/google/models/$argc_model:$func
    _wrapper curl -i $url \
-X POST \
-H "Authorization: Bearer $api_key" \
-H 'Content-Type: application/json' \
-d "$(_build_body vertexai "$@")" 
}

_argc_before() {
    OPENAI_COMPATIBLE_PROVIDERS=( \
        openai,gpt-4o-mini,https://api.openai.com/v1 \
        ai21,jamba-1.5-mini,https://api.ai21.com/studio/v1 \
        cloudflare,@cf/meta/llama-3.1-8b-instruct,https://api.cloudflare.com/client/v4/accounts/${CLOUDFLARE_ACCOUNT_ID}/ai/v1 \
        deepinfra,meta-llama/Meta-Llama-3.1-8B-Instruct,https://api.deepinfra.com/v1/openai \
        deepseek,deepseek-chat,https://api.deepseek.com \
        ernie,ernie-4.0-turbo-8k-latest,https://qianfan.baidubce.com/v2 \
        github,gpt-4o-mini,https://models.inference.ai.azure.com \
        groq,llama-3.1-8b-instant,https://api.groq.com/openai/v1 \
        hunyuan,hunyuan-large,https://api.hunyuan.cloud.tencent.com/v1 \
        minimax,MiniMax-Text-01,https://api.minimax.chat/v1 \
        mistral,mistral-small-latest,https://api.mistral.ai/v1 \
        moonshot,moonshot-v1-8k,https://api.moonshot.cn/v1 \
        openrouter,openai/gpt-4o-mini,https://openrouter.ai/api/v1 \
        perplexity,llama-3.1-8b-instruct,https://api.perplexity.ai \
        qianwen,qwen-turbo-latest,https://dashscope.aliyuncs.com/compatible-mode/v1 \
        xai,grok-beta,https://api.x.ai/v1 \
        zhipuai,glm-4-0520,https://open.bigmodel.cn/api/paas/v4 \
    )

    stream="true"
    if [[ -n "$argc_no_stream" ]]; then
        stream="false"
    fi
}

_openai_compatible_models() {
    api_base="${api_base:-"$argc_api_base"}"
    api_key="${api_key:-"$argc_api_key"}"
    url="${api_base}/models"
    if [[ "$argc_provider" == "cloudflare" ]]; then
        url="https://api.cloudflare.com/client/v4/accounts/${CLOUDFLARE_ACCOUNT_ID}/ai/models/search"
    fi

    _wrapper curl -fsSL "$url" \
-H "Authorization: Bearer $api_key" \

}

_retrieve_api_base() {
    api_base="${provider_config##*,}"
    if [[ -z "$api_base" ]]; then
        key="$(echo $argc_provider |  tr '[:lower:]' '[:upper:]')_API_BASE"
        api_base="${!key}"
        if [[ -z "$api_base" ]]; then
            _die "error: miss api_base for $argc_provider; please set $key"
        fi
    fi
}

_choice_model() {
    aichat --list-models
}

_choice_provider_model() {
    _choice_provider
    _choice_model
}

_choice_provider() {
    _choice_client
    _choice_openai_compatible_provider
}

_choice_client() {
    printf "%s\n" gemini claude cohere azure-openai vertexai bedrock
}

_choice_openai_compatible_provider() {
    for provider_config in "${OPENAI_COMPATIBLE_PROVIDERS[@]}"; do
        echo "${provider_config%%,*}"
    done
}

_build_body() {
    kind="$1"
    if [[ "$#" -eq 1 ]]; then
        file="${BODY_FILE:-"tmp/body/$1.json"}"
        if [[ -f "$file" ]]; then
            cat "$file" | \
            sed -E \
                -e 's%"model": ".*"%"model": "'"$argc_model"'"%' \
                -e 's%"stream": (true|false)%"stream": '$stream'%' \

        fi
    else
        shift
        case "$kind" in
        openai|cohere)
            echo '{
    "model": "'$argc_model'",
    "messages": [
        {
            "role": "user",
            "content": "'"$*"'"
        }
    ],
    "stream": '$stream'
}'
            ;;
        claude)
            echo '{
    "model": "'$argc_model'",
    "messages": [
        {
            "role": "user",
            "content": "'"$*"'"
        }
    ],
    "max_tokens": 4096,
    "stream": '$stream'
}'

            ;;
        gemini|vertexai)
            echo '{
    "contents": [{
        "role": "user",
        "parts": [
            {
                "text": "'"$*"'"
            }
        ]
    }],
    "safetySettings":[{"category":"HARM_CATEGORY_HARASSMENT","threshold":"BLOCK_ONLY_HIGH"},{"category":"HARM_CATEGORY_HATE_SPEECH","threshold":"BLOCK_ONLY_HIGH"},{"category":"HARM_CATEGORY_SEXUALLY_EXPLICIT","threshold":"BLOCK_ONLY_HIGH"},{"category":"HARM_CATEGORY_DANGEROUS_CONTENT","threshold":"BLOCK_ONLY_HIGH"}]
}'
            ;;
        *)
            _die "error: unsupported build body for $kind"
            ;;
        esac

    fi
}

_wrapper() {
    if [[ "$DRY_RUN" == "true" ]] || [[ "$DRY_RUN" == "1" ]]; then
        echo "$@" >&2
    else
        "$@"
    fi
}

_die() {
    echo $*
    exit 1
}

# See more details at https://github.com/sigoden/argc
eval "$(argc --argc-eval "$0" "$@")"
