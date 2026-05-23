# Web Platform Backend Code Review

审查范围: `web-platform/src/` 及 `web-platform/tests/` 全部源文件

---

## Critical Issues

### 1. 硬编码默认管理员密码

- **文件**: `src/main.rs` 第 84 行
- **严重性**: Critical
- **描述**: `seed_admin` 函数使用硬编码密码 `"admin123"` 创建默认管理员账户。如果部署后未立即修改密码，攻击者可直接登录获取管理员权限。虽然日志中有提示修改密码，但没有强制机制。
- **建议修复**: 从环境变量读取初始管理员密码（如 `ADMIN_INITIAL_PASSWORD`），若未设置则拒绝启动或生成随机密码并输出到日志。

### 2. CORS 配置过于宽松

- **文件**: `src/main.rs` 第 59 行
- **严重性**: Critical
- **描述**: `CorsLayer::permissive()` 允许任意来源、任意方法、任意头部的跨域请求。这使得任何恶意网站都可以向 API 发起经过认证的请求（如果用户浏览器中存有 token）。
- **建议修复**: 配置明确的 allowed origins 列表，从环境变量或配置文件读取。至少限制为前端部署的域名。

---

## High Issues

### 3. 同步阻塞操作在 async 上下文中执行

- **文件**: `src/repository/sqlite.rs` 所有 async 方法
- **严重性**: High
- **描述**: 所有 repository 方法标记为 `async` 但内部执行的是同步的 `r2d2` 连接池操作和 `rusqlite` 查询。`pool.get()` 在连接池耗尽时会阻塞当前 tokio 线程，导致整个 runtime 的其他任务被饿死。
- **建议修复**: 使用 `tokio::task::spawn_blocking` 包裹所有数据库操作，或者改用真正的异步 SQLite 驱动（如 `sqlx`）。示例：
  ```rust
  async fn find_by_id(&self, id: i64) -> Result<Option<User>> {
      let pool = self.pool.clone();
      tokio::task::spawn_blocking(move || {
          let conn = pool.get()?;
          // ... query logic
      }).await.map_err(|e| WebPlatformError::Internal(e.to_string()))?
  }
  ```

### 4. Rate Limiter 内存无限增长

- **文件**: `src/auth/rate_limit.rs` 第 55 行
- **严重性**: High
- **描述**: `cleanup_expired` 在每次 `check_rate_limit` 调用时执行全表扫描清理。在高并发场景下：(1) 每次请求都遍历整个 DashMap 造成性能问题；(2) 如果攻击者使用大量不同 IP/用户名，在清理前 DashMap 仍会无限增长。
- **建议修复**: 将清理逻辑移到后台定时任务（如每 30 秒执行一次），或设置 DashMap 容量上限。可以使用 `tokio::spawn` 启动周期性清理任务。

### 5. Token 黑名单仅存内存，重启后丢失

- **文件**: `src/main.rs` 第 37-48 行, `src/auth/jwt.rs` 第 76-81 行
- **严重性**: High
- **描述**: `invalidate_user_tokens` 只写入内存中的 `DashMap`，不持久化到数据库的 `token_blacklist` 表。服务重启后，所有已失效的 token 将重新变为有效。虽然启动时会从数据库加载，但运行时的失效操作不写入数据库。
- **建议修复**: 在 `invalidate_user_tokens` 中同时调用 `TokenBlacklistRepository::add_to_blacklist` 写入数据库。需要将 repo 引用传入或重构为通过 AppState 调用。

### 6. SQLite PRAGMA 仅对单个连接生效

- **文件**: `src/db/mod.rs` 第 17-21 行
- **严重性**: High
- **描述**: `PRAGMA journal_mode=WAL` 对数据库全局生效（正确），但 `PRAGMA foreign_keys=ON` 和 `PRAGMA busy_timeout=5000` 是连接级别的设置。从连接池获取的其他连接不会继承这些 PRAGMA。
- **建议修复**: 使用 `r2d2_sqlite::SqliteConnectionManager` 的 `with_init` 方法，确保每个新连接都执行这些 PRAGMA：
  ```rust
  let manager = SqliteConnectionManager::file(database_url)
      .with_init(|conn| {
          conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
      });
  ```

---

## Medium Issues

### 7. display_name 字段缺少长度验证

- **文件**: `src/handlers/user_profile.rs` 第 69-85 行
- **严重性**: Medium
- **描述**: `update_profile` 接受任意长度的 `display_name`，没有长度限制。攻击者可以提交超长字符串占用存储空间。
- **建议修复**: 添加长度验证，例如限制为 1-64 个字符。

### 8. gitlab_host 缺少 URL 格式验证

- **文件**: `src/handlers/user_profile.rs` 第 145-180 行
- **严重性**: Medium
- **描述**: `update_config` 中的 `gitlab_host` 字段没有验证是否为合法 URL。恶意输入可能导致后续使用该值时出现 SSRF 或其他问题。
- **建议修复**: 验证 `gitlab_host` 是否为合法的 HTTPS URL，且域名在允许列表内或至少是合法的 URL 格式。

### 9. 密码验证使用 timing-safe 但登录流程存在用户枚举

- **文件**: `src/handlers/auth.rs` 第 63-67 行
- **严重性**: Medium
- **描述**: 当用户不存在时直接返回 `Unauthorized`，不执行密码哈希验证。攻击者可以通过响应时间差异判断用户名是否存在（用户存在时需要执行 argon2 验证，耗时明显更长）。
- **建议修复**: 当用户不存在时，仍然执行一次 dummy 的 argon2 验证以保持一致的响应时间：
  ```rust
  let user = match state.repo.find_by_username(&req.username).await? {
      Some(u) => u,
      None => {
          // 执行 dummy 验证以防止时序攻击
          let _ = verify_password("dummy", "$argon2id$v=19$m=19456,t=2,p=1$...");
          return Err(WebPlatformError::Unauthorized);
      }
  };
  ```

### 10. Rate Limiter 在验证前计数

- **文件**: `src/handlers/auth.rs` 第 61 行
- **严重性**: Medium
- **描述**: `check_rate_limit` 在验证密码之前就增加了计数。这意味着即使登录成功，也会消耗一次配额。如果合法用户在短时间内多次登录（如 token 过期后重新登录），可能被误封。
- **建议修复**: 仅在登录失败时增加计数，或者在登录成功时重置该用户的计数器。

### 11. JWT 过期时间过长（7天）

- **文件**: `src/auth/jwt.rs` 第 25 行
- **严重性**: Medium
- **描述**: Token 有效期为 7 天，对于管理平台来说过长。如果 token 泄露，攻击者有较长的利用窗口。
- **建议修复**: 缩短 access token 有效期至 1-2 小时，引入 refresh token 机制用于续期。或至少将过期时间做成可配置项。

### 12. 搜索参数未转义 SQL LIKE 通配符

- **文件**: `src/repository/sqlite.rs` 第 113 行
- **严重性**: Medium
- **描述**: `format!("%{}%", s)` 中的 `s` 如果包含 `%` 或 `_` 字符，会被 SQLite 解释为 LIKE 通配符，导致搜索结果不符合预期。虽然不是 SQL 注入（使用了参数化查询），但属于逻辑错误。
- **建议修复**: 转义搜索字符串中的 `%` 和 `_`：
  ```rust
  let escaped = s.replace('%', "\\%").replace('_', "\\_");
  conditions.push("(username LIKE ?1 OR display_name LIKE ?1) ESCAPE '\\'".to_string());
  params.push(Box::new(format!("%{}%", escaped)));
  ```

### 13. upsert_config 的 COALESCE 逻辑导致无法清除已设置的值

- **文件**: `src/repository/sqlite.rs` 第 221-232 行
- **严重性**: Medium
- **描述**: `ON CONFLICT DO UPDATE SET gitlab_token = COALESCE(?2, gitlab_token)` 意味着传入 NULL 时保留旧值。这使得用户无法主动清除已配置的 token（例如想取消 GitLab 集成）。
- **建议修复**: 区分"未提供"（不更新）和"显式清除"（设为 NULL）两种语义。可以在 handler 层判断：如果请求中包含该字段且为空字符串，则传入空字符串或特殊标记表示清除。

---

## Low Issues

### 14. AppState 中 jwt_secret 以 String 明文存储

- **文件**: `src/lib.rs` 第 22 行
- **严重性**: Low
- **描述**: `jwt_secret` 作为普通 `String` 存储在 `AppState` 中，可能在 debug 日志或 core dump 中泄露。
- **建议修复**: 使用 `secrecy::Secret<String>` 包裹敏感字段，防止意外序列化或打印。

### 15. parse_datetime 静默回退到默认值

- **文件**: `src/repository/sqlite.rs` 第 20-23 行
- **严重性**: Low
- **描述**: `parse_datetime` 在解析失败时返回 `NaiveDateTime::default()`（即 1970-01-01 00:00:00），而不是报错。这会掩盖数据库中的数据损坏问题。
- **建议修复**: 返回 `Result` 并在调用处传播错误，或至少记录一条 warning 日志。

### 16. 缺少请求体大小限制

- **文件**: `src/router.rs` / `src/main.rs`
- **严重性**: Low
- **描述**: 没有配置全局的请求体大小限制。虽然 axum 默认限制为 2MB，但对于此应用的 JSON API 来说仍然过大。
- **建议修复**: 添加 `DefaultBodyLimit::max(64 * 1024)` 或类似的合理限制。

### 17. 未使用的 TokenBlacklistRepository trait 方法

- **文件**: `src/repository/traits.rs` 第 41-49 行
- **严重性**: Low
- **描述**: `TokenBlacklistRepository` trait 定义了 `add_to_blacklist`、`is_blacklisted`、`load_all` 方法，但运行时的 token 失效逻辑（`invalidate_user_tokens`）只操作内存 DashMap，不调用这些方法。这些方法仅在启动时的 `load_all` 被使用。
- **建议修复**: 参见 Issue #5，将持久化逻辑与内存缓存统一。

### 18. 软删除后未失效用户 token

- **文件**: `src/handlers/admin_users.rs` 第 158-177 行
- **严重性**: Low（已有部分缓解）
- **描述**: `delete_user` 执行软删除后没有调用 `invalidate_user_tokens`。虽然后续请求在 `find_by_id` 时会因 `deleted_at IS NULL` 条件返回 None 导致 404/401，但 JWT 验证本身仍然通过。这意味着已删除用户的 token 在某些不查询数据库的中间件中仍可能被视为有效。
- **建议修复**: 在 `soft_delete` 成功后调用 `invalidate_user_tokens(id, &state.token_blacklist)`。

---

## Architecture Notes (非问题，仅建议)

1. **Repository 层的 async trait 设计合理**，但由于底层是同步的 r2d2/rusqlite，建议长期迁移到 sqlx 或至少包裹 spawn_blocking。
2. **错误处理模式良好**：统一的 `WebPlatformError` 枚举 + `IntoResponse` 实现，内部错误不泄露给客户端。
3. **测试覆盖充分**：包含单元测试、集成测试和 E2E 测试，覆盖了认证、授权、CRUD 和边界情况。
4. **加密实现正确**：AES-256-GCM 使用随机 nonce，密码使用 Argon2id，符合最佳实践。
5. **输入验证到位**：用户名、密码、角色都有验证，使用参数化查询防止 SQL 注入。

---

## 总结

| 严重性 | 数量 |
|--------|------|
| Critical | 2 |
| High | 4 |
| Medium | 6 |
| Low | 5 |

最需要优先修复的是：CORS 配置（#2）、阻塞 async（#3）、token 黑名单持久化（#5）、PRAGMA 连接级设置（#6）。硬编码密码（#1）在生产部署前必须解决。
