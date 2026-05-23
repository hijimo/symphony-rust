# 后端测试用例清单

## 覆盖率目标

| 模块 | 行覆盖率 | 分支覆盖率 |
|------|---------|-----------|
| auth (JWT/密码/限流) | 95% | 90% |
| repository | 90% | 85% |
| handler | 85% | 80% |
| middleware | 90% | 85% |
| 整体 | 85% | 80% |

---

## 1. Auth 模块单元测试

### 1.1 JWT 生成与验证

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 1 | test_jwt_generate_valid_token | 有效用户信息 | 调用 `generate_token(user_id=1, role="admin")` | 返回非空 token 字符串 |
| 2 | test_jwt_token_contains_correct_claims | 有效用户信息 | 生成 token 后解码 claims | sub=user_id, role=admin, exp 在 7 天后 |
| 3 | test_jwt_verify_valid_token | 有效 token | 调用 `verify_token(token)` | 返回 Ok(Claims) |
| 4 | test_jwt_verify_expired_token | 已过期 token | 调用 `verify_token(expired_token)` | 返回 Err(TokenExpired) |
| 5 | test_jwt_verify_invalid_signature | 篡改签名的 token | 调用 `verify_token(tampered_token)` | 返回 Err(InvalidToken) |
| 6 | test_jwt_reject_none_algorithm | alg=none 的 token | 调用 `verify_token(none_alg_token)` | 返回 Err(InvalidToken) |
| 7 | test_jwt_verify_malformed_token | 格式错误的字符串 | 调用 `verify_token("not.a.jwt")` | 返回 Err(InvalidToken) |
| 8 | test_jwt_blacklisted_token | token 在黑名单中 | 调用 `verify_token(blacklisted_token)` | 返回 Err(TokenRevoked) |
| 9 | test_jwt_add_to_blacklist | 有效 token | 调用 `revoke_token(token)` 后验证 | token 验证失败 |
| 10 | test_jwt_blacklist_persistence | 重启后 | 添加黑名单 → 重建 blacklist 实例 | 黑名单条目仍存在 |

### 1.2 密码哈希与验证

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 11 | test_password_hash_produces_valid_hash | 明文密码 | 调用 `hash_password("Admin@123")` | 返回非空 hash，以 `$argon2` 开头 |
| 12 | test_password_verify_correct | 正确密码 | hash 后调用 `verify_password(plain, hash)` | 返回 true |
| 13 | test_password_verify_incorrect | 错误密码 | 调用 `verify_password("wrong", hash)` | 返回 false |
| 14 | test_password_hash_different_each_time | 同一密码 | 两次 hash 同一密码 | 两个 hash 值不同（salt 不同） |
| 15 | test_password_hash_empty_string | 空字符串 | 调用 `hash_password("")` | 返回 Err 或 hash（取决于策略） |
| 16 | test_password_validation_min_length | 短密码 | 验证 "Ab@1" | 返回 Err(PasswordTooShort) |
| 17 | test_password_validation_requires_special | 无特殊字符 | 验证 "Abcdefg123" | 返回 Err(PasswordRequiresSpecial) |
| 18 | test_password_validation_valid | 合规密码 | 验证 "Admin@123456" | 返回 Ok |

### 1.3 Rate Limit

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 19 | test_rate_limit_allows_under_threshold | 空计数器 | 同一用户名连续 4 次请求 | 全部允许 |
| 20 | test_rate_limit_blocks_at_threshold | 空计数器 | 同一用户名连续 6 次请求 | 第 6 次被拒绝 |
| 21 | test_rate_limit_per_username | 空计数器 | user_a 5次 + user_b 5次 | 两个用户各自独立计数 |
| 22 | test_rate_limit_per_ip | 空计数器 | 同一 IP 连续 21 次请求 | 第 21 次被拒绝 |
| 23 | test_rate_limit_window_reset | 已达限制 | 等待窗口过期（mock 时间） | 限制解除，请求允许 |
| 24 | test_rate_limit_returns_retry_after | 已达限制 | 发送请求 | 响应包含 Retry-After header |

---

## 2. Repository 层单元测试

### 2.1 UserRepository

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 25 | test_create_user_success | 空数据库 | 创建用户 (username, hash, role) | 返回 User，id > 0 |
| 26 | test_create_user_duplicate_username | 已存在同名用户 | 创建同名用户 | 返回 Err(DuplicateUsername) |
| 27 | test_find_user_by_id_exists | 已创建用户 | find_by_id(user.id) | 返回 Some(User) |
| 28 | test_find_user_by_id_not_exists | 空数据库 | find_by_id(999) | 返回 None |
| 29 | test_find_user_by_id_soft_deleted | 已软删除用户 | find_by_id(deleted_user.id) | 返回 None |
| 30 | test_find_user_by_username_exists | 已创建用户 | find_by_username("admin") | 返回 Some(User) |
| 31 | test_find_user_by_username_not_exists | 空数据库 | find_by_username("ghost") | 返回 None |
| 32 | test_list_users_empty | 空数据库 | list_users() | 返回空 Vec |
| 33 | test_list_users_excludes_deleted | 有正常+已删除用户 | list_users() | 仅返回未删除用户 |
| 34 | test_list_users_pagination | 10 个用户 | list_users(page=2, size=3) | 返回 3 个用户，total=10 |
| 35 | test_soft_delete_user | 已创建用户 | soft_delete(user.id) | deleted_at 非 NULL |
| 36 | test_soft_delete_nonexistent | 空数据库 | soft_delete(999) | 返回 Err(NotFound) |
| 37 | test_update_user_display_name | 已创建用户 | update(id, {display_name: "New"}) | display_name 更新 |
| 38 | test_update_user_password_hash | 已创建用户 | update(id, {password_hash: new_hash}) | password_hash 更新 |

### 2.2 ConfigRepository

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 39 | test_get_config_exists | 已保存配置 | get_config(user_id) | 返回 Some(UserConfig) |
| 40 | test_get_config_not_exists | 无配置 | get_config(user_id) | 返回 None |
| 41 | test_upsert_config_create | 无配置 | upsert_config(user_id, config) | 创建新记录 |
| 42 | test_upsert_config_update | 已有配置 | upsert_config(user_id, new_config) | 更新现有记录 |
| 43 | test_config_token_encrypted | 保存 token | 读取数据库原始值 | 存储值非明文 |
| 44 | test_config_token_decrypted_on_read | 已保存加密 token | get_config(user_id) | 返回解密后的明文 |

---

## 3. Handler 层单元测试

### 3.1 AuthHandler

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 45 | test_login_handler_success | mock repo 返回用户 | POST login(valid_credentials) | 返回 200 + token |
| 46 | test_login_handler_user_not_found | mock repo 返回 None | POST login(unknown_user) | 返回 401 |
| 47 | test_login_handler_wrong_password | mock repo 返回用户 | POST login(wrong_password) | 返回 401 |
| 48 | test_login_handler_deleted_user | mock repo 返回已删除用户 | POST login(deleted_user) | 返回 401 |
| 49 | test_login_handler_empty_username | 无 | POST login(username="") | 返回 400 |
| 50 | test_login_handler_empty_password | 无 | POST login(password="") | 返回 400 |
| 51 | test_change_password_success | mock 验证旧密码通过 | PUT password(old, new) | 返回 200 |
| 52 | test_change_password_wrong_old | mock 验证旧密码失败 | PUT password(wrong_old, new) | 返回 401 |
| 53 | test_change_password_weak_new | 无 | PUT password(old, "123") | 返回 400 |
| 54 | test_change_password_revokes_other_tokens | 修改成功 | 检查黑名单 | 旧 token 被加入黑名单 |

### 3.2 AdminHandler

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 55 | test_admin_create_user_success | admin 角色 | POST /admin/users(valid_data) | 返回 201 + User |
| 56 | test_admin_create_user_duplicate | admin 角色 | POST /admin/users(existing_username) | 返回 409 |
| 57 | test_admin_create_user_invalid_role | admin 角色 | POST /admin/users(role="superadmin") | 返回 400 |
| 58 | test_admin_list_users | admin 角色 | GET /admin/users | 返回 200 + 用户列表 |
| 59 | test_admin_delete_user_success | admin 角色 | DELETE /admin/users/:id | 返回 200 |
| 60 | test_admin_delete_self | admin 角色 | DELETE /admin/users/:self_id | 返回 400（不能删除自己） |
| 61 | test_admin_reset_password | admin 角色 | PUT /admin/users/:id/reset-password | 返回 200 + 临时密码 |
| 62 | test_admin_reset_password_nonexistent | admin 角色 | PUT /admin/users/999/reset-password | 返回 404 |

### 3.3 UserHandler

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 63 | test_get_profile_success | 已认证用户 | GET /user/profile | 返回 200 + Profile |
| 64 | test_update_profile_success | 已认证用户 | PUT /user/profile(display_name) | 返回 200 |
| 65 | test_update_profile_empty_name | 已认证用户 | PUT /user/profile(display_name="") | 返回 400 |
| 66 | test_get_config_success | 已认证用户 | GET /user/config | 返回 200 + Config |
| 67 | test_get_config_no_config | 已认证用户，无配置 | GET /user/config | 返回 200 + 空配置 |
| 68 | test_update_config_success | 已认证用户 | PUT /user/config(tokens) | 返回 200 |
| 69 | test_update_config_invalid_url | 已认证用户 | PUT /user/config(gitlab_host="not-url") | 返回 400 |

---

## 4. Middleware 单元测试

### 4.1 JWT 认证中间件

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 70 | test_middleware_valid_token_passes | 有效 token | 请求带 Authorization header | 请求通过，注入 Claims |
| 71 | test_middleware_missing_header | 无 header | 请求无 Authorization | 返回 401 |
| 72 | test_middleware_invalid_format | 错误格式 | Authorization: "Token xxx" | 返回 401 |
| 73 | test_middleware_expired_token | 过期 token | 请求带过期 token | 返回 401 |
| 74 | test_middleware_blacklisted_token | 黑名单 token | 请求带已撤销 token | 返回 401 |

### 4.2 角色检查中间件

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 75 | test_role_check_admin_access_admin_route | admin token | 访问 admin 路由 | 通过 |
| 76 | test_role_check_user_access_admin_route | user token | 访问 admin 路由 | 返回 403 |
| 77 | test_role_check_admin_access_user_route | admin token | 访问 user 路由 | 通过 |
| 78 | test_role_check_user_access_user_route | user token | 访问 user 路由 | 通过 |

---

## 5. 集成测试

### 5.1 数据库迁移

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 79 | test_migration_fresh_db | 空数据库 | 运行所有迁移 | 所有表创建成功 |
| 80 | test_migration_idempotent | 已迁移数据库 | 再次运行迁移 | 无错误，无变化 |
| 81 | test_migration_creates_indexes | 空数据库 | 运行迁移后查询 sqlite_master | 所有索引存在 |
| 82 | test_migration_default_admin | 空数据库 | 运行迁移 | 默认 admin 用户存在 |

### 5.2 Repository + DB 集成

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 83 | test_user_repo_full_lifecycle | 迁移后数据库 | create → find → update → delete | 每步结果正确 |
| 84 | test_config_repo_encryption_roundtrip | 迁移后数据库 | 保存加密 token → 读取解密 | 明文一致 |
| 85 | test_concurrent_user_creation | 迁移后数据库 | 并发创建 10 个不同用户 | 全部成功 |
| 86 | test_concurrent_duplicate_creation | 迁移后数据库 | 并发创建同名用户 | 仅 1 个成功，其余报错 |

### 5.3 Handler + Repository + DB 完整链路

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 87 | test_login_full_chain | 数据库有 admin 用户 | HTTP POST /auth/login | 返回有效 JWT |
| 88 | test_create_user_full_chain | admin 已登录 | HTTP POST /admin/users | 数据库新增记录 |
| 89 | test_change_password_full_chain | user 已登录 | HTTP PUT /auth/password | 旧密码失效，新密码可用 |
| 90 | test_update_config_full_chain | user 已登录 | HTTP PUT /user/config | 数据库记录更新 |

---

## 6. E2E 测试

### 6.1 完整用户流程

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 91 | test_e2e_user_full_lifecycle | 系统初始化 | 1. admin 登录<br>2. 创建普通用户<br>3. 普通用户登录<br>4. 修改个人信息<br>5. 配置 token<br>6. 修改密码<br>7. 用新密码重新登录 | 每步返回预期状态码 |
| 92 | test_e2e_admin_user_management | admin 已登录 | 1. 创建用户 A<br>2. 创建用户 B<br>3. 列表查看（含 A、B）<br>4. 删除用户 A<br>5. 列表查看（仅含 B）<br>6. 用户 A 登录失败 | 每步返回预期结果 |
| 93 | test_e2e_password_reset_flow | admin + user 存在 | 1. admin 重置 user 密码<br>2. user 用旧密码登录失败<br>3. user 用新密码登录成功<br>4. user 修改为自定义密码<br>5. user 用自定义密码登录成功 | 密码流程完整 |

### 6.2 错误流程

| # | 用例名称 | 前置条件 | 步骤 | 预期结果 |
|---|---------|---------|------|---------|
| 94 | test_e2e_rate_limit_trigger | 系统初始化 | 1. 连续 6 次错误密码登录<br>2. 第 6 次检查响应 | 返回 429 + Retry-After |
| 95 | test_e2e_rate_limit_recovery | 已触发限流 | 1. 等待窗口过期<br>2. 再次登录 | 登录成功 |
| 96 | test_e2e_token_revocation_on_delete | admin + user 存在 | 1. user 登录获取 token<br>2. admin 删除 user<br>3. user 用原 token 访问 | 返回 401 |
| 97 | test_e2e_token_revocation_on_password_change | user 已登录 | 1. user 在设备 A 登录<br>2. user 在设备 B 修改密码<br>3. 设备 A 的 token 访问 | 返回 401 |
| 98 | test_e2e_privilege_escalation_attempt | user 已登录 | 1. user 尝试访问 /admin/users<br>2. user 尝试删除其他用户 | 全部返回 403 |

---

## 7. 输入验证测试

### 7.1 用户名验证

| # | 用例名称 | 输入 | 预期结果 |
|---|---------|------|---------|
| 99 | test_username_empty | "" | 400: username required |
| 100 | test_username_too_short | "ab" | 400: username too short |
| 101 | test_username_too_long | "a" * 65 | 400: username too long |
| 102 | test_username_special_chars | "user@name" | 400: invalid characters |
| 103 | test_username_sql_injection | "'; DROP TABLE users;--" | 400: invalid characters |
| 104 | test_username_unicode | "用户名" | 取决于策略（400 或通过） |
| 105 | test_username_valid | "john_doe123" | 通过 |

### 7.2 密码验证

| # | 用例名称 | 输入 | 预期结果 |
|---|---------|------|---------|
| 106 | test_password_empty | "" | 400: password required |
| 107 | test_password_too_short | "Ab@1" | 400: min 8 characters |
| 108 | test_password_no_uppercase | "admin@123" | 400: requires uppercase |
| 109 | test_password_no_lowercase | "ADMIN@123" | 400: requires lowercase |
| 110 | test_password_no_digit | "Admin@abc" | 400: requires digit |
| 111 | test_password_no_special | "Admin1234" | 400: requires special char |
| 112 | test_password_valid | "Admin@123456" | 通过 |

---

## 8. 边界条件测试

| # | 用例名称 | 场景 | 预期结果 |
|---|---------|------|---------|
| 113 | test_concurrent_login_same_user | 10 个并发登录请求 | 全部成功，各自获得独立 token |
| 114 | test_large_user_list | 1000 个用户 | 分页正常，响应时间 < 200ms |
| 115 | test_token_at_expiry_boundary | token 恰好在验证时过期 | 返回 401 |
| 116 | test_db_connection_pool_exhaustion | 模拟连接池满 | 返回 503 Service Unavailable |
| 117 | test_request_body_too_large | 1MB JSON body | 返回 413 Payload Too Large |
| 118 | test_unicode_in_display_name | display_name="中文名字" | 正常存储和返回 |
| 119 | test_empty_json_body | POST 空 {} | 返回 400 + 具体字段错误 |
| 120 | test_extra_fields_ignored | POST 含未知字段 | 忽略未知字段，正常处理 |
