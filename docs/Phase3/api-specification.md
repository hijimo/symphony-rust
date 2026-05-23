# Symphony Web Platform API - Phase 3: 看板与 AI Issue 生成

## 概述

Phase 3 实现看板（Kanban）视图和 AI 辅助 Issue 生成功能。看板数据通过用户自己的 GitLab/GitHub Token 实时获取，服务端提供 singleflight 内存缓存（5-10s TTL）减少外部 API 调用。AI Issue 生成使用 Azure OpenAI gpt-5.5 模型，通过 SSE 流式返回。

## 通用协议

所有接口遵循 RESTful 规范，Content-Type 为 `application/json`（SSE 接口除外）。

### 统一响应格式

成功响应（`showType` 仅在错误时返回）：
```json
{ "data": T, "success": true, "retCode": "0", "retMsg": "ok" }
```

错误响应：
```json
{ "data": null, "success": false, "retCode": "EXT_001", "retMsg": "GitLab API 不可用", "showType": 4 }
```

> **注意**：成功响应不包含 `showType` 字段（与 Phase 1/2 保持一致，使用 `skip_serializing_if = "Option::is_none"`）。`retMsg` 成功时固定为 `"ok"`。

### 错误码

| retCode | 含义 | showType | HTTP Status |
|---------|------|----------|-------------|
| `0` | 成功 | 0 (silent) | 200 |
| `AUTH_001` | 未登录或 Token 过期 | 9 (redirect) | 401 |
| `AUTH_002` | 权限不足 | 2 (error) | 403 |
| `BIZ_001` | 业务参数错误 | 1 (warn) | 400/422 |
| `BIZ_002` | 资源不存在 | 2 (error) | 404 |
| `BIZ_003` | 操作冲突 | 1 (warn) | 409 |
| `SYS_001` | 系统内部错误 | 2 (error) | 500 |
| `TOKEN_001` | 平台 Token 无效或已过期 | 1 (warn) | 400 |
| `EXT_001` | 外部服务不可用（GitLab/GitHub/AI） | 4 (notification) | 502 |
| `EXT_002` | AI 生成速率限制 | 1 (warn) | 429 |

### 认证

除登录接口外，所有接口需要 Bearer Token（JWT）认证：
```
Authorization: Bearer <token>
```

### 字段命名约定

- 响应包装层使用 camelCase：`retCode`, `retMsg`, `showType`, `pageNo`, `pageSize`, `totalCount`
- 业务实体字段使用 snake_case：`issue_iid`, `merge_request`, `created_at`
- 此约定在整个 API 中保持一致

> **设计决策**：Phase 3 实体字段使用 snake_case，因为数据直接来源于 GitLab/GitHub API（它们使用 snake_case）。这与 Phase 1/2 的 camelCase 实体字段不同。前端通过 `caseTransform.ts` 统一处理。`iid` 对应 GitLab 的 `iid` 和 GitHub 的 `number`。

### 速率限制

| 端点 | 限制 | 说明 |
|------|------|------|
| `GET /api/projects/:id/kanban` | 30 次/分钟/用户 | 看板数据获取 |
| `POST /api/projects/:id/issues` | 20 次/分钟/用户 | Issue 创建 |
| `POST /api/projects/:id/issues/ai-generate` | 10 次/分钟/用户 | AI 生成（成本控制） |
| 其他 GET 接口 | 60 次/分钟/用户 | 详情查询 |

超出限制返回 HTTP 429：
```json
{ "data": null, "success": false, "retCode": "EXT_002", "retMsg": "请求过于频繁，请稍后重试", "showType": 1 }
```

### 缓存策略

服务端对 GitLab/GitHub API 调用实现 singleflight 内存缓存：
- **TTL**：5-10 秒（可配置）
- **缓存键**：`{user_id}:{project_id}:{api_path}:{query_hash}`
- **singleflight**：相同缓存键的并发请求只触发一次外部 API 调用，其余等待结果
- **缓存穿透保护**：空结果也缓存（TTL 减半）

---

## OpenAPI 3.0 规范

```yaml
openapi: '3.0.3'
info:
  title: Symphony Web Platform API - Phase 3
  version: '0.3.0'
  description: |
    看板视图与 AI Issue 生成相关 API（Phase 3）。
    
    看板数据通过用户自己的 GitLab/GitHub Token 实时获取，
    服务端提供 singleflight 内存缓存（5-10s TTL）。
    AI Issue 生成使用 Azure OpenAI gpt-5.5，SSE 流式返回。
  contact:
    name: Symphony Team
  license:
    name: MIT

servers:
  - url: http://localhost:3000
    description: 本地开发

tags:
  - name: Kanban
    description: 看板视图（三列：待处理/处理中/PR）
  - name: Issues
    description: Issue 创建与查询
  - name: AI Generation
    description: AI 辅助 Issue 内容生成（SSE 流式）
  - name: Merge Requests
    description: MR/PR 详情与关联查询

paths:
  /api/projects/{id}/kanban:
    get:
      summary: 获取看板数据
      operationId: getKanban
      description: |
        获取项目的三列看板数据，包含待处理、处理中、PR 三个分组。

        **数据来源**：
        - 使用当前登录用户的 GitLab/GitHub Token 调用外部 API
        - 服务端 singleflight 内存缓存（5-10s TTL）

        **三列定义**：
        - 待处理（todo）：Open issues 且无 `symphony-claimed` label
        - 处理中（in_progress）：Open issues 且有 `symphony-claimed` label
        - PR（pr）：与处理中 issues 关联的 MR/PR

        **业务规则**：
        - 需要项目成员权限或 admin
        - 用户必须已配置对应平台的 Token（未配置返回 BIZ_001）
        - 待处理列默认返回前 50 条（按创建时间倒序）
        - 处理中列返回全部（通常数量有限）
        - PR 列返回与处理中 issues 关联的所有 MR/PR
      tags: [Kanban]
      security:
        - bearerAuth: []
      parameters:
        - $ref: '#/components/parameters/ProjectId'
        - name: todo_limit
          in: query
          description: 待处理列最大返回数量
          schema:
            type: integer
            minimum: 1
            maximum: 100
            default: 50
        - name: assignee
          in: query
          description: 按指派人过滤（GitLab/GitHub 用户名）
          schema:
            type: string
            maxLength: 100
        - name: labels
          in: query
          description: 按标签过滤（逗号分隔，多个标签为 AND 关系）
          schema:
            type: string
            maxLength: 500
            example: "bug,high-priority"
        - name: search
          in: query
          description: 按 Issue 标题关键字搜索
          schema:
            type: string
            maxLength: 200
      responses:
        '200':
          description: 看板数据
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/KanbanResponse'
        '400':
          description: 用户未配置平台 Token
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz001'
        '401':
          description: 未认证
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth001'
        '403':
          description: 非项目成员，无权访问
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth002'
        '404':
          description: 项目不存在
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz002'
        '502':
          description: GitLab/GitHub API 不可用
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt001'
        '500':
          description: 系统内部错误
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseSys001'

  /api/projects/{id}/issues:
    post:
      summary: 创建 Issue
      operationId: createIssue
      description: |
        在项目对应的 GitLab/GitHub 仓库中创建 Issue。

        **业务规则**：
        - 使用当前登录用户的 Token 调用 GitLab/GitHub API 创建
        - Issue 作者即为该用户在平台上的身份
        - 需要项目成员权限或 admin
        - 用户必须已配置对应平台的 Token
        - title 最大 200 字符，description 最大 65536 字符
        - labels 为可选，需为仓库中已存在的标签
      tags: [Issues]
      security:
        - bearerAuth: []
      parameters:
        - $ref: '#/components/parameters/ProjectId'
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateIssueRequest'
      responses:
        '200':
          description: 创建成功
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/IssueResponse'
        '400':
          description: 参数校验失败或用户未配置 Token
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz001'
        '401':
          description: 未认证
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth001'
        '403':
          description: 非项目成员，无权访问
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth002'
        '404':
          description: 项目不存在
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz002'
        '502':
          description: GitLab/GitHub API 不可用
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt001'
        '500':
          description: 系统内部错误
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseSys001'

  /api/projects/{id}/issues/ai-generate:
    post:
      summary: AI 辅助生成 Issue 内容
      operationId: aiGenerateIssue
      description: |
        使用 Azure OpenAI gpt-5.5 模型，根据用户的简短需求描述生成结构化 Issue 内容。
        响应为 SSE（Server-Sent Events）流式返回。

        **业务规则**：
        - 需要项目成员权限或 admin
        - prompt 最大 2000 字符，title 最大 200 字符
        - 速率限制：10 次/用户/分钟
        - 生成内容遵循项目 WORKFLOW.md 中定义的 Issue 模板结构
        - Validation 部分的命令使用白名单校验（防止 prompt injection）

        **Prompt Injection 防护**：
        - 用户输入经过清洗，移除可能的指令注入
        - System Prompt 中明确限定输出格式
        - Validation 部分的命令仅允许白名单内的命令前缀
        - 输出长度限制：最大 4096 tokens

        **SSE 协议**：
        - Content-Type: text/event-stream
        - 每个事件为一行 `data: {json}\n\n`
        - 事件类型通过 JSON 中的 `type` 字段区分
        - 连接建立后立即开始推送，无需额外握手
      tags: [AI Generation]
      security:
        - bearerAuth: []
      parameters:
        - $ref: '#/components/parameters/ProjectId'
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/AIGenerateRequest'
      responses:
        '200':
          description: SSE 流式响应
          content:
            text/event-stream:
              schema:
                $ref: '#/components/schemas/SSEStream'
        '400':
          description: 参数校验失败（prompt 过长等）
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz001'
        '401':
          description: 未认证
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth001'
        '403':
          description: 非项目成员，无权访问
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth002'
        '404':
          description: 项目不存在
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz002'
        '429':
          description: AI 生成速率限制
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt002'
        '502':
          description: Azure OpenAI 服务不可用
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt001'
        '500':
          description: 系统内部错误
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseSys001'

  /api/projects/{id}/issues/{iid}:
    get:
      summary: Issue 详情
      operationId: getIssue
      description: |
        获取单个 Issue 的详细信息。

        **业务规则**：
        - 使用当前登录用户的 Token 调用 GitLab/GitHub API
        - iid 为 Issue 在仓库中的编号（非全局 ID）
        - 需要项目成员权限或 admin
        - 返回 Issue 完整信息，包含 labels、assignees、milestone 等
      tags: [Issues]
      security:
        - bearerAuth: []
      parameters:
        - $ref: '#/components/parameters/ProjectId'
        - $ref: '#/components/parameters/IssueIid'
      responses:
        '200':
          description: Issue 详情
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/IssueDetailResponse'
        '400':
          description: 用户未配置平台 Token
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz001'
        '401':
          description: 未认证
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth001'
        '403':
          description: 非项目成员，无权访问
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth002'
        '404':
          description: 项目或 Issue 不存在
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz002'
        '502':
          description: GitLab/GitHub API 不可用
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt001'
        '500':
          description: 系统内部错误
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseSys001'

  /api/projects/{id}/issues/{iid}/mrs:
    get:
      summary: Issue 关联的 MR/PR 列表
      operationId: getIssueMergeRequests
      description: |
        获取指定 Issue 关联的所有 Merge Request / Pull Request。

        **关联获取方式**：
        - GitLab：`GET /projects/:id/issues/:issue_iid/related_merge_requests`
        - GitHub：Timeline Events API 过滤 `cross-referenced_event`

        **业务规则**：
        - 使用当前登录用户的 Token
        - 需要项目成员权限或 admin
        - 返回所有通过 `Closes #N`、`Fixes #N` 等关键字关联的 MR/PR
      tags: [Merge Requests]
      security:
        - bearerAuth: []
      parameters:
        - $ref: '#/components/parameters/ProjectId'
        - $ref: '#/components/parameters/IssueIid'
      responses:
        '200':
          description: 关联的 MR/PR 列表
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/MergeRequestListResponse'
        '400':
          description: 用户未配置平台 Token
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz001'
        '401':
          description: 未认证
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth001'
        '403':
          description: 非项目成员，无权访问
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth002'
        '404':
          description: 项目或 Issue 不存在
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz002'
        '502':
          description: GitLab/GitHub API 不可用
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt001'
        '500':
          description: 系统内部错误
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseSys001'

  /api/projects/{id}/mrs/{iid}:
    get:
      summary: MR/PR 详情
      operationId: getMergeRequest
      description: |
        获取单个 Merge Request / Pull Request 的详细信息，包含关联的 Issues。

        **业务规则**：
        - 使用当前登录用户的 Token
        - iid 为 MR/PR 在仓库中的编号
        - 需要项目成员权限或 admin
        - 返回 MR/PR 完整信息，包含 CI 状态、Review 状态、关联 Issues
      tags: [Merge Requests]
      security:
        - bearerAuth: []
      parameters:
        - $ref: '#/components/parameters/ProjectId'
        - $ref: '#/components/parameters/MrIid'
      responses:
        '200':
          description: MR/PR 详情
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/MergeRequestDetailResponse'
        '400':
          description: 用户未配置平台 Token
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz001'
        '401':
          description: 未认证
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth001'
        '403':
          description: 非项目成员，无权访问
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseAuth002'
        '404':
          description: 项目或 MR/PR 不存在
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseBiz002'
        '502':
          description: GitLab/GitHub API 不可用
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseExt001'
        '500':
          description: 系统内部错误
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ErrorResponseSys001'

components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      bearerFormat: JWT
      description: |
        JWT Token 通过 POST /api/auth/login 获取。
        有效期 7 天，过期后需重新登录。
        请求头格式：Authorization: Bearer <token>

  parameters:
    ProjectId:
      name: id
      in: path
      required: true
      description: 项目 ID（Symphony 平台内部 ID）
      schema:
        type: integer
        minimum: 1
        example: 1

    IssueIid:
      name: iid
      in: path
      required: true
      description: Issue 在仓库中的编号（GitLab iid / GitHub number）
      schema:
        type: integer
        minimum: 1
        example: 42

    MrIid:
      name: iid
      in: path
      required: true
      description: MR/PR 在仓库中的编号（GitLab iid / GitHub number）
      schema:
        type: integer
        minimum: 1
        example: 15

  schemas:
    # ==================== Request Schemas ====================

    CreateIssueRequest:
      type: object
      required: [title]
      properties:
        title:
          type: string
          description: Issue 标题
          minLength: 1
          maxLength: 200
          example: "修复登录页面在移动端的样式错乱"
        description:
          type: string
          description: |
            Issue 描述内容（Markdown 格式）。
            建议使用 AI 生成接口获取结构化内容后填入。
          maxLength: 65536
          example: |
            ## 描述

            登录页面在移动端（宽度 < 768px）时布局错乱。

            ## Acceptance Criteria

            - [ ] 移动端登录表单居中显示
            - [ ] 输入框宽度自适应

            ## Validation

            - [ ] 响应式测试: `npx playwright test --project=mobile`
        labels:
          type: array
          description: 标签列表（需为仓库中已存在的标签名）
          items:
            type: string
            maxLength: 100
          maxItems: 20
          example: ["bug", "frontend"]
        assignee:
          type: string
          description: 指派人（GitLab/GitHub 用户名，不填则不指派）
          maxLength: 100
          example: "zhangsan"

    AIGenerateRequest:
      type: object
      required: [prompt]
      properties:
        prompt:
          type: string
          description: |
            用户输入的简短需求描述。
            AI 将基于此描述和项目 WORKFLOW.md 模板生成结构化 Issue 内容。
          minLength: 5
          maxLength: 2000
          example: "修复登录页面在移动端的样式错乱，表单没有居中，输入框超出屏幕宽度"
        title:
          type: string
          description: |
            用户已填写的 Issue 标题（可选）。
            提供时 AI 会参考标题上下文生成更精准的内容。
          maxLength: 200
          example: "修复移动端登录页面布局"
        context:
          type: string
          description: |
            额外上下文信息（可选）。
            如相关文件路径、技术栈说明等，帮助 AI 生成更准确的内容。
          maxLength: 1000
          example: "前端使用 React + TailwindCSS，登录页面在 src/pages/Login.tsx"

    # ==================== Entity Schemas ====================

    KanbanIssue:
      type: object
      description: 看板中的 Issue 卡片数据
      properties:
        iid:
          type: integer
          description: Issue 在仓库中的编号
          example: 42
        title:
          type: string
          description: Issue 标题
          example: "修复登录页面在移动端的样式错乱"
        state:
          type: string
          description: Issue 状态
          enum: [opened, closed]
          example: "opened"
        labels:
          type: array
          description: 标签列表
          items:
            type: string
          example: ["bug", "frontend"]
        author:
          $ref: '#/components/schemas/PlatformUser'
        assignees:
          type: array
          description: 指派人列表
          items:
            $ref: '#/components/schemas/PlatformUser'
        created_at:
          type: string
          format: date-time
          description: 创建时间（ISO 8601）
          example: "2025-06-01T10:30:00Z"
        updated_at:
          type: string
          format: date-time
          description: 最后更新时间（ISO 8601）
          example: "2025-06-02T14:20:00Z"
        web_url:
          type: string
          format: uri
          description: Issue 在 GitLab/GitHub 上的 Web 链接
          example: "https://gitlab.com/group/project/-/issues/42"
        mr_count:
          type: integer
          description: 关联的 MR/PR 数量（仅处理中列有值）
          nullable: true
          example: 1

    KanbanMergeRequest:
      type: object
      description: 看板 PR 列中的 MR/PR 卡片数据
      properties:
        iid:
          type: integer
          description: MR/PR 在仓库中的编号
          example: 15
        title:
          type: string
          description: MR/PR 标题
          example: "fix: 修复移动端登录页面布局"
        state:
          type: string
          description: MR/PR 状态
          enum: [opened, closed, merged]
          example: "opened"
        author:
          $ref: '#/components/schemas/PlatformUser'
        source_branch:
          type: string
          description: 源分支
          example: "fix/mobile-login-layout"
        target_branch:
          type: string
          description: 目标分支
          example: "main"
        ci_status:
          type: string
          description: |
            CI/CD 流水线状态：
            - pending: 等待执行
            - running: 执行中
            - success: 通过
            - failed: 失败
            - canceled: 已取消
            - null: 无流水线
          enum: [pending, running, success, failed, canceled]
          nullable: true
          example: "success"
        review_status:
          type: string
          description: |
            Review 状态（聚合）：
            - pending: 等待 review
            - approved: 已批准
            - changes_requested: 需要修改
            - null: 无 reviewer
          enum: [pending, approved, changes_requested]
          nullable: true
          example: "approved"
        related_issue_iids:
          type: array
          description: 关联的 Issue 编号列表
          items:
            type: integer
          example: [42, 43]
        created_at:
          type: string
          format: date-time
          description: 创建时间（ISO 8601）
          example: "2025-06-02T16:00:00Z"
        updated_at:
          type: string
          format: date-time
          description: 最后更新时间（ISO 8601）
          example: "2025-06-03T09:15:00Z"
        web_url:
          type: string
          format: uri
          description: MR/PR 在 GitLab/GitHub 上的 Web 链接
          example: "https://gitlab.com/group/project/-/merge_requests/15"

    IssueDetail:
      type: object
      description: Issue 完整详情
      properties:
        iid:
          type: integer
          description: Issue 在仓库中的编号
          example: 42
        title:
          type: string
          description: Issue 标题
          example: "修复登录页面在移动端的样式错乱"
        description:
          type: string
          description: Issue 描述（Markdown 原文）
          nullable: true
          example: "## 描述\n\n登录页面在移动端布局错乱..."
        state:
          type: string
          description: Issue 状态
          enum: [opened, closed]
          example: "opened"
        labels:
          type: array
          description: 标签列表
          items:
            type: string
          example: ["bug", "frontend", "symphony-claimed"]
        author:
          $ref: '#/components/schemas/PlatformUser'
        assignees:
          type: array
          description: 指派人列表
          items:
            $ref: '#/components/schemas/PlatformUser'
        milestone:
          type: string
          description: 里程碑名称
          nullable: true
          example: "v2.0"
        created_at:
          type: string
          format: date-time
          description: 创建时间（ISO 8601）
          example: "2025-06-01T10:30:00Z"
        updated_at:
          type: string
          format: date-time
          description: 最后更新时间（ISO 8601）
          example: "2025-06-02T14:20:00Z"
        closed_at:
          type: string
          format: date-time
          description: 关闭时间（仅 closed 状态有值）
          nullable: true
          example: null
        web_url:
          type: string
          format: uri
          description: Issue 在 GitLab/GitHub 上的 Web 链接
          example: "https://gitlab.com/group/project/-/issues/42"
        comment_count:
          type: integer
          description: 评论数量
          example: 5
        related_mrs:
          type: array
          description: 关联的 MR/PR 摘要列表
          items:
            $ref: '#/components/schemas/MergeRequestSummary'

    MergeRequestSummary:
      type: object
      description: MR/PR 摘要信息（用于 Issue 详情中的关联展示）
      properties:
        iid:
          type: integer
          description: MR/PR 编号
          example: 15
        title:
          type: string
          description: MR/PR 标题
          example: "fix: 修复移动端登录页面布局"
        state:
          type: string
          enum: [opened, closed, merged]
          example: "opened"
        author:
          $ref: '#/components/schemas/PlatformUser'
        web_url:
          type: string
          format: uri
          example: "https://gitlab.com/group/project/-/merge_requests/15"

    MergeRequestDetail:
      type: object
      description: MR/PR 完整详情
      properties:
        iid:
          type: integer
          description: MR/PR 在仓库中的编号
          example: 15
        title:
          type: string
          description: MR/PR 标题
          example: "fix: 修复移动端登录页面布局"
        description:
          type: string
          description: MR/PR 描述（Markdown 原文）
          nullable: true
          example: "Closes #42\n\n修复了移动端登录页面的布局问题..."
        state:
          type: string
          description: MR/PR 状态
          enum: [opened, closed, merged]
          example: "opened"
        author:
          $ref: '#/components/schemas/PlatformUser'
        source_branch:
          type: string
          description: 源分支
          example: "fix/mobile-login-layout"
        target_branch:
          type: string
          description: 目标分支
          example: "main"
        ci_status:
          type: string
          description: CI/CD 流水线状态
          enum: [pending, running, success, failed, canceled]
          nullable: true
          example: "success"
        ci_web_url:
          type: string
          format: uri
          description: CI/CD 流水线详情链接
          nullable: true
          example: "https://gitlab.com/group/project/-/pipelines/12345"
        review_status:
          type: string
          description: Review 聚合状态
          enum: [pending, approved, changes_requested]
          nullable: true
          example: "approved"
        reviewers:
          type: array
          description: Reviewer 列表及其审核状态
          items:
            $ref: '#/components/schemas/Reviewer'
        merge_status:
          type: string
          description: |
            合并就绪状态：
            - can_be_merged: 可以合并（无冲突）
            - cannot_be_merged: 存在冲突
            - checking: 检查中
            - unchecked: 未检查
          enum: [can_be_merged, cannot_be_merged, checking, unchecked]
          example: "can_be_merged"
        related_issues:
          type: array
          description: 关联的 Issue 列表
          items:
            $ref: '#/components/schemas/IssueSummary'
        additions:
          type: integer
          description: 新增行数
          example: 45
        deletions:
          type: integer
          description: 删除行数
          example: 12
        changed_files:
          type: integer
          description: 变更文件数
          example: 3
        created_at:
          type: string
          format: date-time
          description: 创建时间（ISO 8601）
          example: "2025-06-02T16:00:00Z"
        updated_at:
          type: string
          format: date-time
          description: 最后更新时间（ISO 8601）
          example: "2025-06-03T09:15:00Z"
        merged_at:
          type: string
          format: date-time
          description: 合并时间（仅 merged 状态有值）
          nullable: true
          example: null
        web_url:
          type: string
          format: uri
          description: MR/PR 在 GitLab/GitHub 上的 Web 链接
          example: "https://gitlab.com/group/project/-/merge_requests/15"

    IssueSummary:
      type: object
      description: Issue 摘要信息（用于 MR/PR 详情中的关联展示）
      properties:
        iid:
          type: integer
          description: Issue 编号
          example: 42
        title:
          type: string
          description: Issue 标题
          example: "修复登录页面在移动端的样式错乱"
        state:
          type: string
          enum: [opened, closed]
          example: "opened"
        web_url:
          type: string
          format: uri
          example: "https://gitlab.com/group/project/-/issues/42"

    PlatformUser:
      type: object
      description: GitLab/GitHub 平台用户信息
      properties:
        username:
          type: string
          description: 平台用户名
          example: "zhangsan"
        display_name:
          type: string
          description: 显示名称
          nullable: true
          example: "张三"
        avatar_url:
          type: string
          format: uri
          description: 头像 URL
          nullable: true
          example: "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"

    Reviewer:
      type: object
      description: MR/PR 审核人及其审核状态
      properties:
        user:
          $ref: '#/components/schemas/PlatformUser'
        state:
          type: string
          description: |
            审核状态：
            - pending: 等待审核
            - approved: 已批准
            - changes_requested: 要求修改
          enum: [pending, approved, changes_requested]
          example: "approved"

    # ==================== SSE Event Schemas ====================

    SSEChunkEvent:
      type: object
      description: AI 生成的文本片段事件
      required: [type, content]
      properties:
        type:
          type: string
          enum: [chunk]
          description: 事件类型：文本片段
        content:
          type: string
          description: 生成的文本片段（增量）
          example: "## 描述\n\n"

    SSEDoneEvent:
      type: object
      description: AI 生成完成事件
      required: [type, content]
      properties:
        type:
          type: string
          enum: [done]
          description: 事件类型：生成完成
        content:
          type: string
          description: 完整的生成内容（所有 chunk 拼接结果）
          example: "## 描述\n\n登录页面在移动端...\n\n## Acceptance Criteria\n\n- [ ] ..."

    SSEErrorEvent:
      type: object
      description: AI 生成错误事件（流中途出错）
      required: [type, error]
      properties:
        type:
          type: string
          enum: [error]
          description: 事件类型：错误
        error:
          type: string
          description: 错误描述
          example: "AI 服务响应超时，请重试"
        retCode:
          type: string
          description: 错误码
          example: "EXT_001"

    SSEStream:
      type: object
      description: |
        SSE 流式响应格式说明。

        连接建立后，服务端按以下格式推送事件：

        ```
        data: {"type": "chunk", "content": "## 描述\n\n"}\n\n
        data: {"type": "chunk", "content": "登录页面在移动端..."}\n\n
        data: {"type": "chunk", "content": "布局错乱，表单..."}\n\n
        ...
        data: {"type": "done", "content": "<完整内容>"}\n\n
        ```

        错误情况（流中途出错）：
        ```
        data: {"type": "chunk", "content": "## 描述\n\n"}\n\n
        data: {"type": "error", "error": "AI 服务响应超时", "retCode": "EXT_001"}\n\n
        ```

        客户端断开连接时，服务端应立即停止 AI 生成（取消 Azure OpenAI 请求）。
      properties:
        events:
          type: array
          description: 事件序列（仅用于文档说明，实际为流式推送）
          items:
            oneOf:
              - $ref: '#/components/schemas/SSEChunkEvent'
              - $ref: '#/components/schemas/SSEDoneEvent'
              - $ref: '#/components/schemas/SSEErrorEvent'

    # ==================== Kanban Response ====================

    KanbanData:
      type: object
      description: 看板数据（三列）
      properties:
        todo:
          type: object
          description: 待处理列
          properties:
            issues:
              type: array
              description: 待处理 Issue 列表（无 symphony-claimed label 的 open issues）
              items:
                $ref: '#/components/schemas/KanbanIssue'
            total_count:
              type: integer
              description: 待处理 Issue 总数（可能超过返回数量）
              example: 128
            has_more:
              type: boolean
              description: 是否还有更多（用于前端加载更多）
              example: true
        in_progress:
          type: object
          description: 处理中列
          properties:
            issues:
              type: array
              description: 处理中 Issue 列表（有 symphony-claimed label 的 open issues）
              items:
                $ref: '#/components/schemas/KanbanIssue'
            total_count:
              type: integer
              description: 处理中 Issue 总数
              example: 5
        pr:
          type: object
          description: PR 列
          properties:
            merge_requests:
              type: array
              description: 与处理中 issues 关联的 MR/PR 列表
              items:
                $ref: '#/components/schemas/KanbanMergeRequest'
            total_count:
              type: integer
              description: MR/PR 总数
              example: 3
        cached:
          type: boolean
          description: 本次响应是否来自缓存
          example: false
        cached_at:
          type: string
          format: date-time
          description: 缓存时间（仅 cached=true 时有值）
          nullable: true
          example: null

    # ==================== Response Wrappers ====================

    KanbanResponse:
      type: object
      description: 看板数据响应
      required: [data, success, retCode, retMsg]
      properties:
        data:
          $ref: '#/components/schemas/KanbanData'
        success:
          type: boolean
          example: true
        retCode:
          type: string
          example: "0"
        retMsg:
          type: string
          example: "success"
        showType:
          type: integer
          enum: [0, 1, 2, 4, 9]
          example: 0

    IssueResponse:
      type: object
      description: Issue 创建成功响应
      required: [data, success, retCode, retMsg]
      properties:
        data:
          $ref: '#/components/schemas/IssueDetail'
        success:
          type: boolean
          example: true
        retCode:
          type: string
          example: "0"
        retMsg:
          type: string
          example: "success"
        showType:
          type: integer
          enum: [0, 1, 2, 4, 9]
          example: 0

    IssueDetailResponse:
      type: object
      description: Issue 详情响应
      required: [data, success, retCode, retMsg]
      properties:
        data:
          $ref: '#/components/schemas/IssueDetail'
        success:
          type: boolean
          example: true
        retCode:
          type: string
          example: "0"
        retMsg:
          type: string
          example: "success"
        showType:
          type: integer
          enum: [0, 1, 2, 4, 9]
          example: 0

    MergeRequestListResponse:
      type: object
      description: MR/PR 列表响应
      required: [data, success, retCode, retMsg]
      properties:
        data:
          type: array
          items:
            $ref: '#/components/schemas/MergeRequestSummary'
        success:
          type: boolean
          example: true
        retCode:
          type: string
          example: "0"
        retMsg:
          type: string
          example: "success"
        showType:
          type: integer
          enum: [0, 1, 2, 4, 9]
          example: 0

    MergeRequestDetailResponse:
      type: object
      description: MR/PR 详情响应
      required: [data, success, retCode, retMsg]
      properties:
        data:
          $ref: '#/components/schemas/MergeRequestDetail'
        success:
          type: boolean
          example: true
        retCode:
          type: string
          example: "0"
        retMsg:
          type: string
          example: "success"
        showType:
          type: integer
          enum: [0, 1, 2, 4, 9]
          example: 0

    # ==================== Error Response Schemas ====================

    ErrorResponseAuth001:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "AUTH_001"
        retMsg:
          type: string
          example: "未登录或 Token 已过期"
        showType:
          type: integer
          example: 9

    ErrorResponseAuth002:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "AUTH_002"
        retMsg:
          type: string
          example: "权限不足"
        showType:
          type: integer
          example: 2

    ErrorResponseBiz001:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "BIZ_001"
        retMsg:
          type: string
          example: "请先在个人设置中配置 GitLab Token"
        showType:
          type: integer
          example: 1

    ErrorResponseBiz002:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "BIZ_002"
        retMsg:
          type: string
          example: "项目不存在"
        showType:
          type: integer
          example: 2

    ErrorResponseExt001:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "EXT_001"
        retMsg:
          type: string
          example: "GitLab API 不可用，请稍后重试"
        showType:
          type: integer
          example: 4

    ErrorResponseExt002:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "EXT_002"
        retMsg:
          type: string
          example: "AI 生成请求过于频繁，请稍后重试（限制：10次/分钟）"
        showType:
          type: integer
          example: 1

    ErrorResponseSys001:
      type: object
      required: [data, success, retCode, retMsg]
      properties:
        data:
          nullable: true
          example: null
        success:
          type: boolean
          example: false
        retCode:
          type: string
          example: "SYS_001"
        retMsg:
          type: string
          example: "系统内部错误"
        showType:
          type: integer
          example: 2
```

---

## 接口详细说明

### 1. GET /api/projects/:id/kanban - 获取看板数据

**请求示例**：
```http
GET /api/projects/1/kanban?todo_limit=50&labels=bug HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...
```

**成功响应示例**：
```json
{
  "data": {
    "todo": {
      "issues": [
        {
          "iid": 42,
          "title": "修复登录页面在移动端的样式错乱",
          "state": "opened",
          "labels": ["bug", "frontend"],
          "author": {
            "username": "zhangsan",
            "display_name": "张三",
            "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"
          },
          "assignees": [],
          "created_at": "2025-06-01T10:30:00Z",
          "updated_at": "2025-06-02T14:20:00Z",
          "web_url": "https://gitlab.com/group/project/-/issues/42",
          "mr_count": null
        }
      ],
      "total_count": 128,
      "has_more": true
    },
    "in_progress": {
      "issues": [
        {
          "iid": 38,
          "title": "实现用户注册功能",
          "state": "opened",
          "labels": ["feature", "symphony-claimed"],
          "author": {
            "username": "lisi",
            "display_name": "李四",
            "avatar_url": null
          },
          "assignees": [
            {
              "username": "lisi",
              "display_name": "李四",
              "avatar_url": null
            }
          ],
          "created_at": "2025-05-28T09:00:00Z",
          "updated_at": "2025-06-03T11:00:00Z",
          "web_url": "https://gitlab.com/group/project/-/issues/38",
          "mr_count": 1
        }
      ],
      "total_count": 2
    },
    "pr": {
      "merge_requests": [
        {
          "iid": 15,
          "title": "feat: 实现用户注册功能",
          "state": "opened",
          "author": {
            "username": "symphony-bot",
            "display_name": "Symphony Bot",
            "avatar_url": null
          },
          "source_branch": "feat/user-registration",
          "target_branch": "main",
          "ci_status": "success",
          "review_status": "pending",
          "related_issue_iids": [38],
          "created_at": "2025-06-03T08:00:00Z",
          "updated_at": "2025-06-03T10:30:00Z",
          "web_url": "https://gitlab.com/group/project/-/merge_requests/15"
        }
      ],
      "total_count": 1
    },
    "cached": false,
    "cached_at": null
  },
  "success": true,
  "retCode": "0",
  "retMsg": "success",
  "showType": 0
}
```

**Token 未配置时的错误响应**：
```json
{
  "data": null,
  "success": false,
  "retCode": "BIZ_001",
  "retMsg": "请先在个人设置中配置 GitLab Token",
  "showType": 1
}
```

**外部 API 不可用时的错误响应**：
```json
{
  "data": null,
  "success": false,
  "retCode": "EXT_001",
  "retMsg": "GitLab API 请求超时，请稍后重试",
  "showType": 4
}
```

---

### 2. POST /api/projects/:id/issues - 创建 Issue

**请求示例**：
```http
POST /api/projects/1/issues HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...
Content-Type: application/json

{
  "title": "修复登录页面在移动端的样式错乱",
  "description": "## 描述\n\n登录页面在移动端（宽度 < 768px）时布局错乱，表单没有居中，输入框超出屏幕宽度。\n\n## Acceptance Criteria\n\n- [ ] 移动端登录表单居中显示\n- [ ] 输入框宽度自适应，不超出视口\n- [ ] 按钮宽度与输入框一致\n\n## Validation\n\n- [ ] 响应式测试: `npx playwright test --project=mobile`\n- [ ] 视觉回归: `npx playwright test --project=mobile --update-snapshots`\n\n## Notes\n\n- 使用 TailwindCSS 的响应式断点",
  "labels": ["bug", "frontend"],
  "assignee": "zhangsan"
}
```

**成功响应示例**：
```json
{
  "data": {
    "iid": 43,
    "title": "修复登录页面在移动端的样式错乱",
    "description": "## 描述\n\n登录页面在移动端...",
    "state": "opened",
    "labels": ["bug", "frontend"],
    "author": {
      "username": "zhangsan",
      "display_name": "张三",
      "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"
    },
    "assignees": [
      {
        "username": "zhangsan",
        "display_name": "张三",
        "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"
      }
    ],
    "milestone": null,
    "created_at": "2025-06-04T10:00:00Z",
    "updated_at": "2025-06-04T10:00:00Z",
    "closed_at": null,
    "web_url": "https://gitlab.com/group/project/-/issues/43",
    "comment_count": 0,
    "related_mrs": []
  },
  "success": true,
  "retCode": "0",
  "retMsg": "success",
  "showType": 0
}
```

---

### 3. POST /api/projects/:id/issues/ai-generate - AI 辅助生成 Issue 内容

**请求示例**：
```http
POST /api/projects/1/issues/ai-generate HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...
Content-Type: application/json

{
  "prompt": "修复登录页面在移动端的样式错乱，表单没有居中，输入框超出屏幕宽度",
  "title": "修复移动端登录页面布局",
  "context": "前端使用 React + TailwindCSS，登录页面在 src/pages/Login.tsx"
}
```

**SSE 响应示例**：
```
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
X-Accel-Buffering: no

data: {"type":"chunk","content":"## 描述\n\n"}

data: {"type":"chunk","content":"登录页面在移动端（视口宽度 < 768px）时存在布局问题："}

data: {"type":"chunk","content":"表单容器未居中显示，输入框宽度超出屏幕可视区域，"}

data: {"type":"chunk","content":"导致用户需要横向滚动才能完成登录操作。\n\n"}

data: {"type":"chunk","content":"## Acceptance Criteria\n\n"}

data: {"type":"chunk","content":"- [ ] 移动端（< 768px）登录表单水平居中显示\n"}

data: {"type":"chunk","content":"- [ ] 输入框宽度自适应，不超出视口宽度（含 padding）\n"}

data: {"type":"chunk","content":"- [ ] 提交按钮宽度与输入框保持一致\n"}

data: {"type":"chunk","content":"- [ ] 表单在 320px ~ 768px 范围内均正常显示\n\n"}

data: {"type":"chunk","content":"## Validation\n\n"}

data: {"type":"chunk","content":"- [ ] 响应式测试通过: `npx playwright test tests/login.spec.ts --project=mobile`\n"}

data: {"type":"chunk","content":"- [ ] 视觉无回归: `npx playwright test tests/login.spec.ts --update-snapshots`\n\n"}

data: {"type":"chunk","content":"## Notes\n\n"}

data: {"type":"chunk","content":"- 建议使用 TailwindCSS 的 `max-w-sm` 或 `w-full` 配合 `px-4` 实现自适应\n"}

data: {"type":"chunk","content":"- 参考文件: `src/pages/Login.tsx`\n"}

data: {"type":"done","content":"## 描述\n\n登录页面在移动端（视口宽度 < 768px）时存在布局问题：表单容器未居中显示，输入框宽度超出屏幕可视区域，导致用户需要横向滚动才能完成登录操作。\n\n## Acceptance Criteria\n\n- [ ] 移动端（< 768px）登录表单水平居中显示\n- [ ] 输入框宽度自适应，不超出视口宽度（含 padding）\n- [ ] 提交按钮宽度与输入框保持一致\n- [ ] 表单在 320px ~ 768px 范围内均正常显示\n\n## Validation\n\n- [ ] 响应式测试通过: `npx playwright test tests/login.spec.ts --project=mobile`\n- [ ] 视觉无回归: `npx playwright test tests/login.spec.ts --update-snapshots`\n\n## Notes\n\n- 建议使用 TailwindCSS 的 `max-w-sm` 或 `w-full` 配合 `px-4` 实现自适应\n- 参考文件: `src/pages/Login.tsx`\n"}

```

**流中途错误示例**：
```
data: {"type":"chunk","content":"## 描述\n\n"}

data: {"type":"chunk","content":"登录页面在移动端..."}

data: {"type":"error","error":"AI 服务响应超时，请重试","retCode":"EXT_001"}

```

**速率限制错误响应**（非 SSE，直接返回 JSON）：
```json
{
  "data": null,
  "success": false,
  "retCode": "EXT_002",
  "retMsg": "AI 生成请求过于频繁，请稍后重试（限制：10次/分钟）",
  "showType": 1
}
```

---

### 4. GET /api/projects/:id/issues/:iid - Issue 详情

**请求示例**：
```http
GET /api/projects/1/issues/42 HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...
```

**成功响应示例**：
```json
{
  "data": {
    "iid": 42,
    "title": "修复登录页面在移动端的样式错乱",
    "description": "## 描述\n\n登录页面在移动端（宽度 < 768px）时布局错乱...",
    "state": "opened",
    "labels": ["bug", "frontend", "symphony-claimed"],
    "author": {
      "username": "zhangsan",
      "display_name": "张三",
      "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"
    },
    "assignees": [
      {
        "username": "zhangsan",
        "display_name": "张三",
        "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"
      }
    ],
    "milestone": "v2.0",
    "created_at": "2025-06-01T10:30:00Z",
    "updated_at": "2025-06-03T11:00:00Z",
    "closed_at": null,
    "web_url": "https://gitlab.com/group/project/-/issues/42",
    "comment_count": 5,
    "related_mrs": [
      {
        "iid": 15,
        "title": "fix: 修复移动端登录页面布局",
        "state": "opened",
        "author": {
          "username": "symphony-bot",
          "display_name": "Symphony Bot",
          "avatar_url": null
        },
        "web_url": "https://gitlab.com/group/project/-/merge_requests/15"
      }
    ]
  },
  "success": true,
  "retCode": "0",
  "retMsg": "success",
  "showType": 0
}
```

---

### 5. GET /api/projects/:id/issues/:iid/mrs - Issue 关联的 MR/PR

**请求示例**：
```http
GET /api/projects/1/issues/42/mrs HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...
```

**成功响应示例**：
```json
{
  "data": [
    {
      "iid": 15,
      "title": "fix: 修复移动端登录页面布局",
      "state": "opened",
      "author": {
        "username": "symphony-bot",
        "display_name": "Symphony Bot",
        "avatar_url": null
      },
      "web_url": "https://gitlab.com/group/project/-/merge_requests/15"
    }
  ],
  "success": true,
  "retCode": "0",
  "retMsg": "success",
  "showType": 0
}
```

---

### 6. GET /api/projects/:id/mrs/:iid - MR/PR 详情

**请求示例**：
```http
GET /api/projects/1/mrs/15 HTTP/1.1
Authorization: Bearer eyJhbGciOiJIUzI1NiJ9...
```

**成功响应示例**：
```json
{
  "data": {
    "iid": 15,
    "title": "fix: 修复移动端登录页面布局",
    "description": "Closes #42\n\n修复了移动端登录页面的布局问题，使用 TailwindCSS 响应式类实现自适应。",
    "state": "opened",
    "author": {
      "username": "symphony-bot",
      "display_name": "Symphony Bot",
      "avatar_url": null
    },
    "source_branch": "fix/mobile-login-layout",
    "target_branch": "main",
    "ci_status": "success",
    "ci_web_url": "https://gitlab.com/group/project/-/pipelines/12345",
    "review_status": "approved",
    "reviewers": [
      {
        "user": {
          "username": "zhangsan",
          "display_name": "张三",
          "avatar_url": "https://gitlab.com/uploads/-/system/user/avatar/1/avatar.png"
        },
        "state": "approved"
      }
    ],
    "merge_status": "can_be_merged",
    "related_issues": [
      {
        "iid": 42,
        "title": "修复登录页面在移动端的样式错乱",
        "state": "opened",
        "web_url": "https://gitlab.com/group/project/-/issues/42"
      }
    ],
    "additions": 45,
    "deletions": 12,
    "changed_files": 3,
    "created_at": "2025-06-02T16:00:00Z",
    "updated_at": "2025-06-03T09:15:00Z",
    "merged_at": null,
    "web_url": "https://gitlab.com/group/project/-/merge_requests/15"
  },
  "success": true,
  "retCode": "0",
  "retMsg": "success",
  "showType": 0
}
```

---

## SSE 协议详细说明

### 连接管理

| 项目 | 说明 |
|------|------|
| Content-Type | `text/event-stream` |
| Cache-Control | `no-cache` |
| Connection | `keep-alive` |
| X-Accel-Buffering | `no`（禁用 Nginx 缓冲） |
| 超时 | 60 秒无数据则服务端关闭连接 |
| 客户端断开 | 服务端检测到后立即取消 AI 请求 |

### 事件格式

每个事件为一行 `data: {json}\n\n`，JSON 中通过 `type` 字段区分事件类型：

| type | 说明 | 触发时机 |
|------|------|----------|
| `chunk` | 文本片段（增量） | AI 模型每生成一段文本 |
| `done` | 生成完成 | AI 模型输出结束，包含完整内容 |
| `error` | 生成错误 | AI 服务异常、超时等 |

### 前端处理建议

```typescript
const eventSource = new EventSource('/api/projects/1/issues/ai-generate', {
  // 注意：标准 EventSource 不支持 POST，需使用 fetch + ReadableStream
});

// 推荐使用 fetch 实现：
async function generateIssue(projectId: number, request: AIGenerateRequest) {
  const response = await fetch(`/api/projects/${projectId}/issues/ai-generate`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${token}`,
    },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    // 非 SSE 错误（如 429 速率限制），直接解析 JSON
    const error = await response.json();
    throw new Error(error.retMsg);
  }

  const reader = response.body!.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let fullContent = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';

    for (const line of lines) {
      if (line.startsWith('data: ')) {
        const data = JSON.parse(line.slice(6));
        switch (data.type) {
          case 'chunk':
            fullContent += data.content;
            onChunk(data.content);  // 更新 UI
            break;
          case 'done':
            onComplete(data.content);  // 生成完成
            return;
          case 'error':
            onError(data.error, data.retCode);  // 处理错误
            return;
        }
      }
    }
  }
}
```

### 中断生成

客户端可通过 `AbortController` 中断请求：

```typescript
const controller = new AbortController();

fetch(url, { signal: controller.signal, ... });

// 用户点击"停止生成"按钮
controller.abort();
```

服务端检测到连接断开后，应立即取消对 Azure OpenAI 的请求，避免不必要的 token 消耗。

---

## AI 生成安全防护

### Prompt Injection 防护策略

| 层级 | 措施 | 说明 |
|------|------|------|
| 输入清洗 | 移除 system/assistant 角色标记 | 过滤 `<\|im_start\|>system` 等注入模式 |
| 长度限制 | prompt ≤ 2000 chars, title ≤ 200 chars | 限制攻击面 |
| System Prompt 隔离 | 明确输出格式约束 | AI 只能输出 Issue 模板格式 |
| 输出验证 | Validation 命令白名单 | 仅允许安全命令前缀 |
| Token 限制 | max_tokens = 4096 | 防止无限输出 |

### Validation 命令白名单

AI 生成的 Validation 部分中的命令，仅允许以下前缀：

```
cargo test
cargo build
cargo clippy
npm test
npm run
npx
yarn test
yarn run
pnpm test
pnpm run
go test
python -m pytest
pytest
make
curl
grep
cat
ls
```

不在白名单中的命令将被标记警告（不阻止生成，但前端展示提示）。

### 速率限制实现

```
Key: ai_generate:{user_id}
Window: 60 seconds (sliding window)
Limit: 10 requests
Storage: 内存（HashMap<UserId, VecDeque<Instant>>）
```

---

## 缓存架构

### Singleflight 模式

```
请求 A ──┐
请求 B ──┼──► singleflight ──► GitLab API ──► 缓存结果
请求 C ──┘         │                              │
                   └── 等待 ◄─────────────────────┘
                   └── 返回缓存结果
```

### 缓存键设计

```
{user_id}:{project_id}:{endpoint}:{query_params_hash}
```

示例：
- `5:1:kanban:abc123` — 用户 5 的项目 1 看板数据
- `5:1:issue:42:detail` — 用户 5 的项目 1 Issue #42 详情
- `5:1:issue:42:mrs` — 用户 5 的项目 1 Issue #42 关联 MR

### 缓存配置

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `kanban_cache_ttl_seconds` | 10 | 看板数据缓存 TTL |
| `detail_cache_ttl_seconds` | 5 | 详情数据缓存 TTL |
| `empty_cache_ttl_seconds` | 3 | 空结果缓存 TTL（防穿透） |
| `max_cache_entries` | 10000 | 最大缓存条目数 |
| `cache_cleanup_interval_seconds` | 60 | 过期清理间隔 |

### 缓存失效

- TTL 过期自动失效
- 用户创建 Issue 后，主动清除该项目的看板缓存
- 用户可通过前端"刷新"按钮强制绕过缓存（请求头 `Cache-Control: no-cache`）

---

## 幂等性与并发安全

| 方法 | 幂等 | 说明 |
|------|------|------|
| GET /kanban | 是 | 只读，幂等 |
| GET /issues/:iid | 是 | 只读，幂等 |
| GET /issues/:iid/mrs | 是 | 只读，幂等 |
| GET /mrs/:iid | 是 | 只读，幂等 |
| POST /issues | 否 | 每次创建新 Issue |
| POST /issues/ai-generate | 否 | 每次生成新内容（但不产生副作用） |

### 并发安全

- singleflight 保证同一缓存键的并发请求只触发一次外部调用
- AI 生成接口无共享状态，天然并发安全
- Issue 创建直接调用外部 API，由 GitLab/GitHub 保证一致性

---

## 后端实现要点

### 环境变量

| 变量名 | 必填 | 说明 |
|--------|------|------|
| `AZURE_OPENAI_BASEURL` | 是（AI 功能） | Azure OpenAI 端点 |
| `AZURE_OPENAI_API_KEY` | 是（AI 功能） | Azure OpenAI API Key |
| `AZURE_OPENAI_MODEL` | 否 | 模型名称，默认 `gpt-5.5` |
| `AI_MODEL_FAMILY` | 否 | 模型参数兼容族，可选 `gpt5` / `legacy`；Azure 自定义部署名指向 GPT-5/推理模型时设为 `gpt5` |
| `AI_MAX_TOKENS` | 否 | AI 输出最大 token 数，默认 4096 |
| `AI_RATE_LIMIT_PER_MINUTE` | 否 | AI 速率限制，默认 10 |
| `KANBAN_CACHE_TTL` | 否 | 看板缓存 TTL（秒），默认 10 |

### Rust 模块结构建议

```
web-platform/src/
├── handlers/
│   ├── kanban.rs          # GET /kanban 处理
│   ├── issues.rs          # POST /issues, GET /issues/:iid
│   ├── issue_mrs.rs       # GET /issues/:iid/mrs
│   ├── merge_requests.rs  # GET /mrs/:iid
│   └── ai_generate.rs     # POST /issues/ai-generate (SSE)
├── services/
│   ├── git_platform.rs    # GitLab/GitHub API 统一抽象层
│   ├── gitlab_client.rs   # GitLab API 客户端
│   ├── github_client.rs   # GitHub API 客户端
│   ├── ai_service.rs      # Azure OpenAI 客户端
│   └── cache.rs           # Singleflight 内存缓存
├── models/
│   ├── kanban.rs          # 看板相关数据结构
│   ├── issue.rs           # Issue 相关数据结构
│   └── merge_request.rs   # MR/PR 相关数据结构
└── middleware/
    └── rate_limit.rs      # 速率限制中间件
```

### GitLab/GitHub 统一抽象

```rust
#[async_trait]
pub trait GitPlatformClient: Send + Sync {
    /// 获取 open issues（支持 label 过滤）
    async fn list_issues(
        &self,
        token: &str,
        labels: Option<&[String]>,
        exclude_labels: Option<&[String]>,
        limit: u32,
    ) -> Result<Vec<PlatformIssue>>;

    /// 获取单个 issue 详情
    async fn get_issue(&self, token: &str, iid: u64) -> Result<PlatformIssue>;

    /// 创建 issue
    async fn create_issue(&self, token: &str, req: &CreateIssueRequest) -> Result<PlatformIssue>;

    /// 获取 issue 关联的 MR/PR
    async fn get_issue_merge_requests(&self, token: &str, issue_iid: u64) -> Result<Vec<PlatformMergeRequest>>;

    /// 获取 MR/PR 详情
    async fn get_merge_request(&self, token: &str, mr_iid: u64) -> Result<PlatformMergeRequest>;
}
```

---

## Review 修订记录（基于两轮对抗验证）

### 关键设计决策

| 决策 | 结论 | 原因 |
|------|------|------|
| 成功响应 `showType` | 不返回（省略） | 与 Phase 1/2 `skip_serializing_if` 行为一致 |
| 成功响应 `retMsg` | 固定为 `"ok"` | 与 Phase 1/2 现有实现一致 |
| POST /issues 状态码 | 200 | 与 Phase 1/2 create_project 保持一致 |
| 实体字段命名 | snake_case | 数据来源于 GitLab/GitHub API，保留原始命名 |
| Token 无效错误 | `TOKEN_001`（新增） | 区分"Token 过期"和"服务不可用"，给用户可操作的提示 |
| AI 并发限制 | 1 个/用户 | 防止资源浪费，第二个请求返回 429 |
| 全局 AI 速率 | 30 次/分钟 | 防止团队耗尽 Azure OpenAI 配额 |

### SSE 连接管理补充

- 服务端每 15s 发送 `: keepalive\n\n`（SSE 注释，防止代理超时）
- 最大生成时间：120s，超时发送 error 事件并关闭
- Azure OpenAI 首 token 超时：30s
- 反向代理配置要求：`proxy_read_timeout 120s`、`proxy_buffering off`

### 看板性能预期

- 首次无缓存请求：2-5s（取决于处理中 Issue 数量，N+1 MR 查询并行执行）
- 缓存命中请求：< 50ms
- PR 列 MR 查询并发上限：10（防止外部 API 限流）
- 建议前端：先渲染 todo/in_progress 列骨架，PR 列可接受延迟加载

### 外部 API 超时配置

| 调用 | 超时 | 失败行为 |
|------|------|---------|
| GitLab/GitHub 单次 API 调用 | 10s | 返回 EXT_001 |
| 看板整体请求 | 30s | 返回已获取的部分数据 + 错误标记 |
| Azure OpenAI 首 token | 30s | 发送 SSE error 事件 |
| Azure OpenAI 后续 token 间隔 | 60s | 发送 SSE error 事件 |

### KanbanData 补充字段

```yaml
KanbanData:
  properties:
    platform:
      type: string
      enum: [gitlab, github]
      description: 项目所属平台（前端据此显示 "MR" 或 "PR"）
```

### 429 响应补充 Retry-After 头

所有 429 响应包含 `Retry-After` 头：
```http
HTTP/1.1 429 Too Many Requests
Retry-After: 30
Content-Type: application/json

{"data": null, "success": false, "retCode": "EXT_002", "retMsg": "请求过于频繁，请 30 秒后重试", "showType": 1}
```
