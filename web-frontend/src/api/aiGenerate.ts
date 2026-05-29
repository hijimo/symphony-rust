import type { AIGenerateRequest, SSEEvent } from '../types/issue';

/**
 * AI-assisted issue content generation via SSE stream.
 * Uses raw fetch() instead of Axios because Axios doesn't support streaming.
 */
export async function aiGenerateIssue(
  projectId: number,
  data: AIGenerateRequest,
  callbacks: {
    onChunk: (content: string) => void;
    onDone: (fullContent: string, title?: string) => void;
    onError: (error: string, retCode?: string) => void;
  },
  signal?: AbortSignal,
): Promise<void> {
  const token = localStorage.getItem('token');

  const response = await fetch(`/api/projects/${projectId}/issues/ai-generate`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify(data),
    signal,
  });

  // Non-SSE error responses (401, 429, etc.) return JSON
  if (!response.ok) {
    const contentType = response.headers.get('content-type') || '';
    if (contentType.includes('application/json')) {
      const errorBody = await response.json();
      callbacks.onError(
        errorBody.retMsg || '请求失败',
        errorBody.retCode,
      );
    } else {
      callbacks.onError(`HTTP ${response.status}: 请求失败`);
    }
    return;
  }

  const reader = response.body?.getReader();
  if (!reader) {
    callbacks.onError('浏览器不支持流式读取');
    return;
  }

  const decoder = new TextDecoder();
  let buffer = '';

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      // Keep the last incomplete line in the buffer
      buffer = lines.pop() || '';

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed || trimmed.startsWith(':')) {
          // Empty line or SSE comment (keepalive), skip
          continue;
        }
        if (trimmed.startsWith('data: ')) {
          const jsonStr = trimmed.slice(6);
          try {
            const event: SSEEvent = JSON.parse(jsonStr);
            switch (event.type) {
              case 'chunk':
                callbacks.onChunk(event.content);
                break;
              case 'done':
                callbacks.onDone(event.content, event.title);
                return;
              case 'error':
                callbacks.onError(event.error, event.retCode);
                return;
            }
          } catch {
            // Malformed JSON line, skip
          }
        }
      }
    }
  } catch (err: unknown) {
    if (err instanceof DOMException && err.name === 'AbortError') {
      // User cancelled, not an error
      return;
    }
    callbacks.onError(
      err instanceof Error ? err.message : '流式读取异常',
    );
  } finally {
    reader.releaseLock();
  }
}
