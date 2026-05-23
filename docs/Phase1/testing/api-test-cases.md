# 接口测试用例清单

本文档为 Phase 1 所有 API 端点的完整测试矩阵。每个端点覆盖正常请求、认证失败、权限不足、参数错误、资源不存在、Rate Limit 等场景。

---

## 1. POST /api/auth/login

### 接口说明
用户登录，返回 JWT Token。

### 测试矩阵

| # | 场景 | 请求 | 预期状态码 | 预期响应 |
|---|------|------|-----------|---------|
| 1 | 正常登录 | `{"username":"admin","password":"Admin@123456"}` | 200 | `{"token":"eyJ...","user":{"id":1,"username":"admin","role":"admin"}}` |
| 2 | 普通用户登录 | `{"username":"john","password":"User@123456"}` | 200 | `{"token":"eyJ...","user":{"id":2,"username":"john","role":"user"}}` |
| 3 | 用户名不存在 | `{"username":"ghost","password":"any"}` | 401 | `{"error":{"code":"INVALID_CREDENTIALS","message":"用户名或密码错误"}}` |
| 4 | 密码错误 | `{"username":"admin","password":"wrong"}` | 401 | `{"error":{"code":"INVALID_CREDENTIALS","message":"用户名或密码错误"}}` |
| 5 | 已删除用户登录 | `{"username":"deleted_user","password":"Pass@123"}` | 401 | `{"error":{"code":"INVALID_CREDENTIALS","message":"用户名或密码错误"}}` |
| 6 | 用户名为空 | `{"username":"","password":"Pass@123"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"username":"不能为空"}}}` |
| 7 | 密码为空 | `{"username":"admin","password":""}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"password":"不能为空"}}}` |
| 8 | 缺少 username 字段 | `{"password":"Pass@123"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"username":"必填字段"}}}` |
| 9 | 缺少 password 字段 | `{"username":"admin"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"password":"必填字段"}}}` |
| 10 | 空 JSON body | `{}` | 400 | `{"error":{"code":"VALIDATION_ERROR",...}}` |
| 11 | 非 JSON body | `username=admin` | 400 | `{"error":{"code":"INVALID_CONTENT_TYPE",...}}` |
| 12 | Rate limit - 用户名维度 | 同一用户名连续第 6 次失败 | 429 | `{"error":{"code":"RATE_LIMITED","retry_after":60}}` + `Retry-After: 60` header |
| 13 | Rate limit - IP 维度 | 同一 IP 连续第 21 次请求 | 429 | `{"error":{"code":"RATE_LIMITED","retry_after":60}}` |
| 14 | Rate limit 恢复 | 等待窗口过期后重试 | 200 | 正常登录成功 |

---

## 2. PUT /api/auth/password

### 接口说明
修改当前用户密码。需要认证。

### 测试矩阵

| # | 场景 | Headers | 请求 | 预期状态码 | 预期响应 |
|---|------|---------|------|-----------|---------|
| 1 | 正常修改密码 | Bearer valid_token | `{"old_password":"Admin@123456","new_password":"NewPass@789"}` | 200 | `{"message":"密码修改成功"}` |
| 2 | 旧密码错误 | Bearer valid_token | `{"old_password":"wrong","new_password":"NewPass@789"}` | 401 | `{"error":{"code":"WRONG_PASSWORD","message":"旧密码错误"}}` |
| 3 | 新密码太短 | Bearer valid_token | `{"old_password":"Admin@123456","new_password":"Ab@1"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"new_password":"至少8个字符"}}}` |
| 4 | 新密码无特殊字符 | Bearer valid_token | `{"old_password":"Admin@123456","new_password":"NewPass789"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"new_password":"需要包含特殊字符"}}}` |
| 5 | 新旧密码相同 | Bearer valid_token | `{"old_password":"Admin@123456","new_password":"Admin@123456"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","message":"新密码不能与旧密码相同"}}` |
| 6 | 未认证 | 无 Authorization header | 任意 body | 401 | `{"error":{"code":"UNAUTHORIZED","message":"未提供认证信息"}}` |
| 7 | Token 过期 | Bearer expired_token | 任意 body | 401 | `{"error":{"code":"TOKEN_EXPIRED","message":"Token 已过期"}}` |
| 8 | Token 已撤销 | Bearer revoked_token | 任意 body | 401 | `{"error":{"code":"TOKEN_REVOKED","message":"Token 已失效"}}` |
| 9 | 缺少 old_password | Bearer valid_token | `{"new_password":"NewPass@789"}` | 400 | 验证错误 |
| 10 | 缺少 new_password | Bearer valid_token | `{"old_password":"Admin@123456"}` | 400 | 验证错误 |

---

## 3. GET /api/user/profile

### 接口说明
获取当前用户个人信息。

### 测试矩阵

| # | 场景 | Headers | 预期状态码 | 预期响应 |
|---|------|---------|-----------|---------|
| 1 | 正常获取 | Bearer valid_token | 200 | `{"id":1,"username":"admin","display_name":"Administrator","role":"admin","created_at":"..."}` |
| 2 | 普通用户获取 | Bearer user_token | 200 | `{"id":2,"username":"john","display_name":"John","role":"user",...}` |
| 3 | 未认证 | 无 header | 401 | `{"error":{"code":"UNAUTHORIZED"}}` |
| 4 | Token 格式错误 | Authorization: "Token xxx" | 401 | `{"error":{"code":"UNAUTHORIZED"}}` |
| 5 | Token 过期 | Bearer expired_token | 401 | `{"error":{"code":"TOKEN_EXPIRED"}}` |
| 6 | 用户已被删除（token 未撤销） | Bearer deleted_user_token | 401 | `{"error":{"code":"USER_NOT_FOUND"}}` |

---

## 4. PUT /api/user/profile

### 接口说明
更新当前用户个人信息（display_name）。

### 测试矩阵

| # | 场景 | Headers | 请求 | 预期状态码 | 预期响应 |
|---|------|---------|------|-----------|---------|
| 1 | 正常更新 | Bearer valid_token | `{"display_name":"新名字"}` | 200 | `{"message":"更新成功","user":{...}}` |
| 2 | 更新为空字符串 | Bearer valid_token | `{"display_name":""}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"display_name":"不能为空"}}}` |
| 3 | 更新为超长字符串 | Bearer valid_token | `{"display_name":"a"*256}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"display_name":"最多64个字符"}}}` |
| 4 | Unicode 名称 | Bearer valid_token | `{"display_name":"中文名字"}` | 200 | 正常更新 |
| 5 | 未认证 | 无 header | 任意 body | 401 | 未认证错误 |
| 6 | 空 body | Bearer valid_token | `{}` | 400 | 验证错误 |

---

## 5. GET /api/user/config

### 接口说明
获取当前用户的 Token 配置。

### 测试矩阵

| # | 场景 | Headers | 预期状态码 | 预期响应 |
|---|------|---------|-----------|---------|
| 1 | 有配置 | Bearer valid_token | 200 | `{"gitlab_token":"***","gitlab_host":"https://gitlab.example.com","github_token":null}` |
| 2 | 无配置（首次） | Bearer new_user_token | 200 | `{"gitlab_token":null,"gitlab_host":null,"github_token":null}` |
| 3 | Token 脱敏显示 | Bearer valid_token | 200 | token 字段显示为掩码（非明文） |
| 4 | 未认证 | 无 header | 401 | 未认证错误 |
| 5 | Token 过期 | Bearer expired_token | 401 | Token 过期错误 |

---

## 6. PUT /api/user/config

### 接口说明
更新当前用户的 Token 配置。

### 测试矩阵

| # | 场景 | Headers | 请求 | 预期状态码 | 预期响应 |
|---|------|---------|------|-----------|---------|
| 1 | 正常更新 GitLab Token | Bearer valid_token | `{"gitlab_token":"glpat-xxx","gitlab_host":"https://gitlab.com"}` | 200 | `{"message":"配置更新成功"}` |
| 2 | 正常更新 GitHub Token | Bearer valid_token | `{"github_token":"ghp_xxx"}` | 200 | 更新成功 |
| 3 | 同时更新多个 Token | Bearer valid_token | `{"gitlab_token":"glpat-xxx","github_token":"ghp_xxx"}` | 200 | 更新成功 |
| 4 | 清空 Token | Bearer valid_token | `{"gitlab_token":null}` | 200 | Token 清除 |
| 5 | GitLab Host 格式错误 | Bearer valid_token | `{"gitlab_host":"not-a-url"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"gitlab_host":"无效的 URL 格式"}}}` |
| 6 | GitLab Host 无协议 | Bearer valid_token | `{"gitlab_host":"gitlab.com"}` | 400 | URL 格式错误 |
| 7 | 未认证 | 无 header | 任意 body | 401 | 未认证错误 |
| 8 | 空 body | Bearer valid_token | `{}` | 400 | 验证错误（至少一个字段） |

---

## 7. GET /api/admin/users

### 接口说明
管理员获取用户列表。支持分页。

### 测试矩阵

| # | 场景 | Headers | Query Params | 预期状态码 | 预期响应 |
|---|------|---------|-------------|-----------|---------|
| 1 | 正常获取（默认分页） | Bearer admin_token | 无 | 200 | `{"data":[...],"pagination":{"total":N,"page":1,"size":20}}` |
| 2 | 指定分页 | Bearer admin_token | `?page=2&size=5` | 200 | 返回第 2 页，每页 5 条 |
| 3 | 超出范围的页码 | Bearer admin_token | `?page=999` | 200 | `{"data":[],"pagination":{"total":N,"page":999,"size":20}}` |
| 4 | 不包含已删除用户 | Bearer admin_token | 无 | 200 | 列表中无 deleted_at 非空的用户 |
| 5 | 未认证 | 无 header | 无 | 401 | 未认证错误 |
| 6 | 权限不足（普通用户） | Bearer user_token | 无 | 403 | `{"error":{"code":"FORBIDDEN","message":"需要管理员权限"}}` |
| 7 | page 参数非数字 | Bearer admin_token | `?page=abc` | 400 | 参数错误 |
| 8 | size 超出限制 | Bearer admin_token | `?size=1000` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"size":"最大100"}}}` |

---

## 8. POST /api/admin/users

### 接口说明
管理员创建新用户。

### 测试矩阵

| # | 场景 | Headers | 请求 | 预期状态码 | 预期响应 |
|---|------|---------|------|-----------|---------|
| 1 | 正常创建 | Bearer admin_token | `{"username":"newuser","password":"Pass@123456","role":"user","display_name":"New User"}` | 201 | `{"id":N,"username":"newuser","role":"user",...}` |
| 2 | 创建管理员 | Bearer admin_token | `{"username":"admin2","password":"Pass@123456","role":"admin"}` | 201 | role=admin 的用户 |
| 3 | 用户名重复 | Bearer admin_token | `{"username":"admin",...}` | 409 | `{"error":{"code":"DUPLICATE_USERNAME","message":"用户名已存在"}}` |
| 4 | 用户名为空 | Bearer admin_token | `{"username":"","password":"Pass@123456","role":"user"}` | 400 | 验证错误 |
| 5 | 用户名太短 | Bearer admin_token | `{"username":"ab","password":"Pass@123456","role":"user"}` | 400 | 验证错误 |
| 6 | 用户名含特殊字符 | Bearer admin_token | `{"username":"user@name","password":"Pass@123456","role":"user"}` | 400 | 验证错误 |
| 7 | 密码不合规 | Bearer admin_token | `{"username":"newuser","password":"123","role":"user"}` | 400 | 密码规则错误 |
| 8 | 无效角色 | Bearer admin_token | `{"username":"newuser","password":"Pass@123456","role":"superadmin"}` | 400 | `{"error":{"code":"VALIDATION_ERROR","fields":{"role":"无效的角色"}}}` |
| 9 | 未认证 | 无 header | 任意 body | 401 | 未认证错误 |
| 10 | 权限不足 | Bearer user_token | 任意 body | 403 | 权限不足错误 |
| 11 | 缺少必填字段 | Bearer admin_token | `{"username":"newuser"}` | 400 | 验证错误 |

---

## 9. DELETE /api/admin/users/:id

### 接口说明
管理员删除用户（软删除）。

### 测试矩阵

| # | 场景 | Headers | URL | 预期状态码 | 预期响应 |
|---|------|---------|-----|-----------|---------|
| 1 | 正常删除 | Bearer admin_token | /api/admin/users/2 | 200 | `{"message":"用户已删除"}` |
| 2 | 删除不存在的用户 | Bearer admin_token | /api/admin/users/999 | 404 | `{"error":{"code":"NOT_FOUND","message":"用户不存在"}}` |
| 3 | 删除自己 | Bearer admin_token | /api/admin/users/1 (自己的 id) | 400 | `{"error":{"code":"CANNOT_DELETE_SELF","message":"不能删除自己"}}` |
| 4 | 删除已删除的用户 | Bearer admin_token | /api/admin/users/3 (已软删除) | 404 | 用户不存在 |
| 5 | id 非数字 | Bearer admin_token | /api/admin/users/abc | 400 | 参数错误 |
| 6 | 未认证 | 无 header | /api/admin/users/2 | 401 | 未认证错误 |
| 7 | 权限不足 | Bearer user_token | /api/admin/users/2 | 403 | 权限不足错误 |
| 8 | 删除后 token 失效 | Bearer admin_token | 删除用户 2 后，用户 2 的 token 访问 | 401 | Token 已撤销 |

---

## 10. PUT /api/admin/users/:id/reset-password

### 接口说明
管理员重置用户密码为临时密码。

### 测试矩阵

| # | 场景 | Headers | URL | 预期状态码 | 预期响应 |
|---|------|---------|-----|-----------|---------|
| 1 | 正常重置 | Bearer admin_token | /api/admin/users/2/reset-password | 200 | `{"temporary_password":"TmpXxx@123"}` |
| 2 | 用户不存在 | Bearer admin_token | /api/admin/users/999/reset-password | 404 | 用户不存在 |
| 3 | 重置已删除用户 | Bearer admin_token | /api/admin/users/3/reset-password (已删除) | 404 | 用户不存在 |
| 4 | 重置自己的密码 | Bearer admin_token | /api/admin/users/1/reset-password | 200 | 允许（admin 可重置自己） |
| 5 | id 非数字 | Bearer admin_token | /api/admin/users/abc/reset-password | 400 | 参数错误 |
| 6 | 未认证 | 无 header | /api/admin/users/2/reset-password | 401 | 未认证错误 |
| 7 | 权限不足 | Bearer user_token | /api/admin/users/2/reset-password | 403 | 权限不足错误 |
| 8 | 重置后旧密码失效 | 重置后 | 用旧密码登录 | 401 | 登录失败 |
| 9 | 重置后临时密码可用 | 重置后 | 用临时密码登录 | 200 | 登录成功 |
| 10 | 重置后旧 token 失效 | 重置后 | 用户 2 的旧 token 访问 | 401 | Token 已撤销 |

---

## 11. GET /health

### 接口说明
健康检查端点。无需认证。

### 测试矩阵

| # | 场景 | Headers | 预期状态码 | 预期响应 |
|---|------|---------|-----------|---------|
| 1 | 正常健康检查 | 无 | 200 | `{"status":"ok","version":"0.1.0","uptime_seconds":N}` |
| 2 | 带认证也可访问 | Bearer valid_token | 200 | 同上 |
| 3 | 数据库连接正常 | 无 | 200 | `{"status":"ok","db":"connected"}` |
| 4 | 数据库连接异常 | 无（模拟 DB 故障） | 503 | `{"status":"degraded","db":"disconnected"}` |

---

## 12. 通用安全测试

以下测试适用于所有需要认证的端点。

| # | 场景 | 适用端点 | 请求 | 预期结果 |
|---|------|---------|------|---------|
| 1 | SQL 注入 - 用户名 | POST /auth/login | `{"username":"' OR 1=1 --","password":"x"}` | 401（非 200） |
| 2 | SQL 注入 - 路径参数 | DELETE /admin/users/:id | /admin/users/1;DROP TABLE users | 400 |
| 3 | XSS - display_name | PUT /user/profile | `{"display_name":"<script>alert(1)</script>"}` | 200 但输出转义 |
| 4 | 超大 body | POST /auth/login | 1MB JSON | 413 Payload Too Large |
| 5 | Content-Type 不匹配 | POST /auth/login | Content-Type: text/plain | 400 或 415 |
| 6 | HTTP Method 不允许 | GET /auth/login | GET 请求 | 405 Method Not Allowed |
| 7 | CORS preflight | OPTIONS /api/auth/login | Origin: http://evil.com | 正确的 CORS 响应 |
| 8 | 并发请求一致性 | POST /admin/users | 10 个并发相同请求 | 仅 1 个 201，其余 409 |

---

## 13. 响应格式规范

### 成功响应

```json
// 单个资源
{
  "id": 1,
  "username": "admin",
  "display_name": "Administrator",
  "role": "admin",
  "created_at": "2024-01-01T00:00:00Z"
}

// 列表资源
{
  "data": [...],
  "pagination": {
    "total": 50,
    "page": 1,
    "size": 20,
    "total_pages": 3
  }
}

// 操作确认
{
  "message": "操作成功"
}
```

### 错误响应

```json
{
  "error": {
    "code": "ERROR_CODE",
    "message": "人类可读的错误描述",
    "fields": {                    // 可选，仅验证错误时
      "field_name": "字段级错误描述"
    }
  }
}
```

### 错误码清单

| HTTP 状态码 | 错误码 | 说明 |
|------------|--------|------|
| 400 | VALIDATION_ERROR | 请求参数验证失败 |
| 400 | INVALID_CONTENT_TYPE | Content-Type 不正确 |
| 400 | CANNOT_DELETE_SELF | 不能删除自己 |
| 401 | UNAUTHORIZED | 未提供认证信息 |
| 401 | INVALID_CREDENTIALS | 用户名或密码错误 |
| 401 | TOKEN_EXPIRED | Token 已过期 |
| 401 | TOKEN_REVOKED | Token 已被撤销 |
| 401 | WRONG_PASSWORD | 旧密码错误 |
| 403 | FORBIDDEN | 权限不足 |
| 404 | NOT_FOUND | 资源不存在 |
| 405 | METHOD_NOT_ALLOWED | HTTP 方法不允许 |
| 409 | DUPLICATE_USERNAME | 用户名已存在 |
| 413 | PAYLOAD_TOO_LARGE | 请求体过大 |
| 429 | RATE_LIMITED | 请求频率超限 |
| 500 | INTERNAL_ERROR | 服务器内部错误 |
| 503 | SERVICE_UNAVAILABLE | 服务不可用 |
