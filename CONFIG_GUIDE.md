# AIChat 完整配置指南

## 配置目录结构

```
~/.config/aichat/
├── config.yaml              # 主配置文件
├── .env                     # 环境变量文件
├── models-override.yaml     # 模型覆盖文件
├── messages.md              # 消息文件
├── roles/                   # 角色目录
│   └── *.md                # 角色文件
├── sessions/                # 会话目录
│   └── *.yaml              # 会话文件
├── rags/                    # RAG目录
│   └── *.yaml              # RAG配置文件
├── macros/                  # 宏目录
│   └── *.yaml              # 宏文件
├── functions/               # 函数目录
│   ├── functions.json      # 函数定义文件
│   ├── bin/                # 函数二进制目录
│   └── agents/             # 代理函数目录
│       ├── agents.txt      # 代理注册文件
│       └── <agent-name>/   # 各个代理目录
│           └── index.yaml  # 代理定义文件
└── agents/                 # 代理数据目录
    └── <agent-name>/       # 各个代理数据目录
        ├── config.yaml     # 代理配置文件
        ├── sessions/       # 代理会话目录
        └── messages.md     # 代理消息文件
```

## 环境变量覆盖

所有配置路径都支持通过环境变量覆盖：

### 主配置环境变量
```bash
# 配置目录
AICHAT_CONFIG_DIR=/custom/path

# 具体文件路径
AICHAT_CONFIG_FILE=/custom/config.yaml
AICHAT_ENV_FILE=/custom/.env
AICHAT_MESSAGES_FILE=/custom/messages.md
AICHAT_LOG_PATH=/custom/aichat.log

# 目录路径
AICHAT_ROLES_DIR=/custom/roles
AICHAT_SESSIONS_DIR=/custom/sessions
AICHAT_RAGS_DIR=/custom/rags
AICHAT_MACROS_DIR=/custom/macros
AICHAT_FUNCTIONS_DIR=/custom/functions
```

### 代理特定环境变量
```bash
# 代理数据目录
<AGENT_NAME>_DATA_DIR=/custom/agent-data

# 代理配置文件
<AGENT_NAME>_CONFIG_FILE=/custom/agent-config.yaml

# 代理函数目录
<AGENT_NAME>_FUNCTIONS_DIR=/custom/agent-functions
```

注意：`<AGENT_NAME>` 需要转换为大写并用下划线替换连字符，例如 `code-assistant` 变为 `CODE_ASSISTANT`

## 配置文件详解

### 1. 主配置文件 (config.yaml)

```yaml
# ---- LLM 配置 ----
model: openai:gpt-4o             # 指定使用的LLM
temperature: null                # 默认温度参数 (0-1)
top_p: null                      # 默认top-p参数

# ---- 行为配置 ----
stream: true                     # 是否使用流式API
save: true                       # 是否持久化消息
keybindings: emacs               # 键绑定风格 (emacs, vi)
editor: null                     # 编辑命令 (vim, emacs, nano)
wrap: no                         # 文本换行 (no, auto, <max-width>)
wrap_code: false                 # 是否换行代码块

# ---- 函数调用配置 ----
function_calling: true           # 启用函数调用
mapping_tools:                   # 工具别名映射
  fs: 'fs_cat,fs_ls,fs_mkdir,fs_rm,fs_write'
use_tools: null                  # 默认使用的工具

# ---- 预置配置 ----
repl_prelude: null               # REPL模式默认角色/会话
cmd_prelude: null                # CMD模式默认角色/会话
agent_prelude: null              # 代理启动时使用的会话

# ---- 会话配置 ----
save_session: null               # 会话持久化控制
compress_threshold: 4000         # 会话压缩阈值
summarize_prompt: 'Summarize the discussion briefly in 200 words or less'
summary_prompt: 'This is a summary of the chat history as a recap: '

# ---- RAG 配置 ----
rag_embedding_model: null        # RAG嵌入模型
rag_reranker_model: null         # RAG重排序模型
rag_top_k: 5                     # 检索文档数量
rag_chunk_size: null             # 文档分块大小
rag_chunk_overlap: null          # 分块重叠大小
rag_template: |                  # RAG查询模板
  Answer the query based on the context...

document_loaders:                # 文档加载器
  pdf: 'pdftotext $1 -'
  docx: 'pandoc --to plain $1'

# ---- 外观配置 ----
highlight: true                  # 语法高亮
light_theme: false               # 浅色主题
left_prompt: '{color.green}...'  # 左侧提示符
right_prompt: '{color.purple}...' # 右侧提示符

# ---- 其他配置 ----
serve_addr: 127.0.0.1:8000      # 服务器地址
user_agent: null                # User-Agent头
save_shell_history: true        # 保存shell历史
sync_models_url: https://...    # 模型同步URL

# ---- 客户端配置 ----
clients:
  - type: openai
    api_base: https://api.openai.com/v1
    api_key: xxx
    organization_id: org-xxx

  - type: openai-compatible
    name: ollama
    api_base: http://localhost:11434/v1
    models:
      - name: llama3.1
        max_input_tokens: 128000
        supports_function_calling: true
```

### 2. 角色文件 (*.md)

```markdown
---
model: openai:gpt-4o
temperature: 0.7
---

# 角色名称

角色描述和系统提示词...
```

### 3. 会话文件 (*.yaml)

```yaml
model_id: openai:gpt-4o
temperature: null
top_p: null
use_tools: null
save_session: null
compress_threshold: 4000

role_name: null
agent_variables: {}
agent_instructions: ""

compressed_messages: []
messages:
  - role: user
    content: Hello
  - role: assistant
    content: Hi there!

data_urls: {}
```

### 4. RAG配置文件 (*.yaml)

```yaml
name: my-rag
embedding_model: openai:text-embedding-3-small
reranker_model: null
chunk_size: 1000
chunk_overlap: 200
sources:
  - /path/to/document1.pdf
  - /path/to/document2.txt
```

### 5. 宏文件 (*.yaml)

```yaml
name: my-macro
description: 我的宏描述
commands:
  - .role my-role
  - .session my-session
  - Hello, how are you?
```

### 6. 代理定义文件 (index.yaml)

```yaml
name: "my-agent"
description: "代理描述"
version: "1.0.0"

instructions: |
  代理系统提示词...

dynamic_instructions: false

variables:
  - name: "language"
    description: "编程语言"
    default: "Python"

conversation_starters:
  - "对话启动器1"
  - "对话启动器2"

documents:
  - /path/to/document.md
```

### 7. 代理配置文件 (config.yaml)

```yaml
model_id: null
temperature: null
top_p: null
use_tools: null
agent_prelude: null
instructions: null
variables: {}
```

## 配置优先级

1. **命令行参数** (最高优先级)
2. **环境变量**
3. **配置文件**
4. **默认值** (最低优先级)

## 配置初始化

首次运行时，AIChat会：
1. 检查配置目录是否存在
2. 如果不存在且在有TTY的环境中，会交互式创建配置文件
3. 支持通过环境变量 `AICHAT_PROVIDER` 或 `AICHAT_PLATFORM` 动态加载配置

## 调试配置

```bash
# 查看当前配置
AICHAT_CONFIG_DIR=/custom/path aichat --info

# 列出所有可用配置
AICHAT_CONFIG_DIR=/custom/path aichat --list-models
AICHAT_CONFIG_DIR=/custom/path aichat --list-roles
AICHAT_CONFIG_DIR=/custom/path aichat --list-agents
```

这个配置系统提供了极大的灵活性，允许用户完全自定义AIChat的行为和存储位置。