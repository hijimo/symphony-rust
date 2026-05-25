# 测试规范

## 测试分层

| 层级 | 范围 | 工具 | 位置 |
|------|------|------|------|
| 单元测试 | 单个函数/模块 | `#[cfg(test)]`、`mockall` | 源码文件内 `mod tests` |
| 集成测试 | 多模块协作、HTTP API | `wiremock`、`axum::test` | `tests/` 目录 |
| E2E 测试 | 完整流程（含子进程） | `tokio::test`、真实进程 | `tests/e2e_*.rs` |
| 前端单元测试 | 组件、工具函数 | Vitest | `src/**/*.test.ts(x)` |
| 前端组件测试 | React 组件渲染与交互 | React Testing Library | `src/**/*.test.tsx` |
| 前端 E2E 测试 | 完整用户流程 | Playwright | `e2e/` 目录 |

---

## Rust 测试规范

### 基本结构

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_state_converts_spaces_to_underscores() {
        assert_eq!(normalize_state("In Progress"), "in_progress");
        assert_eq!(normalize_state("In-Progress"), "in_progress");
        assert_eq!(normalize_state(" Todo "), "todo");
    }

    #[tokio::test]
    async fn test_async_operation() {
        // 异步测试使用 tokio::test
    }
}
```

### HTTP Mock（wiremock）

使用 `wiremock` mock 外部 HTTP 服务（GitLab/GitHub API）：

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn test_gitlab_api_call() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/projects/123/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([...])))
        .mount(&mock_server)
        .await;

    // 使用 mock_server.uri() 作为 GitLab host
}
```

### Trait Mock（mockall）

使用 `mockall` mock Trait 实现：

```rust
use mockall::mock;

mock! {
    TrackerAdapter {}
    impl TrackerTrait for TrackerAdapter {
        async fn list_issues(&self) -> Result<Vec<Issue>, TrackerError>;
    }
}

#[tokio::test]
async fn test_reconciler_with_mock_tracker() {
    let mut mock = MockTrackerAdapter::new();
    mock.expect_list_issues()
        .returning(|| Ok(vec![/* test issues */]));
    // ...
}
```

### 临时目录（tempfile）

```rust
use tempfile::tempdir;

#[test]
fn test_workspace_creation() {
    let dir = tempdir().unwrap();
    // 使用 dir.path() 作为临时工作区
    // dir 在 drop 时自动清理
}
```

### 异步测试

```rust
#[tokio::test]
async fn test_codex_client_timeout() {
    // 使用 tokio::time::pause() 控制时间
    tokio::time::pause();
    // ...
    tokio::time::advance(Duration::from_secs(10)).await;
}
```

---

## 测试编写要求（关键规则）

### 不直接构造理想化的 state 字符串

**错误做法**：直接传入已归一化的 state 字符串，绕过真实数据流路径。

```rust
// 错误：直接构造理想化 state
let issue = Issue {
    state: "in_progress".to_string(), // 直接用 state_key
    // ...
};
assert!(reconciler.is_active(&issue, &config));
```

**正确做法**：模拟真实数据流路径，从 platform adapter 返回 state_key，再经过 reconciler 判断。

```rust
// 正确：模拟真实数据流
// active_states 用原始形式（来自 WORKFLOW.md 配置）
let config = ServiceConfig {
    active_states: vec!["In Progress".to_string(), "Todo".to_string()],
    // ...
};

// tracker 返回 state_key 形式（来自 GitLab adapter）
let issue = Issue {
    state: "in_progress".to_string(), // GitlabTrackerAdapter 返回的形式
    // ...
};

// reconciler 使用 normalize_state 比较，应该匹配
assert!(reconciler.is_active(&issue, &config));
```

### 加入交叉匹配用例

测试 `active_states` 用原始形式、`refreshed_states` 用 state_key 形式的交叉场景，验证归一化后能正确匹配：

```rust
#[test]
fn test_state_normalization_cross_match() {
    // active_states 来自配置（原始形式）
    let active_states = vec!["In Progress", "To Do"];

    // tracker 返回的 state_key 形式
    let tracker_states = vec!["in_progress", "to_do"];

    for tracker_state in &tracker_states {
        let normalized = normalize_state(tracker_state);
        let matched = active_states.iter()
            .any(|s| normalize_state(s) == normalized);
        assert!(matched, "state '{}' should match active_states", tracker_state);
    }
}
```

### 考虑 stdio pipe buffer 影响

子进程相关测试不能假设 stderr 永远为空：

```rust
#[tokio::test]
async fn test_subprocess_with_stderr_output() {
    // 测试子进程产生大量 stderr 输出时不会 deadlock
    // 验证 stderr drain 后台任务正常工作
    let mut child = Command::new("bash")
        .args(["-c", "for i in $(seq 1 10000); do echo 'error line' >&2; done; echo 'done'"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // 必须 drain stderr，否则会 deadlock
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut buf = String::new();
            while reader.read_line(&mut buf).await.unwrap_or(0) > 0 {
                buf.clear();
            }
        });
    }

    let output = child.wait_with_output().await.unwrap();
    assert!(output.status.success());
}
```

---

## 前端测试规范

### Vitest 单元测试

```typescript
import { describe, it, expect } from 'vitest'
import { normalizeState } from '../utils/state'

describe('normalizeState', () => {
  it('converts spaces to underscores', () => {
    expect(normalizeState('In Progress')).toBe('in_progress')
  })
})
```

### React Testing Library 组件测试

```typescript
import { render, screen, fireEvent } from '@testing-library/react'
import { ProjectCard } from './ProjectCard'

describe('ProjectCard', () => {
  it('displays project name', () => {
    render(<ProjectCard name="My Project" status="running" />)
    expect(screen.getByText('My Project')).toBeInTheDocument()
  })

  it('calls onStart when start button clicked', async () => {
    const onStart = vi.fn()
    render(<ProjectCard name="Test" status="stopped" onStart={onStart} />)
    fireEvent.click(screen.getByRole('button', { name: /start/i }))
    expect(onStart).toHaveBeenCalledOnce()
  })
})
```

### MSW API Mock

使用 Mock Service Worker mock API 请求：

```typescript
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'

const server = setupServer(
  http.get('/api/projects', () => {
    return HttpResponse.json({
      data: { records: [...], totalCount: 1 },
      success: true,
      retCode: '0',
      retMsg: 'ok'
    })
  })
)

beforeAll(() => server.listen())
afterEach(() => server.resetHandlers())
afterAll(() => server.close())
```

### Playwright E2E 测试

```typescript
import { test, expect } from '@playwright/test'

test('user can login and view projects', async ({ page }) => {
  await page.goto('/login')
  await page.fill('[name=username]', 'admin')
  await page.fill('[name=password]', 'password')
  await page.click('button[type=submit]')
  await expect(page).toHaveURL('/projects')
  await expect(page.locator('h1')).toContainText('项目')
})
```

---

## 覆盖率要求

- **Rust 核心逻辑**（reconciler、orchestrator、state 归一化）：目标覆盖率 ≥ 80%
- **HTTP handlers**：关键路径（认证、权限、错误处理）必须有集成测试覆盖
- **前端组件**：核心业务组件（ProjectCard、KanbanBoard 等）必须有组件测试

---

## 测试命名规范

### Rust

- 测试函数名使用 `test_` 前缀 + 描述性名称
- 格式：`test_{被测函数/场景}_{预期结果}`
- 示例：`test_normalize_state_converts_spaces_to_underscores`、`test_reconciler_skips_terminal_issues`

### TypeScript/前端

- 使用 `describe` 分组，`it` 描述具体行为
- 格式：`it('does something when condition')` 或 `it('should do something')`
- 示例：`it('displays error message when login fails')`

### 测试文件命名

- Rust 集成测试：`tests/{feature}_test.rs` 或 `tests/api_{resource}.rs`
- 前端单元测试：`{Component}.test.tsx` 或 `{util}.test.ts`
- Playwright E2E：`{flow}.spec.ts`
