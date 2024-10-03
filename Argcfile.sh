#!/usr/bin/env bash
set -e

# @meta dotenv
# @env DRY_RUN Dry run mode

# @cmd Test first running
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

# @cmd Test running with AICHAT_PLATFORM environment variable
# @env AICHAT_PLATFORM!
# @arg args~
test-platform-env() {
    cargo run -- "$@"
}

# @cmd Test function calling
# @option --model[?`_choice_model`]
# @option --preset[=default|weather|multi-weathers]
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

OPENAI_COMPATIBLE_PLATFORMS=( \
  openai,gpt-4o-mini,https://api.openai.com/v1 \
  ai21,jamba-1.5-mini,https://api.ai21.com/studio/v1 \
  cloudflare,@cf/meta/llama-3.1-8b-instruct, \
  deepinfra,meta-llama/Meta-Llama-3.1-8B-Instruct,https://api.deepinfra.com/v1/openai \
  deepseek,deepseek-chat,https://api.deepseek.com \
  fireworks,accounts/fireworks/models/llama-v3p1-8b-instruct,https://api.fireworks.ai/inference/v1 \
  github,gpt-4o-mini,https://models.inference.ai.azure.com \
  groq,llama3-8b-8192,https://api.groq.com/openai/v1 \
  huggingface,meta-llama/Meta-Llama-3-8B-Instruct,https://api-inference.huggingface.co/v1 \
  lingyiwanwu,yi-large,https://api.lingyiwanwu.com/v1 \
  mistral,open-mistral-nemo,https://api.mistral.ai/v1 \
  moonshot,moonshot-v1-8k,https://api.moonshot.cn/v1 \
  openrouter,openai/gpt-4o-mini,https://openrouter.ai/api/v1 \
  ollama,llama3.1:latest,http://localhost:11434/v1 \
  perplexity,llama-3.1-8b-instruct,https://api.perplexity.ai \
  qianwen,qwen-turbo,https://dashscope.aliyuncs.com/compatible-mode/v1 \
  siliconflow,meta-llama/Meta-Llama-3.1-8B-Instruct,https://api.siliconflow.cn/v1 \
  together,meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo,https://api.together.xyz/v1 \
  zhipuai,glm-4-0520,https://open.bigmodel.cn/api/paas/v4 \
)

# @cmd Chat with any LLM api 
# @flag -S --no-stream
# @arg platform_model![?`_choice_platform_model`]
# @arg text~
chat() {
    if [[ "$argc_platform_model" == *':'* ]]; then
        model="${argc_platform_model##*:}"
        argc_platform="${argc_platform_model%:*}"
    else
        argc_platform="${argc_platform_model}"
    fi
    for platform_config in "${OPENAI_COMPATIBLE_PLATFORMS[@]}"; do
        if [[ "$argc_platform" == "${platform_config%%,*}" ]]; then
            _retrieve_api_base
            break
        fi
    done
    if [[ -n "$api_base" ]]; then
        env_prefix="$(echo "$argc_platform" | tr '[:lower:]' '[:upper:]')"
        api_key_env="${env_prefix}_API_KEY"
        api_key="${!api_key_env}" 
        if [[ -z "$model" ]]; then
            model="$(echo "$platform_config" | cut -d, -f2)"
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
        argc chat-$argc_platform "${argc_text[@]}"
    fi
}

# @cmd List models by openai-compatible api
# @arg platform![`_choice_platform`]
models() {
    for platform_config in "${OPENAI_COMPATIBLE_PLATFORMS[@]}"; do
        if [[ "$argc_platform" == "${platform_config%%,*}" ]]; then
            _retrieve_api_base
            break
        fi
    done
    if [[ -n "$api_base" ]]; then
        env_prefix="$(echo "$argc_platform" | tr '[:lower:]' '[:upper:]')"
        api_key_env="${env_prefix}_API_KEY"
        api_key="${!api_key_env}" 
        _retrieve_models
    else
        argc models-$argc_platform
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
models-openai-compatible() {
    _retrieve_models
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
# @option -m --model=gemini-1.0-pro-latest $GEMINI_MODEL
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
models-gemini() {
    _wrapper curl "https://generativelanguage.googleapis.com/v1beta/models?key=${GEMINI_API_KEY}" \
-H 'Content-Type: application/json' \

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

# @cmd Chat with cohere api
# @env COHERE_API_KEY!
# @option -m --model=command-r $COHERE_MODEL
# @flag -S --no-stream
# @arg text~
chat-cohere() {
    _wrapper curl -i https://api.cohere.ai/v1/chat \
-X POST \
-H 'Content-Type: application/json' \
-H "Authorization: Bearer $COHERE_API_KEY" \
-d "$(_build_body cohere "$@")"
}

# @cmd List cohere models
# @env COHERE_API_KEY!
models-cohere() {
    _wrapper curl https://api.cohere.ai/v1/models \
-H "Authorization: Bearer $COHERE_API_KEY" \

}

# @cmd Chat with vertexai api
# @env require-tools gcloud
# @env VERTEXAI_PROJECT_ID!
# @env VERTEXAI_LOCATION!
# @option -m --model=gemini-1.0-pro $VERTEXAI_GEMINI_MODEL
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

# @cmd Chat with ernie api
# @meta require-tools jq
# @env ERNIE_API_KEY!
# @option -m --model=ernie-tiny-8k $ERNIE_MODEL
# @flag -S --no-stream
# @arg text~
chat-ernie() {
    auth_url="https://aip.baidubce.com/oauth/2.0/token?grant_type=client_credentials&client_id=$ERNIE_API_KEY&client_secret=$ERNIE_SECRET_KEY"
    ACCESS_TOKEN="$(curl -fsSL "$auth_url" | jq -r '.access_token')"
    url="https://aip.baidubce.com/rpc/2.0/ai_custom/v1/wenxinworkshop/chat/$argc_model?access_token=$ACCESS_TOKEN"
    _wrapper curl -i "$url" \
-X POST \
-d "$(_build_body ernie "$@")"
}

_argc_before() {
    stream="true"
    if [[ -n "$argc_no_stream" ]]; then
        stream="false"
    fi
}

_retrieve_models() {
    api_base="${api_base:-"$argc_api_base"}"
    api_key="${api_key:-"$argc_api_key"}"
    _wrapper curl "$api_base/models" \
-H "Authorization: Bearer $api_key" \

}

_retrieve_api_base() {
    api_base="${platform_config##*,}"
    if [[ -z "$api_base" ]]; then
        key="$(echo $argc_platform |  tr '[:lower:]' '[:upper:]')_API_BASE"
        api_base="${!key}"
        if [[ -z "$api_base" ]]; then
            _die "Miss api_base for $argc_platform; please set $key"
        fi
    fi
}

_choice_model() {
    aichat --list-models
}

_choice_platform_model() {
    _choice_platform
    _choice_model
}

_choice_platform() {
    _choice_client
    _choice_openai_compatible_platform
}

_choice_client() {
    printf "%s\n" openai gemini claude cohere ollama azure-openai vertexai bedrock cloudflare ernie qianwen moonshot
}

_choice_openai_compatible_platform() {
    for platform_config in "${OPENAI_COMPATIBLE_PLATFORMS[@]}"; do
        echo "${platform_config%%,*}"
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
        openai)
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
        cohere)
            echo '{
  "model": "'$argc_model'",
  "message": "'"$*"'",
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
        ernie)
            echo '{
    "messages": [
        {
            "role": "user",
            "content": "'"$*"'"
        }
    ],
    "stream": '$stream'
}'
            ;;
        *)
            _die "Unsupported build body for $kind"
            ;;
        esac

    fi
}

_wrapper() {
    if [[ "$DRY_RUN" == "true" ]] || [[ "$DRY_RUN" == "1" ]]; then
        echo "$@"
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
