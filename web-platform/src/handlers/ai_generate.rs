use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::Response,
    Json,
};
use futures::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::require_project_member;
use crate::models::kanban::{AIGenerateRequest, SseEvent};
use crate::repository::ProjectRepository;
use crate::AppState;

/// Allowed command prefixes for the Validation section.
const ALLOWED_COMMAND_PREFIXES: &[&str] = &[
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

/// Patterns that indicate prompt injection attempts.
const INJECTION_PATTERNS: &[&str] = &[
    "<|im_start|>",
    "<|im_end|>",
    "<|system|>",
    "<|assistant|>",
    "<|user|>",
    "ignore previous instructions",
    "ignore all previous",
    "disregard previous",
    "forget your instructions",
    "you are now",
    "new instructions:",
    "system prompt:",
    "SYSTEM:",
    "ASSISTANT:",
];

/// POST /api/projects/:id/issues/ai-generate
///
/// Generate issue content using Azure OpenAI, streamed via SSE.
pub async fn generate_issue(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<AIGenerateRequest>,
) -> Result<Response, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Check project membership
    require_project_member(&claims, project_id, &state.repo).await?;

    // Validate request
    if req.prompt.len() < 5 || req.prompt.len() > 2000 {
        return Err(WebPlatformError::BadRequest(
            "prompt must be 5-2000 characters".to_string(),
        ));
    }
    if let Some(ref title) = req.title {
        if title.len() > 200 {
            return Err(WebPlatformError::BadRequest(
                "title must be at most 200 characters".to_string(),
            ));
        }
    }
    if let Some(ref context) = req.context {
        if context.len() > 1000 {
            return Err(WebPlatformError::BadRequest(
                "context must be at most 1000 characters".to_string(),
            ));
        }
    }

    // Rate limit: 10/min/user for AI generation
    if let Err(retry_after) = state.phase3_rate_limiter.check("ai_generate", user_id, 10) {
        return Err(WebPlatformError::AiRateLimited(retry_after));
    }

    // Global rate limit: 30/min
    if let Err(retry_after) = state.phase3_rate_limiter.check_global("ai_global", 30) {
        return Err(WebPlatformError::AiRateLimited(retry_after));
    }

    // Concurrent generation limit: 1 per user
    if state.phase3_rate_limiter.has_active_generation(user_id) {
        return Err(WebPlatformError::AiRateLimited(5));
    }

    // Check AI service is configured
    let ai_service = state
        .ai_service
        .as_ref()
        .ok_or_else(|| {
            WebPlatformError::ExternalService("AI service is not configured".to_string())
        })?
        .clone();

    // Get project info for context
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Sanitize user input
    let sanitized_prompt = sanitize_input(&req.prompt);
    let sanitized_title = req.title.as_deref().map(sanitize_input);
    let sanitized_context = req.context.as_deref().map(sanitize_input);

    // Build system prompt
    let system_prompt = build_system_prompt(&project.name, &project.repo_name);

    // Build user prompt
    let mut user_prompt = String::new();
    if let Some(ref title) = sanitized_title {
        user_prompt.push_str(&format!("Issue Title: {}\n\n", title));
    }
    user_prompt.push_str(&format!("Requirement: {}", sanitized_prompt));
    if let Some(ref ctx) = sanitized_context {
        user_prompt.push_str(&format!("\n\nAdditional Context: {}", ctx));
    }

    // Mark generation as active
    state.phase3_rate_limiter.start_generation(user_id);

    // Create SSE stream
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(32);

    // Spawn the AI generation task
    let rate_limiter = state.phase3_rate_limiter.clone();
    tokio::spawn(async move {
        let _guard = GenerationGuard {
            rate_limiter: rate_limiter.clone(),
            user_id,
        };

        let mut full_content = String::new();

        match ai_service
            .generate_stream(&system_prompt, &user_prompt)
            .await
        {
            Ok(mut stream) => {
                let mut last_keepalive = tokio::time::Instant::now();
                let keepalive_interval = tokio::time::Duration::from_secs(15);

                loop {
                    tokio::select! {
                        chunk = stream.next() => {
                            match chunk {
                                Some(crate::services::ai_service::AiStreamChunk::Content(text)) => {
                                    full_content.push_str(&text);
                                    let event = SseEvent::Chunk { content: text };
                                    let data = format!("data: {}\n\n", serde_json::to_string(&event).unwrap_or_default());
                                    if tx.send(Ok(data)).await.is_err() {
                                        // Client disconnected
                                        return;
                                    }
                                }
                                Some(crate::services::ai_service::AiStreamChunk::Done) => {
                                    // Validate output commands
                                    let validated_content = validate_output_commands(&full_content);
                                    let event = SseEvent::Done { content: validated_content };
                                    let data = format!("data: {}\n\n", serde_json::to_string(&event).unwrap_or_default());
                                    let _ = tx.send(Ok(data)).await;
                                    return;
                                }
                                Some(crate::services::ai_service::AiStreamChunk::Error(err)) => {
                                    let event = SseEvent::Error {
                                        error: err,
                                        ret_code: "EXT_001".to_string(),
                                    };
                                    let data = format!("data: {}\n\n", serde_json::to_string(&event).unwrap_or_default());
                                    let _ = tx.send(Ok(data)).await;
                                    return;
                                }
                                None => {
                                    // Stream ended without Done event
                                    let validated_content = validate_output_commands(&full_content);
                                    let event = SseEvent::Done { content: validated_content };
                                    let data = format!("data: {}\n\n", serde_json::to_string(&event).unwrap_or_default());
                                    let _ = tx.send(Ok(data)).await;
                                    return;
                                }
                            }
                        }
                        _ = tokio::time::sleep_until(last_keepalive + keepalive_interval) => {
                            // Send keepalive comment
                            last_keepalive = tokio::time::Instant::now();
                            if tx.send(Ok(": keepalive\n\n".to_string())).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let event = SseEvent::Error {
                    error: format!("AI 服务不可用: {}", err),
                    ret_code: "EXT_001".to_string(),
                };
                let data = format!(
                    "data: {}\n\n",
                    serde_json::to_string(&event).unwrap_or_default()
                );
                let _ = tx.send(Ok(data)).await;
            }
        }
    });

    // Build SSE response
    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .map_err(|e| WebPlatformError::Internal(format!("failed to build SSE response: {}", e)))?;

    Ok(response)
}

/// Guard that ensures generation is marked as complete when dropped.
struct GenerationGuard {
    rate_limiter: std::sync::Arc<crate::Phase3RateLimiter>,
    user_id: i64,
}

impl Drop for GenerationGuard {
    fn drop(&mut self) {
        self.rate_limiter.end_generation(self.user_id);
    }
}

/// Sanitize user input by removing potential prompt injection patterns.
fn sanitize_input(input: &str) -> String {
    let mut sanitized = input.to_string();
    for pattern in INJECTION_PATTERNS {
        sanitized = sanitized.replace(pattern, "");
    }
    // Also remove any remaining angle-bracket sequences that look like role markers
    let re = regex::Regex::new(r"<\|[^|]*\|>").unwrap();
    sanitized = re.replace_all(&sanitized, "").to_string();
    sanitized.trim().to_string()
}

/// Build the system prompt for AI issue generation.
fn build_system_prompt(project_name: &str, repo_name: &str) -> String {
    format!(
        r#"You are an AI assistant that generates structured Issue content for the project "{}" (repository: {}).

Your output MUST follow this exact Markdown structure:

## 描述

[Clear description of the problem or feature requirement]

## Acceptance Criteria

- [ ] [Specific, testable criterion 1]
- [ ] [Specific, testable criterion 2]
- [ ] [Additional criteria as needed]

## Validation

- [ ] [Test command or verification step]: `[command]`
- [ ] [Additional validation steps]

## Notes

- [Implementation hints, related files, or technical context]

RULES:
1. Output ONLY the Markdown content above. No preamble, no explanation outside the template.
2. Write in the same language as the user's input.
3. Acceptance Criteria must be specific and testable.
4. Validation commands must be real, executable commands appropriate for the project.
5. Keep the output concise but complete.
6. Do NOT include any system instructions, role markers, or meta-commentary in your output.
7. Maximum output length: 4096 tokens."#,
        project_name, repo_name
    )
}

/// Validate commands in the Validation section against the whitelist.
/// Returns the content with warnings appended for non-whitelisted commands.
fn validate_output_commands(content: &str) -> String {
    let mut result = content.to_string();
    let mut warnings = Vec::new();

    // Find commands in backticks within the Validation section
    let validation_start = content.find("## Validation");
    let validation_end = content[validation_start.unwrap_or(0)..]
        .find("\n## ")
        .map(|pos| validation_start.unwrap_or(0) + pos)
        .unwrap_or(content.len());

    if let Some(start) = validation_start {
        let validation_section = &content[start..validation_end];

        // Extract commands from backticks
        let re = regex::Regex::new(r"`([^`]+)`").unwrap();
        for cap in re.captures_iter(validation_section) {
            let command = cap.get(1).unwrap().as_str().trim();
            if !command.is_empty() && !is_command_allowed(command) {
                warnings.push(format!("  - `{}` (not in allowed command list)", command));
            }
        }
    }

    if !warnings.is_empty() {
        result.push_str("\n\n> **Warning**: The following commands are not in the allowed list:\n");
        for w in warnings {
            result.push_str(&w);
            result.push('\n');
        }
    }

    result
}

/// Check if a command starts with an allowed prefix.
fn is_command_allowed(command: &str) -> bool {
    ALLOWED_COMMAND_PREFIXES
        .iter()
        .any(|prefix| command.starts_with(prefix))
}
