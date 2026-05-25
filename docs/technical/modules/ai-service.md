# AI Issue 生成服务

源文件：`web-platform/src/services/ai_service.rs`

---

## 功能概述

AI Issue 生成服务允许用户通过 AI 模型自动生成 Issue 内容（标题、描述、验收标准等），支持流式响应（SSE），用户可实时看到生成过程。

主要功能：

- 根据用户输入的简短描述，生成结构化的 Issue 内容
- 流式输出，前端实时展示生成进度
- 支持用户随时取消生成
- 每用户和全局双层限流，防止滥用

---

## 架构

```
前端（React）
    │ POST /api/v1/ai/generate-issue（SSE 请求）
    │ AbortController 支持取消
    ▼
Web API Handler（handlers/ai.rs）
    │ 验证 JWT、检查限流
    ▼
AiService（services/ai_service.rs）
    │ 构建 Prompt
    │ 调用 AI API（流式）
    ▼
Azure OpenAI / OpenAI-compatible API
    │ Server-Sent Events 流
    ▼
Handler 转发 SSE 事件给前端
```

---

## 模型配置

AI 服务通过环境变量配置，两个必填变量均设置时服务才启用：

| 环境变量 | 必填 | 说明 |
|----------|------|------|
| `AZURE_OPENAI_BASEURL` | 是（启用 AI） | API 端点，支持 Azure OpenAI 和 OpenAI-compatible 端点 |
| `AZURE_OPENAI_API_KEY` | 是（启用 AI） | API 密钥 |
| `AZURE_OPENAI_MODEL` | 否 | 模型名称，默认 `gpt-5.5` |
| `AI_MODEL_FAMILY` | 否 | 模型系列：`legacy`（GPT-4 等）或 `gpt5`（GPT-5/推理模型） |
| `AI_MAX_TOKENS` | 否 | 最大生成 token 数，默认 `4096` |
| `AI_RATE_LIMIT_PER_MINUTE` | 否 | 每用户每分钟请求限制，默认 `10` |
| `AI_GLOBAL_RATE_LIMIT_PER_MINUTE` | 否 | 全局每分钟请求限制，默认 `30` |

**模型系列说明**：

- `legacy`（GPT-4 等旧版模型）：使用 `max_tokens` 参数，支持自定义 temperature
- `gpt5`（GPT-5 / 推理模型）：使用 `max_completion_tokens` 参数，使用默认采样参数

若未设置 `AI_MODEL_FAMILY`，服务会根据模型名称自动推断（包含 "gpt-5"、"o1"、"o3" 等关键词时使用 `gpt5` 系列）。

**端点配置示例**：

```bash
# Azure OpenAI
AZURE_OPENAI_BASEURL=https://<resource-name>.openai.azure.com
AZURE_OPENAI_API_KEY=<azure-api-key>
AZURE_OPENAI_MODEL=<deployment-name>

# OpenAI 官方 API
AZURE_OPENAI_BASEURL=https://api.openai.com/v1
AZURE_OPENAI_API_KEY=<openai-api-key>
AZURE_OPENAI_MODEL=gpt-4o

# 其他 OpenAI-compatible 端点（如 Ollama、vLLM）
AZURE_OPENAI_BASEURL=http://localhost:11434/v1
AZURE_OPENAI_API_KEY=ollama
AZURE_OPENAI_MODEL=llama3
```

---

## 流式响应实现

### 后端（SSE）

AI 生成接口使用 Server-Sent Events（SSE）协议推送流式响应：

```
HTTP Response Headers:
    Content-Type: text/event-stream
    Cache-Control: no-cache
    Connection: keep-alive

事件流格式：
    data: {"type": "chunk", "content": "生成的文本片段"}\n\n
    data: {"type": "done"}\n\n
    data: {"type": "error", "message": "错误信息"}\n\n
```

后端通过 `reqwest` 的流式响应（`response.bytes_stream()`）逐块读取 AI API 的 SSE 输出，解析后转发给前端。

### 前端（AbortController）

前端使用 `fetch` + `AbortController` 实现可取消的 SSE 请求：

```typescript
const controller = new AbortController();

const response = await fetch('/api/v1/ai/generate-issue', {
    method: 'POST',
    headers: { 'Authorization': `Bearer ${token}` },
    body: JSON.stringify({ description: userInput }),
    signal: controller.signal,  // 绑定取消信号
});

// 用户点击取消时
controller.abort();
```

前端收到 `abort` 信号后，服务端检测到连接断开，停止向 AI API 发送请求。

---

## 限流机制

双层限流防止滥用：

**用户级限流**（`AI_RATE_LIMIT_PER_MINUTE`，默认 10）

- 基于用户 ID 的滑动窗口计数
- 超限返回 `429 Too Many Requests`

**全局限流**（`AI_GLOBAL_RATE_LIMIT_PER_MINUTE`，默认 30）

- 所有用户共享的全局计数器
- 超限返回 `429 Too Many Requests`

限流实现：`web-platform/src/lib.rs` 中的 `Phase3RateLimiter`，使用 `VecDeque` 维护时间窗口内的请求时间戳。

---

## Token 验证

用户可通过以下接口验证自己配置的 AI API Key 是否有效：

```
POST /api/user/config/validate-token
Authorization: Bearer <jwt>

Request Body:
{
    "provider": "openai",
    "api_key": "sk-...",
    "base_url": "https://api.openai.com/v1"  // 可选
}

Response:
{
    "valid": true,
    "model": "gpt-4o"  // 验证成功时返回可用模型
}
```

验证逻辑：发送一个最小化的 API 请求（如列出模型），检查是否返回 200。

---

## 错误处理

| 错误场景 | HTTP 状态码 | 说明 |
|----------|-------------|------|
| AI 服务未配置 | `503 Service Unavailable` | `AZURE_OPENAI_BASEURL` 或 `AZURE_OPENAI_API_KEY` 未设置 |
| 用户限流 | `429 Too Many Requests` | 超过每用户每分钟限制 |
| 全局限流 | `429 Too Many Requests` | 超过全局每分钟限制 |
| API 认证失败 | `502 Bad Gateway` | AI API 返回 401/403 |
| API 超时 | `504 Gateway Timeout` | AI API 响应超时 |
| 模型不可用 | `502 Bad Gateway` | 指定模型不存在或不可用 |

流式响应中的错误通过 SSE 事件传递：

```
data: {"type": "error", "message": "AI API authentication failed"}\n\n
```

---

## 用户配置存储

用户级 AI 配置（自定义 API Key、端点等）存储在 `user_configs` 表中，API Key 使用 AES-GCM 加密存储。

相关 API：

```
GET  /api/user/config          — 获取当前用户配置
PUT  /api/user/config          — 更新用户配置
POST /api/user/config/validate-token  — 验证 API Key
```

用户配置的优先级高于系统级环境变量配置（若用户设置了自己的 API Key，则使用用户的配置）。
