# Phase 5 后端测试框架设计

## 概述

本文档定义 Phase 5（告警与通知）后端的完整测试策略，覆盖单元测试、集成测试、API 接口测试和 E2E 测试。

## GitLab CI 信息

- GitLab URL: http://gitlab.jushuitan-inc.com:8081/
- 项目: /zimei10525/symphony_e2e_test_repo
- GITLAB_TOKEN: gitlab-token-example

---

## 1. 单元测试

### 1.1 目录结构与约定

```
web-platform/src/
├── alert/
│   ├── mod.rs
│   ├── metrics.rs       # 内含 #[cfg(test)] mod tests
│   ├── rules.rs         # 内含 #[cfg(test)] mod tests
│   └── engine.rs        # 内含 #[cfg(test)] mod tests
├── notification/
│   ├── mod.rs
│   ├── channel.rs
│   ├── dingtalk.rs      # 内含 #[cfg(test)] mod tests
│   └── dispatcher.rs    # 内含 #[cfg(test)] mod tests
└── handlers/
    └── alerts.rs
```

约定：
- 每个模块文件底部添加 `#[cfg(test)] mod tests { ... }`
- 测试函数命名：`test_<功能>_<场景>_<预期结果>`
- 使用 `mockall` crate mock repository traits

### 1.2 MetricCollector 单元测试 (`alert/metrics.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_snapshot_empty_state() {
        let collector = DefaultMetricCollector::new_for_test();
        let snapshot = collector.collect_sync();
        assert!(snapshot.running_projects.is_empty());
        assert_eq!(snapshot.global_concurrency.active_agents, 0);
    }

    #[test]
    fn test_record_api_failure_increments() {
        let collector = DefaultMetricCollector::new_for_test();
        collector.record_api_failure("gitlab");
        collector.record_api_failure("gitlab");
        assert_eq!(collector.get_api_failures("gitlab"), 2);
    }

    #[test]
    fn test_record_service_crash() {
        let collector = DefaultMetricCollector::new_for_test();
        collector.record_service_crash(1, -1);
        let crashes = collector.drain_crash_events();
        assert_eq!(crashes.len(), 1);
        assert_eq!(crashes[0].project_id, 1);
    }

    #[test]
    fn test_reset_api_failures() {
        let collector = DefaultMetricCollector::new_for_test();
        collector.record_api_failure("github");
        collector.record_api_failure("github");
        collector.reset_api_failures("github");
        assert_eq!(collector.get_api_failures("github"), 0);
    }
}
```

### 1.3 RuleEvaluator 单元测试 (`alert/rules.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_task_timeout_fires_when_exceeded() {
        let rule = make_rule("task_timeout", 30); // 30 min threshold
        let metric = RunningProjectMetric {
            project_id: 1,
            started_at: Utc::now() - chrono::Duration::minutes(35),
            ..Default::default()
        };
        let result = evaluate_task_timeout(&rule, &[metric]);
        assert!(result.is_some());
    }

    #[test]
    fn test_task_timeout_no_fire_within_threshold() {
        let rule = make_rule("task_timeout", 30);
        let metric = RunningProjectMetric {
            project_id: 1,
            started_at: Utc::now() - chrono::Duration::minutes(10),
            ..Default::default()
        };
        let result = evaluate_task_timeout(&rule, &[metric]);
        assert!(result.is_none());
    }

    #[test]
    fn test_service_crash_fires_on_unexpected_exit() {
        let rule = make_rule("service_crash", 1);
        let crash = CrashEvent { project_id: 1, exit_code: -1 };
        let result = evaluate_service_crash(&rule, &[crash]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().severity, Severity::Critical);
    }

    #[test]
    fn test_concurrency_saturation_fires_after_duration() {
        let rule = make_rule("concurrency_saturation", 10); // 10 min
        let metric = ConcurrencyMetric {
            active_agents: 5,
            max_agents: 5,
            saturation_since: Some(Utc::now() - chrono::Duration::minutes(15)),
        };
        let result = evaluate_concurrency_saturation(&rule, &metric);
        assert!(result.is_some());
    }

    #[test]
    fn test_concurrency_saturation_no_fire_below_duration() {
        let rule = make_rule("concurrency_saturation", 10);
        let metric = ConcurrencyMetric {
            active_agents: 5,
            max_agents: 5,
            saturation_since: Some(Utc::now() - chrono::Duration::minutes(3)),
        };
        let result = evaluate_concurrency_saturation(&rule, &metric);
        assert!(result.is_none());
    }

    #[test]
    fn test_consecutive_failures_fires_at_threshold() {
        let rule = make_rule("consecutive_failures", 3);
        let failures = vec![(1_i64, 3_u32)]; // project 1 has 3 failures
        let result = evaluate_consecutive_failures(&rule, &failures);
        assert!(result.is_some());
    }

    #[test]
    fn test_consecutive_failures_no_fire_below_threshold() {
        let rule = make_rule("consecutive_failures", 3);
        let failures = vec![(1_i64, 2_u32)];
        let result = evaluate_consecutive_failures(&rule, &failures);
        assert!(result.is_none());
    }

    #[test]
    fn test_api_unreachable_fires_at_threshold() {
        let rule = make_rule("api_unreachable", 5);
        let api_health = HashMap::from([("gitlab".to_string(), 5_u32)]);
        let result = evaluate_api_unreachable(&rule, &api_health);
        assert!(result.is_some());
    }

    #[test]
    fn test_cooldown_prevents_duplicate_fire() {
        let manager = CooldownManager::new();
        manager.mark_fired("task_timeout", "project:1", 300);
        assert!(manager.is_in_cooldown("task_timeout", "project:1"));
    }

    #[test]
    fn test_cooldown_expires_allows_refire() {
        let manager = CooldownManager::new();
        manager.mark_fired_with_expiry("task_timeout", "project:1",
            Utc::now() - chrono::Duration::seconds(1));
        assert!(!manager.is_in_cooldown("task_timeout", "project:1"));
    }
}
```

### 1.4 DingTalk 通知单元测试 (`notification/dingtalk.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dingtalk_sign_generation() {
        let secret = "SEC1234567890";
        let timestamp = 1609459200000_i64; // fixed timestamp
        let sign = compute_sign(secret, timestamp);
        // Verify it's a valid base64 + URL-encoded string
        assert!(!sign.is_empty());
        assert!(sign.contains('%') || sign.chars().all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '='));
    }

    #[test]
    fn test_dingtalk_message_format() {
        let event = AlertEvent {
            rule_id: "task_timeout".to_string(),
            severity: Severity::Warning,
            project_id: Some(1),
            project_name: Some("my-project".to_string()),
            title: "任务超时告警".to_string(),
            message: "Issue #42 运行时间超过 30 分钟".to_string(),
            ..Default::default()
        };
        let msg = format_dingtalk_message(&event);
        assert!(msg.contains("任务超时告警"));
        assert!(msg.contains("my-project"));
        assert!(msg.contains("warning"));
    }

    #[test]
    fn test_dingtalk_message_format_without_project() {
        let event = AlertEvent {
            rule_id: "api_unreachable".to_string(),
            severity: Severity::Critical,
            project_id: None,
            project_name: None,
            title: "API 不可达".to_string(),
            message: "GitLab API 连续 5 次请求失败".to_string(),
            ..Default::default()
        };
        let msg = format_dingtalk_message(&event);
        assert!(msg.contains("API 不可达"));
        assert!(!msg.contains("项目"));
    }
}
```

---

## 2. 集成测试

### 2.1 AlertRepository 集成测试

文件：`web-platform/tests/phase5_alerts_api.rs`

```rust
#[tokio::test]
async fn test_alert_rules_crud() {
    let repo = setup_test_db().await;
    // 验证默认 6 条规则
    let rules = repo.list_alert_rules().await.unwrap();
    assert_eq!(rules.len(), 6);

    // 更新规则
    repo.update_alert_rule("task_timeout", Some(true), Some(45), Some(600)).await.unwrap();
    let updated = repo.get_alert_rule("task_timeout").await.unwrap().unwrap();
    assert_eq!(updated.threshold_value, 45);
    assert_eq!(updated.cooldown_seconds, 600);
}

#[tokio::test]
async fn test_notification_channels_crud() {
    let repo = setup_test_db().await;
    // 初始为空
    let channels = repo.list_notification_channels().await.unwrap();
    assert!(channels.is_empty());

    // 添加钉钉渠道
    repo.upsert_notification_channel(&channel_config).await.unwrap();
    let channels = repo.list_notification_channels().await.unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].channel_type, "dingtalk");
}

#[tokio::test]
async fn test_alert_history_query_with_filters() {
    let repo = setup_test_db().await;
    // 插入测试数据
    insert_test_alerts(&repo, 20).await;

    // 按严重级别筛选
    let (records, total) = repo.query_alert_history(&AlertHistoryQuery {
        severity: Some("critical".to_string()),
        page_no: 1, page_size: 10,
        ..Default::default()
    }).await.unwrap();
    assert!(records.iter().all(|r| r.severity == "critical"));

    // 分页
    let (page1, total) = repo.query_alert_history(&AlertHistoryQuery {
        page_no: 1, page_size: 5,
        ..Default::default()
    }).await.unwrap();
    assert_eq!(page1.len(), 5);
    assert_eq!(total, 20);
}

#[tokio::test]
async fn test_cooldown_persistence() {
    let repo = setup_test_db().await;
    repo.set_cooldown("task_timeout", "project:1", &now, &expires).await.unwrap();
    let cooldown = repo.get_cooldown("task_timeout", "project:1").await.unwrap();
    assert!(cooldown.is_some());
}
```

### 2.2 告警生命周期集成测试

```rust
#[tokio::test]
async fn test_alert_lifecycle_fire_notify_resolve() {
    // 1. 设置规则 + 渠道
    // 2. 触发告警条件
    // 3. 验证 alert_history 记录已创建
    // 4. 验证通知状态为 sent/failed
    // 5. 验证冷却期内不重复触发
}
```

---

## 3. API 接口测试

### 3.1 测试辅助函数

```rust
// tests/common/mod.rs
pub async fn setup_test_app() -> (Router, TestState) { ... }
pub fn admin_token() -> String { ... }
pub fn user_token() -> String { ... }
pub fn no_token() -> &'static str { "" }
```

### 3.2 GET /api/admin/alerts

| 测试用例 | 预期 |
|---------|------|
| `test_list_alerts_empty` | 200, records=[], totalCount=0 |
| `test_list_alerts_with_data` | 200, 分页正确 |
| `test_list_alerts_filter_by_severity` | 200, 仅返回匹配 severity |
| `test_list_alerts_filter_by_project` | 200, 仅返回匹配 project_id |
| `test_list_alerts_filter_by_date_range` | 200, 仅返回时间范围内 |
| `test_list_alerts_requires_auth` | 401, AUTH_001 |
| `test_list_alerts_requires_admin` | 403, AUTH_002 |
| `test_list_alerts_invalid_page` | 200, 使用默认分页 |

### 3.3 GET /api/admin/alerts/rules

| 测试用例 | 预期 |
|---------|------|
| `test_get_rules_returns_defaults` | 200, 6 条默认规则 |
| `test_get_rules_requires_admin` | 403 |
| `test_get_rules_requires_auth` | 401 |

### 3.4 PUT /api/admin/alerts/rules

| 测试用例 | 预期 |
|---------|------|
| `test_update_rules_enable_disable` | 200, enabled 状态变更 |
| `test_update_rules_change_threshold` | 200, threshold 更新 |
| `test_update_rules_change_cooldown` | 200, cooldown 更新 |
| `test_update_rules_invalid_rule_id` | 404, ALERT_001 |
| `test_update_rules_negative_threshold` | 400, BIZ_001 |
| `test_update_rules_requires_admin` | 403 |

### 3.5 GET /api/admin/alerts/channels

| 测试用例 | 预期 |
|---------|------|
| `test_get_channels_empty` | 200, channels=[] |
| `test_get_channels_with_config` | 200, 返回渠道列表 |
| `test_get_channels_masks_secrets` | 200, secret 字段被遮蔽 |
| `test_get_channels_requires_admin` | 403 |

### 3.6 PUT /api/admin/alerts/channels

| 测试用例 | 预期 |
|---------|------|
| `test_update_channels_add_dingtalk` | 200, 新增钉钉渠道 |
| `test_update_channels_update_existing` | 200, 更新已有渠道 |
| `test_update_channels_invalid_webhook_url` | 400, ALERT_002 |
| `test_update_channels_empty_name` | 400, BIZ_001 |
| `test_update_channels_requires_admin` | 403 |

### 3.7 POST /api/admin/alerts/test

| 测试用例 | 预期 |
|---------|------|
| `test_notification_success` | 200, success=true |
| `test_notification_invalid_channel` | 404, ALERT_001 |
| `test_notification_requires_channel_id` | 400, BIZ_001 |
| `test_notification_channel_unreachable` | 502, ALERT_003 |
| `test_notification_requires_admin` | 403 |

---

## 4. E2E 测试

### 4.1 完整告警流程 E2E

```rust
#[tokio::test]
async fn test_e2e_alert_flow() {
    // 1. 启动完整应用（含 AlertEngine）
    // 2. 配置钉钉渠道（使用 mock HTTP server）
    // 3. 启用 task_timeout 规则（阈值设为 1 秒方便测试）
    // 4. 模拟任务超时条件
    // 5. 等待 AlertEngine tick
    // 6. 验证 mock server 收到钉钉通知
    // 7. 验证 alert_history 记录
    // 8. 验证冷却期内不重复发送
}
```

### 4.2 Mock DingTalk Server

```rust
async fn start_mock_dingtalk_server() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let app = Router::new().route("/robot/send", post(move |body: Json<Value>| {
        received_clone.lock().unwrap().push(body.0);
        Json(json!({"errcode": 0, "errmsg": "ok"}))
    }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());

    (format!("http://{}/robot/send", addr), received)
}
```

---

## 5. CI 集成

```yaml
# .gitlab-ci.yml
test-phase5-backend:
  stage: test
  script:
    - cargo test -p web-platform --lib -- alert:: notification::
    - cargo test -p web-platform --test phase5_alerts_api
  variables:
    DATABASE_URL: "sqlite://:memory:"
    JWT_SECRET: "test-secret-at-least-32-characters-long"
    ENCRYPTION_KEY: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```
