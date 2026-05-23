use futures::stream::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Configuration for the AI service.
#[derive(Debug, Clone)]
pub struct AiServiceConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub model_family: AiModelFamily,
    pub max_tokens: u32,
    pub rate_limit_per_minute: u32,
    pub global_rate_limit_per_minute: u32,
}

/// Parameter compatibility family for the configured AI model/deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiModelFamily {
    /// Older chat-completion models that support `max_tokens` and custom temperature.
    Legacy,
    /// GPT-5 / reasoning-style models that require `max_completion_tokens` and default sampling.
    Gpt5,
}

impl AiModelFamily {
    fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "legacy" | "chat" | "gpt4" | "gpt-4" => Some(Self::Legacy),
            "gpt5" | "gpt-5" | "reasoning" | "o-series" | "modern" => Some(Self::Gpt5),
            _ => None,
        }
    }

    fn infer_from_model(model: &str) -> Self {
        if model_supports_max_completion_tokens(model) {
            Self::Gpt5
        } else {
            Self::Legacy
        }
    }
}

impl AiServiceConfig {
    /// Load configuration from environment variables.
    /// Returns None if the required env vars are not set (AI feature disabled).
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("AZURE_OPENAI_BASEURL").ok()?;
        let api_key = std::env::var("AZURE_OPENAI_API_KEY").ok()?;
        let model = std::env::var("AZURE_OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.5".to_string());
        let model_family = std::env::var("AI_MODEL_FAMILY")
            .ok()
            .and_then(|v| AiModelFamily::from_env_value(&v))
            .unwrap_or_else(|| AiModelFamily::infer_from_model(&model));
        let max_tokens = std::env::var("AI_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4096);
        let rate_limit_per_minute = std::env::var("AI_RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let global_rate_limit_per_minute = std::env::var("AI_GLOBAL_RATE_LIMIT_PER_MINUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        Some(Self {
            base_url,
            api_key,
            model,
            model_family,
            max_tokens,
            rate_limit_per_minute,
            global_rate_limit_per_minute,
        })
    }
}

/// A streaming chunk from the AI service.
#[derive(Debug, Clone)]
pub enum AiStreamChunk {
    /// A text content delta.
    Content(String),
    /// The stream has finished successfully.
    Done,
    /// An error occurred during streaming.
    Error(String),
}

/// Rate limiter for AI generation requests (sliding window).
pub struct AiRateLimiter {
    /// Per-user request timestamps: user_id -> timestamps
    user_windows: Mutex<dashmap::DashMap<i64, VecDeque<Instant>>>,
    /// Global request timestamps
    global_window: Mutex<VecDeque<Instant>>,
    per_user_limit: u32,
    global_limit: u32,
    window: Duration,
}

impl AiRateLimiter {
    pub fn new(per_user_limit: u32, global_limit: u32) -> Self {
        Self {
            user_windows: Mutex::new(dashmap::DashMap::new()),
            global_window: Mutex::new(VecDeque::new()),
            per_user_limit,
            global_limit,
            window: Duration::from_secs(60),
        }
    }

    /// Check if a request is allowed. Returns Ok(()) or Err with seconds until retry.
    pub async fn check(&self, user_id: i64) -> Result<(), u64> {
        let now = Instant::now();

        // Check global limit
        {
            let mut global = self.global_window.lock().await;
            // Remove expired entries
            while global
                .front()
                .is_some_and(|t| now.duration_since(*t) > self.window)
            {
                global.pop_front();
            }
            if global.len() >= self.global_limit as usize {
                let oldest = global.front().unwrap();
                let retry_after = self.window.as_secs()
                    - now
                        .duration_since(*oldest)
                        .as_secs()
                        .min(self.window.as_secs());
                return Err(retry_after.max(1));
            }
        }

        // Check per-user limit
        {
            let user_windows = self.user_windows.lock().await;
            let mut entry = user_windows.entry(user_id).or_insert_with(VecDeque::new);
            let window = entry.value_mut();

            // Remove expired entries
            while window
                .front()
                .is_some_and(|t| now.duration_since(*t) > self.window)
            {
                window.pop_front();
            }

            if window.len() >= self.per_user_limit as usize {
                let oldest = window.front().unwrap();
                let retry_after = self.window.as_secs()
                    - now
                        .duration_since(*oldest)
                        .as_secs()
                        .min(self.window.as_secs());
                return Err(retry_after.max(1));
            }

            // Record this request
            window.push_back(now);
        }

        // Record in global
        {
            let mut global = self.global_window.lock().await;
            global.push_back(now);
        }

        Ok(())
    }
}

/// The AI service client for Azure OpenAI.
pub struct AiService {
    pub config: AiServiceConfig,
    pub http: Client,
    pub rate_limiter: AiRateLimiter,
}

impl AiService {
    pub fn new(config: AiServiceConfig) -> Self {
        let rate_limiter = AiRateLimiter::new(
            config.rate_limit_per_minute,
            config.global_rate_limit_per_minute,
        );

        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build AI HTTP client");

        Self {
            config,
            http,
            rate_limiter,
        }
    }

    /// Check rate limit for a user. Returns Ok(()) or Err with retry_after seconds.
    pub async fn check_rate_limit(&self, user_id: i64) -> Result<(), u64> {
        self.rate_limiter.check(user_id).await
    }

    /// Generate issue content as a stream of chunks.
    ///
    /// The system_prompt should include the project context and output format constraints.
    /// The user_prompt is the user's input (already sanitized).
    ///
    /// Returns a stream of AiStreamChunk values.
    pub async fn generate_stream(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = AiStreamChunk> + Send>>, String> {
        let base = self.config.base_url.trim_end_matches('/');
        let is_openai_compatible = base.ends_with("/v1");

        let (url, auth_header_name, auth_header_value) = if is_openai_compatible {
            (
                format!("{}/chat/completions", base),
                "Authorization",
                format!("Bearer {}", self.config.api_key),
            )
        } else {
            (
                format!(
                    "{}/openai/deployments/{}/chat/completions?api-version=2024-02-01",
                    base, self.config.model
                ),
                "api-key",
                self.config.api_key.clone(),
            )
        };

        let model = if is_openai_compatible {
            Some(self.config.model.clone())
        } else {
            None
        };

        let request_body = ChatCompletionRequest {
            model,
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            token_limit: TokenLimit::for_family(self.config.model_family, self.config.max_tokens),
            sampling: SamplingOptions::for_family(self.config.model_family),
            stream: true,
        };

        let response = self
            .http
            .post(&url)
            .header(auth_header_name, &auth_header_value)
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(30))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "AI 服务响应超时，请重试".to_string()
                } else {
                    format!("AI 服务请求失败: {}", e)
                }
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "AI 服务返回错误 {}: {}",
                status,
                &body[..body.len().min(200)]
            ));
        }

        // Convert the response byte stream into an SSE chunk stream
        let byte_stream = response.bytes_stream();

        let stream = SseParser::new(byte_stream);

        Ok(Box::pin(stream))
    }

    /// Sanitize user input to prevent prompt injection.
    pub fn sanitize_input(input: &str) -> String {
        // Remove potential system/assistant role markers
        let sanitized = input
            .replace("<|im_start|>", "")
            .replace("<|im_end|>", "")
            .replace("<|im_sep|>", "");

        // Remove any attempts to inject role markers
        let sanitized = regex::Regex::new(r"(?i)(system|assistant|user)\s*:")
            .map(|re| re.replace_all(&sanitized, "[filtered]:").to_string())
            .unwrap_or(sanitized);

        sanitized
    }

    /// Build the system prompt for issue generation.
    pub fn build_system_prompt(workflow_content: Option<&str>) -> String {
        let template_section = if let Some(workflow) = workflow_content {
            format!(
                "\n\n## Project Issue Template\n\nFollow this template structure:\n\n{}",
                workflow
            )
        } else {
            String::new()
        };

        format!(
            r#"You are an AI assistant that generates structured Issue content for software projects.

## Output Format

Generate Issue content in Markdown format with the following sections:
1. ## 描述 - Clear description of the problem or feature
2. ## Acceptance Criteria - Checklist of requirements (use `- [ ]` format)
3. ## Validation - Commands to verify the implementation (use `- [ ]` format with backtick-wrapped commands)
4. ## Notes - Additional context or implementation hints

## Rules

- Output ONLY the Markdown content, no preamble or explanation
- Keep descriptions concise but complete
- Acceptance criteria should be testable and specific
- Validation commands must be safe, standard development commands
- Do not include any system commands, file deletion, or network operations outside of testing
- Maximum output length: 4096 tokens
- Write in the same language as the user's input{template_section}"#
        )
    }

    /// Validate that commands in the Validation section are in the whitelist.
    pub fn validate_commands(content: &str) -> Vec<String> {
        let allowed_prefixes = [
            "cargo test",
            "cargo build",
            "cargo clippy",
            "npm test",
            "npm run",
            "npx",
            "yarn test",
            "yarn run",
            "pnpm test",
            "pnpm run",
            "go test",
            "python -m pytest",
            "pytest",
            "make",
            "curl",
            "grep",
            "cat",
            "ls",
        ];

        let mut warnings = Vec::new();

        // Find commands in backticks within the Validation section
        let validation_start = content.find("## Validation");
        let validation_end = content[validation_start.unwrap_or(0)..]
            .find("\n## ")
            .map(|i| i + validation_start.unwrap_or(0))
            .unwrap_or(content.len());

        if let Some(start) = validation_start {
            let section = &content[start..validation_end];
            // Extract commands from backticks
            let re = regex::Regex::new(r"`([^`]+)`").unwrap();
            for cap in re.captures_iter(section) {
                let cmd = cap.get(1).unwrap().as_str().trim();
                let is_allowed = allowed_prefixes
                    .iter()
                    .any(|prefix| cmd.starts_with(prefix));
                if !is_allowed && !cmd.is_empty() {
                    warnings.push(format!("Unrecognized command: `{}`", cmd));
                }
            }
        }

        warnings
    }
}

// ==================== SSE Parser for Azure OpenAI streaming ====================

/// Parses an SSE byte stream from Azure OpenAI into AiStreamChunk items.
struct SseParser<S> {
    inner: S,
    buffer: String,
}

impl<S> SseParser<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: String::new(),
        }
    }
}

impl<S> Stream for SseParser<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send,
{
    type Item = AiStreamChunk;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // Try to extract a complete SSE event from the buffer
            if let Some(pos) = this.buffer.find("\n\n") {
                let event_str = this.buffer[..pos].to_string();
                this.buffer = this.buffer[pos + 2..].to_string();

                // Parse the SSE event
                if let Some(chunk) = parse_sse_event(&event_str) {
                    return std::task::Poll::Ready(Some(chunk));
                }
                // If parse returned None, continue to next event
                continue;
            }

            // Need more data from the inner stream
            match Pin::new(&mut this.inner).poll_next(cx) {
                std::task::Poll::Ready(Some(Ok(bytes))) => {
                    if let Ok(text) = std::str::from_utf8(&bytes) {
                        this.buffer.push_str(text);
                    }
                    // Loop back to try parsing
                }
                std::task::Poll::Ready(Some(Err(e))) => {
                    return std::task::Poll::Ready(Some(AiStreamChunk::Error(format!(
                        "Stream error: {}",
                        e
                    ))));
                }
                std::task::Poll::Ready(None) => {
                    // Stream ended
                    if this.buffer.trim().is_empty() {
                        return std::task::Poll::Ready(None);
                    }
                    // Try to parse remaining buffer
                    let remaining = std::mem::take(&mut this.buffer);
                    if let Some(chunk) = parse_sse_event(&remaining) {
                        return std::task::Poll::Ready(Some(chunk));
                    }
                    return std::task::Poll::Ready(None);
                }
                std::task::Poll::Pending => {
                    return std::task::Poll::Pending;
                }
            }
        }
    }
}

/// Parse a single SSE event string into an AiStreamChunk.
fn parse_sse_event(event: &str) -> Option<AiStreamChunk> {
    for line in event.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            let data = data.trim();

            // [DONE] marker
            if data == "[DONE]" {
                return Some(AiStreamChunk::Done);
            }

            // Parse the JSON chunk
            if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
                if let Some(choice) = chunk.choices.first() {
                    if let Some(ref content) = choice.delta.content {
                        if !content.is_empty() {
                            return Some(AiStreamChunk::Content(content.clone()));
                        }
                    }
                    // Check for finish_reason
                    if choice.finish_reason.is_some() {
                        return Some(AiStreamChunk::Done);
                    }
                }
            }
        }
    }
    None
}

// ==================== Request/Response types ====================

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    messages: Vec<ChatMessage>,
    #[serde(flatten)]
    token_limit: TokenLimit,
    #[serde(flatten)]
    sampling: SamplingOptions,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct TokenLimit {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
}

impl TokenLimit {
    fn for_family(model_family: AiModelFamily, value: u32) -> Self {
        match model_family {
            AiModelFamily::Gpt5 => Self {
                max_tokens: None,
                max_completion_tokens: Some(value),
            },
            AiModelFamily::Legacy => Self {
                max_tokens: Some(value),
                max_completion_tokens: None,
            },
        }
    }
}

fn model_supports_max_completion_tokens(model: &str) -> bool {
    let normalized = model.to_ascii_lowercase();
    normalized.starts_with("gpt-5")
        || normalized.starts_with("o1")
        || normalized.starts_with("o3")
        || normalized.starts_with("o4")
}

#[derive(Debug, Serialize)]
struct SamplingOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

impl SamplingOptions {
    fn for_family(model_family: AiModelFamily) -> Self {
        match model_family {
            AiModelFamily::Gpt5 => Self { temperature: None },
            AiModelFamily::Legacy => Self {
                temperature: Some(0.7),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChunkDelta {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::Value;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn gpt5_models_use_max_completion_tokens() {
        let captured = Arc::new(Mutex::new(None));
        let base_url = spawn_chat_completion_server(captured.clone()).await;
        let service = AiService::new(AiServiceConfig {
            base_url,
            api_key: "test-key".to_string(),
            model: "gpt-5.5".to_string(),
            model_family: AiModelFamily::Gpt5,
            max_tokens: 1234,
            rate_limit_per_minute: 10,
            global_rate_limit_per_minute: 30,
        });

        let _stream = service
            .generate_stream("system prompt", "user prompt")
            .await
            .expect("AI stream should start");

        let body = captured
            .lock()
            .await
            .clone()
            .expect("request body should be captured");
        assert_eq!(body["max_completion_tokens"], 1234);
        assert!(
            body.get("max_tokens").is_none(),
            "gpt-5 models must not send max_tokens"
        );
    }

    #[tokio::test]
    async fn gpt5_models_omit_temperature() {
        let captured = Arc::new(Mutex::new(None));
        let base_url = spawn_chat_completion_server(captured.clone()).await;
        let service = AiService::new(AiServiceConfig {
            base_url,
            api_key: "test-key".to_string(),
            model: "gpt-5.5".to_string(),
            model_family: AiModelFamily::Gpt5,
            max_tokens: 1234,
            rate_limit_per_minute: 10,
            global_rate_limit_per_minute: 30,
        });

        let _stream = service
            .generate_stream("system prompt", "user prompt")
            .await
            .expect("AI stream should start");

        let body = captured
            .lock()
            .await
            .clone()
            .expect("request body should be captured");
        assert!(
            body.get("temperature").is_none(),
            "gpt-5 models only support default temperature, so omit it"
        );
    }

    #[tokio::test]
    async fn legacy_models_use_max_tokens() {
        let captured = Arc::new(Mutex::new(None));
        let base_url = spawn_chat_completion_server(captured.clone()).await;
        let service = AiService::new(AiServiceConfig {
            base_url,
            api_key: "test-key".to_string(),
            model: "gpt-4.1".to_string(),
            model_family: AiModelFamily::Legacy,
            max_tokens: 1234,
            rate_limit_per_minute: 10,
            global_rate_limit_per_minute: 30,
        });

        let _stream = service
            .generate_stream("system prompt", "user prompt")
            .await
            .expect("AI stream should start");

        let body = captured
            .lock()
            .await
            .clone()
            .expect("request body should be captured");
        assert_eq!(body["max_tokens"], 1234);
        assert!(
            body.get("max_completion_tokens").is_none(),
            "legacy models should keep using max_tokens"
        );
        assert_eq!(body["temperature"], 0.7);
    }

    #[tokio::test]
    async fn azure_custom_deployments_can_be_marked_as_gpt5_family() {
        let captured = Arc::new(Mutex::new(None));
        let base_url =
            spawn_azure_chat_completion_server("issue-generator-prod", captured.clone()).await;
        let service = AiService::new(AiServiceConfig {
            base_url,
            api_key: "test-key".to_string(),
            model: "issue-generator-prod".to_string(),
            model_family: AiModelFamily::Gpt5,
            max_tokens: 1234,
            rate_limit_per_minute: 10,
            global_rate_limit_per_minute: 30,
        });

        let _stream = service
            .generate_stream("system prompt", "user prompt")
            .await
            .expect("AI stream should start");

        let body = captured
            .lock()
            .await
            .clone()
            .expect("request body should be captured");
        assert_eq!(body["max_completion_tokens"], 1234);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("temperature").is_none());
        assert!(
            body.get("model").is_none(),
            "Azure deployment URL should not send model"
        );
    }

    async fn spawn_chat_completion_server(captured: Arc<Mutex<Option<Value>>>) -> String {
        async fn handler(
            State(captured): State<Arc<Mutex<Option<Value>>>>,
            Json(body): Json<Value>,
        ) -> &'static str {
            *captured.lock().await = Some(body);
            "data: [DONE]\n\n"
        }

        let app = Router::new()
            .route("/v1/chat/completions", post(handler))
            .with_state(captured);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{}/v1", addr)
    }

    async fn spawn_azure_chat_completion_server(
        deployment: &str,
        captured: Arc<Mutex<Option<Value>>>,
    ) -> String {
        async fn handler(
            State(captured): State<Arc<Mutex<Option<Value>>>>,
            Json(body): Json<Value>,
        ) -> &'static str {
            *captured.lock().await = Some(body);
            "data: [DONE]\n\n"
        }

        let path = format!("/openai/deployments/{}/chat/completions", deployment);
        let app = Router::new()
            .route(&path, post(handler))
            .with_state(captured);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{}", addr)
    }
}
