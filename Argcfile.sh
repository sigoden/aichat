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

# @cmd Test running without the config file
# @env AICHAT_CLIENT_TYPE!
# @arg args~
test-without-config() {
    cargo run -- "$@"
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
# @option -m --model=default
# @flag -S --no-stream
# @arg text~
test-server() {
    args=()
    if [[ -n "$argc_no_stream" ]]; then
        args+=("-S")
    fi
    argc generic-chat "${args[@]}" \
    --api-base http://localhost:8000/v1 \
    --model $argc_model \
    "$@"
}

# @cmd Chat with openai-comptabile api
# @option --api-base! $$ 
# @option --api-key! $$
# @option -m --model! $$
# @flag -S --no-stream
# @arg text~
generic-chat() {
    curl_args="$CURL_ARGS"
    _openai_chat "$@"
}

# @cmd List models by openai-comptabile api
# @option --api-base! $$
# @option --api-key! $$
generic-models() {
    curl_args="$CURL_ARGS"
    _openai_models
}


# @cmd Chat with openai api
# @env OPENAI_API_KEY!
# @option -m --model=gpt-3.5-turbo $OPENAI_MODEL
# @flag -S --no-stream
# @arg text~
openai-chat() {
    api_base=https://api.openai.com/v1
    api_key=$OPENAI_API_KEY
    curl_args="-i $OPENAI_CURL_ARGS"
    _openai_chat "$@"
}

# @cmd List openai models
# @env OPENAI_API_KEY!
openai-models() {
    api_base=https://api.openai.com/v1
    api_key=$OPENAI_API_KEY
    curl_args="$OPENAI_CURL_ARGS"
    _openai_models
}

# @cmd Chat with gemini api
# @env GEMINI_API_KEY!
# @option -m --model=gemini-1.0-pro-latest $GEMINI_MODEL
# @flag -S --no-stream
# @arg text~
gemini-chat() {
    method="streamGenerateContent"
    if [[ -n "$argc_no_stream" ]]; then
        method="generateContent"
    fi
    _wrapper curl -i $GEMINI_CURL_ARGS "https://generativelanguage.googleapis.com/v1beta/models/${argc_model}:${method}?key=${GEMINI_API_KEY}" \
-i -X POST \
-H 'Content-Type: application/json' \
-d '{ 
    "safetySettings":[{"category":"HARM_CATEGORY_HARASSMENT","threshold":"BLOCK_ONLY_HIGH"},{"category":"HARM_CATEGORY_HATE_SPEECH","threshold":"BLOCK_ONLY_HIGH"},{"category":"HARM_CATEGORY_SEXUALLY_EXPLICIT","threshold":"BLOCK_ONLY_HIGH"},{"category":"HARM_CATEGORY_DANGEROUS_CONTENT","threshold":"BLOCK_ONLY_HIGH"}],
    "contents": '"$(_build_msg_gemini $*)"'
}'
}

# @cmd List gemini models
# @env GEMINI_API_KEY!
gemini-models() {
    _wrapper curl $GEMINI_CURL_ARGS "https://generativelanguage.googleapis.com/v1beta/models?key=${GEMINI_API_KEY}" \
-H 'Content-Type: application/json' \

}

# @cmd Chat with claude api
# @env CLAUDE_API_KEY!
# @option -m --model=claude-3-haiku-20240307 $CLAUDE_MODEL
# @flag -S --no-stream
# @arg text~
claude-chat() {
    _wrapper curl -i $CLAUDE_CURL_ARGS https://api.anthropic.com/v1/messages \
-X POST \
-H 'content-type: application/json' \
-H 'anthropic-version: 2023-06-01' \
-H "x-api-key: $CLAUDE_API_KEY" \
-d '{
  "model": "'$argc_model'",
  "messages": '"$(_build_msg $*)"',
  "max_tokens": 4096,
  "stream": '$stream'
}
'
}

# @cmd Chat with mistral api
# @env MISTRAL_API_KEY!
# @option -m --model=mistral-small-latest $MISTRAL_MODEL
# @flag -S --no-stream
# @arg text~
mistral-chat() {
    api_base=https://api.mistral.ai/v1
    api_key=$MISTRAL_API_KEY
    curl_args="$MISTRAL_CURL_ARGS"
    _openai_chat "$@"
}

# @cmd List mistral models
# @env MISTRAL_API_KEY!
mistral-models() {
    api_base=https://api.mistral.ai/v1
    api_key=$MISTRAL_API_KEY
    curl_args="$MISTRAL_CURL_ARGS"
    _openai_models
}

# @cmd Chat with cohere api
# @env COHERE_API_KEY!
# @option -m --model=command-r $COHERE_MODEL
# @flag -S --no-stream
# @arg text~
cohere-chat() {
    _wrapper curl -i $COHERE_CURL_ARGS https://api.cohere.ai/v1/chat \
-X POST \
-H 'Content-Type: application/json' \
-H "Authorization: Bearer $COHERE_API_KEY" \
--data '{
  "model": "'$argc_model'",
  "message": "'"$*"'",
  "stream": '$stream'
}
'
}

# @cmd List cohere models
# @env COHERE_API_KEY!
cohere-models() {
    _wrapper curl $COHERE_CURL_ARGS https://api.cohere.ai/v1/models \
-H "Authorization: Bearer $COHERE_API_KEY" \

}

# @cmd Chat with perplexity api
# @env PERPLEXITY_API_KEY!
# @option -m --model=sonar-small-chat $PERPLEXITY_MODEL
# @flag -S --no-stream
# @arg text~
perplexity-chat() {
    api_base=https://api.perplexity.ai
    api_key=$PERPLEXITY_API_KEY
    curl_args="$PERPLEXITY_CURL_ARGS"
    _openai_chat "$@"
}

# @cmd Chat with groq api
# @env GROQ_API_KEY!
# @option -m --model=llama3-70b-8192 $GROQ_MODEL
# @flag -S --no-stream
# @arg text~
groq-chat() {
    api_base=https://api.groq.com/openai/v1
    api_key=$GROQ_API_KEY
    curl_args="$GROQ_CURL_ARGS"
    _openai_chat "$@"
}

# @cmd List groq models
# @env GROQ_API_KEY!
groq-models() {
    api_base=https://api.groq.com/openai/v1
    api_key=$GROQ_API_KEY
    curl_args="$GROQ_CURL_ARGS"
    _openai_models
}

# @cmd Chat with ollama api
# @option -m --model=codegemma $OLLAMA_MODEL
# @flag -S --no-stream
# @arg text~
ollama-chat() {
    _wrapper curl -i $OLLAMA_CURL_ARGS http://localhost:11434/api/chat \
-X POST \
-H 'Content-Type: application/json' \
-d '{
    "model": "'$argc_model'",
    "stream": '$stream',
    "messages": '"$(_build_msg $*)"'
}'
}

# @cmd Chat with vertexai-gemini api
# @env require-tools gcloud
# @env VERTEXAI_PROJECT_ID!
# @env VERTEXAI_LOCATION!
# @option -m --model=gemini-1.0-pro $VERTEXAI_GEMINI_MODEL
# @flag -S --no-stream
# @arg text~
vertexai-gemini-chat() {
    api_key="$(gcloud auth print-access-token)"
    func="streamGenerateContent"
    if [[ -n "$argc_no_stream" ]]; then
        func="generateContent"
    fi
    url=https://$VERTEXAI_LOCATION-aiplatform.googleapis.com/v1/projects/$VERTEXAI_PROJECT_ID/locations/$VERTEXAI_LOCATION/publishers/google/models/$argc_model:$func
    _wrapper curl -i $VERTEXAI_CURL_ARGS $url \
-X POST \
-H "Authorization: Bearer $api_key" \
-H 'Content-Type: application/json' \
-d '{ 
    "contents": '"$(_build_msg_gemini $*)"',
    "generationConfig": {}
}'
}

# @cmd Chat with vertexai-claude api
# @env require-tools gcloud
# @env VERTEXAI_PROJECT_ID!
# @env VERTEXAI_LOCATION!
# @option -m --model=claude-3-haiku@20240307 $VERTEXAI_CLAUDE_MODEL
# @flag -S --no-stream
# @arg text~
vertexai-claude-chat() {
    api_key="$(gcloud auth print-access-token)"
    url=https://$VERTEXAI_LOCATION-aiplatform.googleapis.com/v1/projects/$VERTEXAI_PROJECT_ID/locations/$VERTEXAI_LOCATION/publishers/anthropic/models/$argc_model:streamRawPredict
    _wrapper curl -i $VERTEXAI_CURL_ARGS $url \
-X POST \
-H "Authorization: Bearer $api_key" \
-H 'Content-Type: application/json' \
-d '{
  "anthropic_version": "vertex-2023-10-16",
  "messages": '"$(_build_msg $*)"',
  "max_tokens": 4096,
  "stream": '$stream'
}'
}

# @cmd Chat with bedrock api
# @meta require-tools aws
# @option -m --model=mistral.mistral-7b-instruct-v0:2 $BEDROCK_MODEL
# @env AWS_REGION=us-east-1
bedrock-chat() {
    file="$(mktemp)"
    case "$argc_model" in
        mistral.* | meta.*)
            body='{"prompt":"'"$*"'"}'
            ;;
        anthropic.*)
            body='{
  "anthropic_version": "vertex-2023-10-16",
  "messages": '"$(_build_msg $*)"',
  "max_tokens": 4096
}'
            ;;
        *)
            _die "Invalid model: $argc_model"
            ;;
    esac

    _wrapper aws bedrock-runtime invoke-model \
        --model-id $argc_model \
        --region $AWS_REGION \
        --body "$(echo "$body" | base64)" \
        "$file"
    cat "$file"
}

# @cmd Chat with ernie api
# @meta require-tools jq
# @env ERNIE_API_KEY!
# @option -m --model=ernie-tiny-8k $ERNIE_MODEL
# @flag -S --no-stream
# @arg text~
ernie-chat() {
    ACCESS_TOKEN="$(curl -fsSL "https://aip.baidubce.com/oauth/2.0/token?grant_type=client_credentials&client_id=$ERNIE_API_KEY&client_secret=$ERNIE_SECRET_KEY" | jq -r '.access_token')"
    _wrapper curl -i $ERNIE_CURL_ARGS "https://aip.baidubce.com/rpc/2.0/ai_custom/v1/wenxinworkshop/chat/$argc_model?access_token=$ACCESS_TOKEN" \
-X POST \
-d '{
    "messages": '"$(_build_msg $*)"',
    "stream": '$stream'
}'
}


# @cmd Chat with qianwen api
# @env QIANWEN_API_KEY!
# @option -m --model=qwen-turbo $QIANWEN_MODEL
# @flag -S --no-stream
# @arg text~
qianwen-chat() {
    stream_args="-H X-DashScope-SSE:enable"
    parameters_args='{"incremental_output": true}'
    if [[ -n "$argc_no_stream" ]]; then
        stream_args=""
        parameters_args='{}'
    fi
    parameters_args='{ "temperature": 0.5 }'

    _wrapper curl -i $QIANWEN_CURL_ARGS 'https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation' \
-X POST \
-H "Authorization: Bearer $QIANWEN_API_KEY" \
-H 'Content-Type: application/json' $stream_args  \
-d '{
    "model": "'$argc_model'",
    "parameters": '"$parameters_args"',
    "input":{
        "messages": '"$(_build_msg $*)"'
    }
}'
}

# @cmd Chat with moonshot api
# @env MOONSHOT_API_KEY!
# @option -m --model=moonshot-v1-8k @MOONSHOT_MODEL
# @flag -S --no-stream
# @arg text~
moonshot-chat() {
    api_base=https://api.moonshot.cn/v1
    api_key=$MOONSHOT_API_KEY
    curl_args="$MOONSHOT_CURL_ARGS"
    _openai_chat "$@"
}

# @cmd List moonshot models
# @env MOONSHOT_API_KEY!
moonshot-models() {
    api_base=https://api.moonshot.cn/v1
    api_key=$MOONSHOT_API_KEY
    curl_args="$MOONSHOT_CURL_ARGS"
    _openai_models
}

_argc_before() {
    stream="true"
    if [[ -n "$argc_no_stream" ]]; then
        stream="false"
    fi
}

_openai_chat() {
    api_base="${api_base:-"$argc_api_base"}"
    api_key="${api_key:-"$argc_api_key"}"
    _wrapper curl -i $curl_args "$api_base/chat/completions" \
-X POST \
-H "Content-Type: application/json" \
-H "Authorization: Bearer $api_key" \
--data '{
  "model": "'$argc_model'",
  "messages": '"$(_build_msg $*)"',
  "stream": '$stream'
}
'
}

_openai_models() {
    api_base="${api_base:-"$argc_api_base"}"
    api_key="${api_key:-"$argc_api_key"}"
    _wrapper curl $curl_args "$api_base/models" \
-H "Authorization: Bearer $api_key" \

}

_choice_client() {
    printf "%s\n" openai gemini claude mistral cohere ollama vertexai bedrock ernie qianwen moonshot
}

_build_msg() {
    if [[ $# -eq 0 ]]; then
        cat tmp/messages.json
    else
        echo '
[
    {
        "role": "user",
        "content": "'"$*"'"
    }
]
'
    fi
}

_build_msg_gemini() {
    if [[ $# -eq 0 ]]; then
        cat tmp/messages.gemini.json
    else
        echo '
[{
    "role": "user",
    "parts": [
        {
            "text": "'"$*"'"
        }
    ]
}]
'
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
