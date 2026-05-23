# Symphony 网络代理模块与系统配置界面方案

## 背景

当前 Web 平台启动项目服务时，只在 `web-platform/src/process_manager/spawn.rs` 中手工继承了一组代理环境变量：

- `https_proxy`、`http_proxy`、`all_proxy`
- `HTTPS_PROXY`、`HTTP_PROXY`、`ALL_PROXY`
- `no_proxy`、`NO_PROXY`

这段逻辑只能解决“把宿主环境变量顺手传给项目服务”的测试场景，不是完整的网络代理模块。真实运行还需要覆盖 Web 平台自身外部请求、rust-platform 的所有 reqwest client、Codex 子进程、Git/Hook 命令、配置界面、敏感值存储、连通性测试和诊断脱敏。

本方案目标是重新设计网络代理模块，并在系统配置中增加网络代理配置界面。本文只描述方案，不包含实现代码。

## 目标

1. 建立统一的网络代理配置模型，覆盖 Web 平台、rust-platform、Codex 子进程以及 Git/Hook 命令。
2. 支持管理员在系统配置界面中选择代理模式、填写代理地址、维护绕过规则、执行连通性测试。
3. 明确配置优先级和跨进程传递语义，避免环境变量、数据库配置和系统代理互相覆盖。
4. 保护代理凭据，所有 API 响应、日志、诊断信息默认脱敏。
5. 让新启动的项目服务自动使用最新代理配置；已运行服务明确提示重启后生效。
6. 保留现有通用 `system_configs` 管理能力，但代理敏感值不进入通用配置表格和通用配置接口。

## 非目标

- 不实现按项目、按用户、按目标域名的多套代理策略。
- 不在第一阶段做 rust-platform 内部热更新或动态替换已创建的 reqwest client。
- 不自动修改操作系统级代理设置。
- 不把代理 URL 写入 `WORKFLOW.md`，避免敏感信息进入项目工作区。
- 第一阶段不支持 PAC、NTLM/Kerberos、浏览器式复杂 bypass 规则。
- 第一阶段不支持 SOCKS5；如后续支持，必须先启用相关依赖 feature 并补充兼容矩阵。

## 现状边界

### Web 平台

Web 平台负责：

- 管理 `system_configs`。
- 通过 `/api/admin/config` 暴露通用系统配置。
- 启动项目级 `symphony-platform` 子进程。
- 发起 AI、平台 token 校验、GitHub/GitLab、通知测试等外部 HTTP 请求。

当前问题：

- `/api/admin/config` 直接返回 `system_configs.value`。
- 管理端通用配置表会把 value 直接渲染到可编辑输入框。
- `system_configs` 当前更新逻辑不是 upsert，不存在的 key 会返回 NotFound。
- Web 侧存在多个分散的 reqwest client 构造点。

### rust-platform

rust-platform 负责：

- 访问 GitHub/GitLab/Linear 等平台 API。
- 提供 `linear_graphql` 等外部 HTTP 工具。
- 启动 Codex app-server 子进程。
- 执行 workspace hook、Git 命令和 agent 运行流程。

当前问题：

- `platform/http_client.rs`、Linear tracker、`linear_graphql` 等 reqwest 构造点没有统一代理入口。
- reqwest 默认可能读取系统或环境代理；仅移除环境变量不足以表达“禁用代理”。
- `Command` 默认继承父进程环境，禁用代理时如果不显式 `env_remove`，代理变量可能泄漏给 Codex、Hook 和 Git 命令。

## 设计原则

1. **单一事实源**：运行时只使用一份解析后的 `EffectiveProxyConfig`。
2. **显式模式优先**：`disabled`、`manual`、`inherit_env` 必须通过非敏感哨兵变量跨进程传递，不能只靠代理变量是否存在推断。
3. **默认防泄漏**：应用代理环境前必须先移除标准代理变量，再按有效配置注入。
4. **敏感信息隔离**：代理 URL 不进入通用 `/api/admin/config` 明文响应，不进入通用配置表格编辑。
5. **配置损坏 fail closed**：首次安装可继承环境；已有配置损坏不能悄悄回退到继承环境。
6. **第一阶段收窄能力**：优先做全局代理闭环，先不做项目级覆盖和复杂 no_proxy 语义。

## 推荐方案

采用“Web 平台管理全局代理配置 + 共享代理模块 + 显式跨进程模式哨兵”的方案。

Web 平台负责：

- 提供结构化代理配置 API。
- 校验、加密存储和脱敏代理 URL。
- 为 Web 侧 HTTP client factory 提供当前有效代理配置。
- 启动项目服务时注入代理环境变量和非敏感模式哨兵。
- 标记运行中项目服务使用的代理配置版本。

rust-platform 负责：

- 从 `SYMPHONY_PROXY_MODE`、`SYMPHONY_PROXY_VERSION` 和标准代理变量构造 `EffectiveProxyConfig`。
- 对所有 reqwest builder 应用同一代理规则。
- 对 Codex、Git、Hook 等所有子进程使用统一代理环境封装。

建议实现一个共享代理模块或 workspace 级共享 crate。若第一阶段不新增 crate，也必须保证 Web 平台和 rust-platform 使用同一份契约、同一组测试用例和同样的脱敏规则。

## 模块职责

| 职责 | 说明 |
|---|---|
| 配置模型 | 表达代理模式、代理 URL、绕过规则、配置版本 |
| 配置解析 | 从数据库、环境变量和跨进程哨兵生成有效配置 |
| 校验 | 校验 URL scheme、host、port、凭据策略、NO_PROXY 规则 |
| 脱敏 | 对 API 响应、日志、诊断输出隐藏用户名、密码和 query string |
| 环境变量生成 | 生成大小写兼容的 `HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY` |
| 命令封装 | 对 `Command` 先移除代理变量，再按有效配置注入 |
| reqwest 应用 | 对 reqwest builder 应用代理或 `.no_proxy()` |
| 诊断 | 输出模式、来源、版本、脱敏地址、受影响项目和连接测试结果 |

## 运行时数据流

1. Web 平台启动时读取数据库和宿主环境变量。
2. `ProxyConfigProvider` 生成 Web 进程内的 `EffectiveProxyConfig`。
3. Web 平台自身外部 HTTP 请求必须通过统一 client factory 创建。
4. 管理员保存代理配置后，Web 平台刷新配置缓存并递增 `proxyConfigVersion`。
5. Web 平台把所有运行中项目服务标记为代理配置版本落后，UI 显示“重启后生效”。
6. Web 平台启动或重启项目服务时，先清理标准代理变量，再注入规范化代理变量和哨兵变量。
7. rust-platform 启动时必须读取 `SYMPHONY_PROXY_MODE` 和 `SYMPHONY_PROXY_VERSION`；不能只靠代理变量是否存在判断模式。
8. rust-platform 所有 reqwest client、Codex 子进程、Git/Hook 命令都使用同一份有效配置。

## 跨进程环境契约

Web 平台启动 rust-platform 时必须传递以下非敏感变量：

| 变量 | 说明 |
|---|---|
| `SYMPHONY_PROXY_MODE` | `disabled`、`inherit_env`、`manual` |
| `SYMPHONY_PROXY_VERSION` | 当前代理配置版本，用于诊断和重启提示 |
| `SYMPHONY_PROXY_SOURCE` | `system_config`、`environment`、`fallback_disabled` 等脱敏来源 |

标准代理变量由代理模块统一生成：

| 变量 | 说明 |
|---|---|
| `HTTP_PROXY` / `http_proxy` | HTTP 代理 |
| `HTTPS_PROXY` / `https_proxy` | HTTPS 代理 |
| `ALL_PROXY` / `all_proxy` | 兜底代理 |
| `NO_PROXY` / `no_proxy` | 绕过代理规则 |

禁用代理时：

- 仍必须传 `SYMPHONY_PROXY_MODE=disabled`。
- 必须对标准代理变量执行 `env_remove`。
- rust-platform reqwest builder 必须显式 `.no_proxy()`。
- rust-platform 启动的 Codex、Hook、Git 命令也必须移除标准代理变量。

## 配置优先级

从高到低：

1. 数据库中系统配置选择 `disabled`：强制禁用代理。
2. 数据库中系统配置选择 `manual`：使用管理员填写的代理配置。
3. 数据库中系统配置选择 `inherit_env`：继承 Web 平台启动环境中的代理变量。
4. 首次安装且无配置：默认 `inherit_env`，并显示来源为启动环境。
5. 已有配置损坏：fail closed，优先保留上一份有效配置；没有上一份有效配置时进入 `disabled`，并在 UI 显示阻断级告警。

不能把“已有配置损坏”静默回退为 `inherit_env`，否则管理员原本禁用代理后可能因配置损坏重新走宿主代理。

## 配置模型

### 模式

| 模式 | 含义 | 第一阶段行为 |
|---|---|---|
| `disabled` | 禁用代理 | Web 和 rust-platform 都显式禁用代理 |
| `inherit_env` | 继承 Web 平台启动环境 | Web 平台读取启动环境并规范化后使用和传递 |
| `manual` | 使用系统配置界面填写的代理 | Web 平台从加密存储读取并规范化后使用和传递 |

### 手动配置字段

| 字段 | 说明 | 是否敏感 |
|---|---|---|
| HTTP 代理 | 用于 `http://` 请求 | 可能敏感 |
| HTTPS 代理 | 用于 `https://` 请求 | 可能敏感 |
| ALL 代理 | 兜底代理 | 可能敏感 |
| 绕过代理 | `NO_PROXY` 规则，逗号分隔 | 否 |
| 自动绕过本机地址 | 默认包含 `localhost`、`127.0.0.1`、`::1` | 否 |

第一阶段不提供“应用到 Web 平台 / 项目服务 / Codex/Git/Hook”的复选框，避免 UI 能表达但运行时无法可靠执行的半生效状态。第一阶段全链路生效。未来如果需要细分范围，必须增加非敏感控制变量，例如 `SYMPHONY_PROXY_APPLY_CHILD_PROCESSES=false`，并同步改造 rust-platform。

### URL 支持范围

第一阶段支持：

- `http://host:port`
- `https://host:port`
- 含用户名密码的 URL，但必须加密存储，并且所有展示和日志脱敏

第一阶段不支持：

- `socks5://`。后续如支持，Web 平台和 rust-platform 的 reqwest 依赖都必须启用 `socks` feature，并补 Codex/Git/CLI 兼容矩阵。
- PAC 文件。
- NTLM/Kerberos 系统级认证。
- 按域名匹配多个代理。

### `NO_PROXY` 规则

第一阶段支持最小通用子集：

- 普通域名，例如 `gitlab.internal`。第一阶段按 reqwest 实际 `NO_PROXY` 语义验收，域名可能匹配自身及子域名；若产品需要严格“仅精确匹配”，必须实现自定义 matcher。
- 域名后缀，例如 `.example.com`
- IP 地址，例如 `127.0.0.1`
- CIDR，例如 `10.0.0.0/8`
- 星号 `*` 表示全部绕过

第一阶段不支持端口限定规则，例如 `localhost:3000`。原因是 reqwest 的 no_proxy 匹配按 host/IP/域名处理，不保证端口限定生效。UI 保存时应拒绝 `host:port` 形式，除非未来实现自定义 proxy matcher。

默认自动包含：

- `localhost`
- `127.0.0.1`
- `::1`

## 配置存储设计

### 存储边界

| 类型 | 存储位置 | 是否进入 `/api/admin/config` |
|---|---|---|
| 非敏感配置 | `system_configs` 或专用普通配置表 | 只能通过结构化 API 管理；通用配置接口完全隐藏 `network_proxy.*` |
| 敏感代理 URL | 加密配置表或加密字段 | 不允许 |

硬性约束：

- `network_proxy.http_url`、`network_proxy.https_url`、`network_proxy.all_url` 不得以明文形式存入 `system_configs`。
- 通用 `/api/admin/config` 的 GET 必须完全过滤所有 `network_proxy.*` key，不返回只读摘要，避免现有通用表格误渲染成可编辑项。
- 管理端通用 key/value 表不得编辑代理 URL。
- 代理敏感值只能通过 `/api/admin/network-proxy` 结构化 API 写入和脱敏读取。
- 通用 `/api/admin/config` 的更新接口必须拒绝所有 `network_proxy.*` key，不能绕过结构化 API 修改 mode、NO_PROXY、version 或 secret。
- 通用 `/api/admin/config` 的所有响应路径都必须应用同一过滤规则，包括 PUT 成功后的返回体；若 PUT 仍返回配置列表，返回前必须过滤所有 `network_proxy.*`，或改为保存后由前端重新调用过滤后的 GET。
- 如果历史数据库中已经误写入 `network_proxy.http_url`、`network_proxy.https_url`、`network_proxy.all_url`，迁移必须先迁入加密存储或清除，并保证通用 GET 不返回明文。

如果短期不新增敏感配置表，则必须禁用 `manual` 模式的代理 URL 保存能力，无论 URL 是否包含凭据；代理 URL 不能退回到通用 key/value 存储。此时只允许 `disabled` 或 `inherit_env`。

### 建议配置项

| key | 类型 | 存储 |
|---|---|---|
| `network_proxy.mode` | enum | 非敏感配置 |
| `network_proxy.no_proxy` | string | 非敏感配置 |
| `network_proxy.auto_bypass_local` | bool | 非敏感配置 |
| `network_proxy.version` | integer/string | 非敏感配置 |
| HTTP 代理 URL | secret | 加密存储，不用明文 key/value |
| HTTPS 代理 URL | secret | 加密存储，不用明文 key/value |
| ALL 代理 URL | secret | 加密存储，不用明文 key/value |

因为当前 `update_system_configs` 不是 upsert，落地时必须二选一：

- 通过 DB migration 预置所有非敏感 `network_proxy.*` key。
- 或修改仓储层，让系统配置更新成为真正 upsert。

无论采用哪种方式，`network_proxy.*` 都是保留命名空间：只能由 `/api/admin/network-proxy*` 读写，不能由通用配置表单或通用配置 API 修改。

### 敏感配置表建议

第一阶段建议新增加密配置表，复用 Web 平台现有加密能力：

| 字段 | 说明 |
|---|---|
| `key` | secret 唯一键，例如 `network_proxy.http_url` |
| `encrypted_value` | 加密后的代理 URL |
| `kind` | `network_proxy_http`、`network_proxy_https`、`network_proxy_all` |
| `updated_at` | 更新时间 |

约束：

- `key` 唯一。
- 解密失败、加密失败、kind 不匹配都必须进入 fail closed，不得回退到继承环境。
- API 响应只能使用统一脱敏函数生成 `displayValue`。
- `keep` 遇到当前 secret 不存在时返回 400，前端必须改用 `set` 或切换非 manual 模式。
- `manual` 模式的“至少一个代理 URL”校验必须在应用 keep/set/clear 动作后的草稿状态上执行；如果 `clear` 后没有任何 URL，保存失败。
- `GET /api/admin/network-proxy` 遇到解密失败时返回 200，但 `mode` 展示为 fail-closed 的禁用态并携带阻断级 `warnings`；`/effective` 同样返回 disabled effective config；`/test` 拒绝执行并返回 `validation_failed`。

## 后端 API 设计

新增结构化 API，挂在现有 admin route group 下，必须经过 JWT 和 `require_admin`：

- `GET /api/admin/network-proxy`
- `PUT /api/admin/network-proxy`
- `POST /api/admin/network-proxy/test`
- `GET /api/admin/network-proxy/effective`

所有响应仍使用现有 `ResponseData.data` 包装，字段使用 camelCase。

### 获取代理配置

返回管理员可编辑配置，不返回明文 secret。

| 字段 | 说明 |
|---|---|
| `mode` | 当前模式 |
| `version` | 当前配置版本 |
| `source` | 脱敏来源 |
| `httpProxy` / `httpsProxy` / `allProxy` | secret 展示对象 |
| `noProxy` | 当前绕过规则文本 |
| `autoBypassLocal` | 是否自动绕过本机地址 |
| `needsRestartProjectCount` | 需要重启后生效的运行中项目数量 |
| `updatedAt` | 更新时间 |
| `warnings` | 配置损坏、兼容性等告警 |

`warnings` 必须是结构化数组：

```json
[
  {
    "code": "proxy_secret_decrypt_failed",
    "severity": "error",
    "blocking": true,
    "message": "代理配置不可用，已进入禁用态"
  }
]
```

前端只能依赖 `code`、`severity`、`blocking` 做状态判断；`message` 只用于展示。

secret 展示对象只包含：

| 字段 | 说明 |
|---|---|
| `configured` | 是否已有值 |
| `displayValue` | 脱敏展示值，可为空 |
| `updatedAt` | secret 更新时间 |

### 更新代理配置

更新请求必须整体校验。每个 secret 字段必须使用明确动作：

| action | 含义 |
|---|---|
| `keep` | 保留原密文 |
| `set` | 写入新 URL |
| `clear` | 清空该 URL |

后端必须拒绝把脱敏占位符写回 secret，例如包含 `***` 的代理 URL 一律视为非法输入。

请求必须包含 `expectedVersion`，用于防止两个管理员并发覆盖：

```json
{
  "expectedVersion": "42",
  "mode": "manual",
  "httpProxy": { "action": "keep" },
  "httpsProxy": { "action": "set", "value": "http://user:password@proxy.example.com:8080" },
  "allProxy": { "action": "clear" },
  "noProxy": "localhost,127.0.0.1,::1,.example.com,10.0.0.0/8",
  "autoBypassLocal": true
}
```

字段语义：

- `expectedVersion` 必填；与当前版本不一致时返回配置版本冲突错误，前端必须刷新。
- `action=keep` 不允许携带 `value`，表示保留当前密文。
- `action=set` 必须携带非空 `value`，空字符串非法。
- `action=clear` 不允许携带 `value`，表示删除该 secret。
- `clear` 和空字符串不是同义词；清空必须显式用 `clear`。
- `mode`、`noProxy`、`autoBypassLocal` 必填，避免局部 PATCH 带来不可见状态。

保存规则：

- `manual` 模式至少需要一个代理 URL。
- URL scheme 只能是第一阶段支持范围。
- host 和 port 必须有效。
- 含凭据 URL 必须进入加密存储。
- `NO_PROXY` 必须能解析为受支持规则。
- 切换到 `disabled` 默认保留手动代理 secret，但不生效。
- 保存成功后递增 `version`，并标记运行中项目需重启。
- `expectedVersion` 校验、非敏感配置更新、secret keep/set/clear、`version` 递增、运行中项目重启标记必须在同一个数据库 transaction 内完成，且必须使用 DB 级 CAS，例如 `UPDATE ... WHERE version = expectedVersion` 或 SQLite `BEGIN IMMEDIATE` 加版本复核。
- 如果 transaction 任一步失败，必须整体回滚，不允许留下 mode 已改但 secret 未改、version 已递增但项目未标记等半更新状态。
- 需要新增专用 repository 方法处理代理配置更新；不得复用当前逐 key 更新的通用 `update_system_configs` 路径。

### 连通性测试

测试接口可以使用已保存配置，也可以使用本次表单草稿配置；草稿配置不得写入数据库或日志。

SSRF 防护要求：

- 第一阶段禁用任意自定义 URL，只允许后端维护的预置目标，例如 GitHub、GitLab、Linear、OpenAI 或经过同一安全校验登记的企业 GitLab 域名。
- 如果未来开放自定义 URL，不能只做“请求前解析校验后交给 reqwest 正常请求”。测试请求必须使用受控 resolver 或等价能力，把本次请求 pin 到已校验 IP；重定向的每一跳都必须重新解析、校验并重新建立受控请求，避免 DNS rebinding / TOCTOU。
- 预置或登记目标也必须禁止本机、私网、链路本地、metadata 地址和不可解析地址。
- 限制或禁用跨主机重定向。
- 不记录草稿代理 URL、Authorization header、完整请求头和完整错误对象。
- 测试结果只返回脱敏代理摘要和错误分类。

可实现规则：

- 只允许 `http` 和 `https` scheme。
- 解析 IDNA 域名后再做安全判断。
- DNS 解析得到的每个 A/AAAA 地址都必须是公网地址；拒绝 RFC1918、loopback、link-local、multicast、unspecified、IPv4-mapped IPv6、metadata 地址等。
- CNAME 最终解析到私网地址时拒绝。
- 每次请求前重新解析目标，重定向的每一跳都必须重新执行同样校验。
- 禁止跨 scheme 重定向；跨 host 重定向默认拒绝。
- 拒绝十进制、八进制、十六进制、混淆 IPv4 表达。
- 管理员配置的企业 GitLab 域名如果用于测试目标，也必须先通过同一套 URL 安全校验。

测试请求示例：

```json
{
  "targetId": "github",
  "useDraftConfig": true,
  "draftConfig": {
    "expectedVersion": "42",
    "mode": "manual",
    "httpProxy": { "action": "keep" },
    "httpsProxy": { "action": "set", "value": "http://proxy.example.com:8080" },
    "allProxy": { "action": "clear" },
    "noProxy": "localhost,127.0.0.1,::1",
    "autoBypassLocal": true
  }
}
```

请求 schema 约束：

- `targetId` 必填，只能是后端登记的枚举值；未知 `targetId` 返回 `validation_failed`。
- 第一阶段请求体不允许出现 `targetUrl`、`customUrl` 或任意 URL 字段，出现即返回 400。
- `useDraftConfig=false` 时不得携带 `draftConfig`。
- `useDraftConfig=true` 时必须携带 `draftConfig`。
- `draftConfig` 使用与 PUT 相同的 schema 和校验规则，但不要求 `expectedVersion` 与当前版本一致；`keep` 动作仍只能引用当前已存在的 secret。
- 测试接口不得持久化草稿配置。

测试结果字段：

| 字段 | 说明 |
|---|---|
| `status` | `success`、`proxy_failed`、`target_failed`、`timeout`、`tls_failed`、`validation_failed` |
| `targetHost` | 测试目标 host |
| `proxyUsed` | 是否命中代理 |
| `proxySummary` | 脱敏代理摘要 |
| `durationMs` | 耗时 |
| `message` | 面向管理员的短错误信息 |

### 有效配置诊断

只展示脱敏后的有效配置：

- 模式、来源、版本。
- 最终会注入的变量名和值摘要，不展示原始敏感值。
- NO_PROXY 归一化结果。
- 需要重启的运行中项目数量和项目列表摘要。
- 配置损坏时显示 fail closed 状态。

## Web 平台改造点

### 进程启动

`spawn_symphony` 不再手工遍历代理变量。它必须调用统一代理模块应用环境变量：

1. 对所有标准代理变量执行 `env_remove`。
2. 注入 `SYMPHONY_PROXY_MODE`、`SYMPHONY_PROXY_VERSION`、`SYMPHONY_PROXY_SOURCE`。
3. 如果模式是 `manual` 或 `inherit_env`，注入规范化后的大小写代理变量。
4. 如果模式是 `disabled`，不注入任何标准代理变量。

大小写处理：

- 注入时同时设置大写和小写，保证不同工具兼容。
- 冲突时以规范化后的大写值为准，小写同步同值。
- 禁用时大小写变量都必须移除。

### Web 平台 HTTP client

Web 平台所有外部 HTTP 请求必须通过统一 client factory 创建。第一阶段必须覆盖：

- AI 服务请求。
- token validate 请求。
- GitHub client。
- GitLab client。
- DingTalk/通知测试。
- 后续新增 webhook 或外部服务调用。

代理应用规则：

- `disabled`：Web 平台所有 reqwest builder 也必须显式 `.no_proxy()`，避免读取父进程环境或系统代理。
- `manual` / `inherit_env`：HTTP、HTTPS、ALL 按“HTTP 专用、HTTPS 专用、ALL 兜底”的顺序注册，所有 proxy 使用同一份 normalized NO_PROXY 规则。
- 业务模块不得直接调用 `reqwest::Client::new()` 或裸 `reqwest::Client::builder()`；必须通过 proxy-aware factory。

保存代理配置后：

- Web 平台新建请求使用最新配置。
- 第一阶段统一采用版本化 factory/cache：长期持有的 client 在配置版本变化后必须重建，不能把“重启 Web 平台”作为代理配置生效路径。
- 诊断接口要能显示当前 Web client 使用的代理配置版本。

### 项目服务重启提示

第一阶段就需要引入 `effective_proxy_config_version` 或等价可观测字段：

- 项目服务启动时记录使用的 `proxyConfigVersion`。
- 代理配置变更后，运行中项目如果版本落后，系统配置页显示受影响数量。
- 项目状态诊断返回脱敏的代理版本信息。
- 第一阶段可以只提示重启，不要求一键批量重启。

## rust-platform 改造点

### 所有 reqwest 构造点

rust-platform 不只改 `platform/http_client.rs`。第一阶段必须覆盖所有外部 HTTP client 构造点：

- GitHub/GitLab 平台 API client。
- Linear tracker。
- `linear_graphql` 工具。
- 后续新增 reqwest client。

代理应用规则：

- `disabled`：所有 reqwest builder 必须显式 `.no_proxy()`，避免读取环境或系统代理。
- `manual` / `inherit_env`：按 HTTP 专用、HTTPS 专用、ALL 兜底的顺序注册代理；所有代理必须绑定同一份 normalized NO_PROXY 规则，或使用等价统一 matcher。
- 业务模块不得直接调用 `reqwest::Client::new()` 或裸 `reqwest::Client::builder()`；必须通过 proxy-aware factory 或 builder helper。
- 请求错误要能区分代理连接失败、目标连接失败、TLS 错误和超时。

### 命令启动封装

rust-platform 必须提供统一命令环境封装，例如 `ProxyCommandExt`、`proxy_command(...)` 或等价能力。所有外部命令启动点必须使用它，避免链式 `Command::new(...).spawn()` 漏调用代理环境处理：

- Codex app-server。
- workspace hook。
- Git clone/fetch/push 等命令。
- 未来新增的外部命令。

封装规则：

- 先移除标准代理变量。
- 再注入哨兵变量。
- `manual` / `inherit_env` 时注入规范化代理变量。
- `disabled` 时不注入代理变量。
- 日志只记录脱敏模式、来源和版本。
- 测试或 CI 必须扫描生产代码，默认禁止裸 `Command::new`；仅允许带明确注释或 allowlist 的非外部命令例外。

### Codex 子进程与工具环境

Codex app-server 拿到代理环境，不等于 Codex 内部 shell/MCP/Git 工具一定会继承。第一阶段必须明确并验证：

- 默认 Codex 启动命令或配置必须允许代理变量进入 Codex 工具执行环境。
- 如果需要依赖 `shell_environment_policy.inherit=all` 或等价配置，模板和项目配置必须显式设置。
- 验收不能只检查 Codex 进程环境，还要检查 Codex 发起的工具命令是否能看到代理环境。

## 系统配置界面设计

### 信息架构

当前 `/admin/config` 是通用表格。建议改为分区式系统配置页：

- 顶部保留系统统计卡片。
- 下方使用页签或分组面板：
  - `基础配置`
  - `网络代理`
  - `高级配置`

如果短期不重构页面，可以先在现有系统配置页顶部增加“网络代理”专用区域，通用 key/value 表格保留下方。通用表格不得展示可编辑代理 URL secret。

### 网络代理区布局

采用 JXY 企业级设置页风格：

- 紧凑表单、分组标题、状态 Chip、测试结果表。
- 不使用紫色渐变按钮；主操作使用主题主色 `#0053db -> #0048c1`。
- 静态区域不使用阴影，不做卡片嵌套。
- 控件建议使用 MUI `ToggleButtonGroup`、`TextField`、`Chip`、`Alert`、`Table`、`Button`。

建议布局：

| 区域 | 控件 |
|---|---|
| 当前状态 | 模式 Chip、来源、版本、最后更新时间、需重启项目数量 |
| 代理模式 | 分段控件：禁用、继承环境变量、手动配置 |
| 手动代理 | HTTP/HTTPS/ALL 代理输入框，支持重填和清空 |
| 绕过代理 | 多行输入或标签输入，展示解析后的规则 |
| 连通性测试 | 预置目标选择、测试按钮、结果列表；第一阶段不渲染自定义 URL 输入 |
| 操作区 | 保存、重置、刷新有效配置 |

第一阶段不展示“生效范围”复选框；代理配置全链路生效。

### 表单交互

- 模式为 `disabled` 时，手动代理字段禁用但保留值。
- 模式为 `inherit_env` 时，显示当前继承到的脱敏环境变量。
- 模式为 `manual` 时，至少填写一个代理地址。
- 含凭据的代理 URL 保存后，输入框显示脱敏值；如果管理员不重新填写，提交 secret action 为 `keep`。
- 管理员清空某个代理字段时，提交 secret action 为 `clear`。
- 输入框中包含 `***` 的 URL 不能提交为新 secret。
- `NO_PROXY` 保存前实时解析，无法识别的规则标红。
- 保存成功后，如果有运行中项目服务，显示“运行中的项目服务需重启后使用新代理配置”。

### API 模块与测试落点

前端建议新增 `adminNetworkProxy.ts`，不要把代理结构化 API 塞进现有通用 `adminConfig.ts`。

测试需要覆盖：

- MSW handlers 返回脱敏 secret。
- 模式切换字段启用状态。
- secret keep/set/clear 三种提交。
- 非 admin 路由跳转和后端 403。
- 测试连接 loading、成功、失败状态。
- 受影响项目数量展示。

## 错误处理与安全

### 错误分类

| 错误 | 展示方式 |
|---|---|
| URL 格式错误 | 表单字段内联错误 |
| 提交脱敏占位符 | 阻止保存，提示重新输入或选择保留 |
| 代理认证失败 | 测试结果显示认证失败，不显示凭据 |
| 代理连接失败 | 显示代理 host 和 port 的脱敏摘要 |
| 目标不可达 | 区分目标服务错误和代理错误 |
| 未登记测试目标或请求携带自定义 URL 字段 | 显示安全策略提示 |
| 配置版本不一致 | 提示刷新后重试 |
| 配置损坏 | 显示阻断级告警，fail closed |

### 脱敏规则

任何输出中出现代理 URL 时：

- 用户名最多保留首尾字符，或完全隐藏。
- 密码永远显示为 `***`。
- query string 默认隐藏。
- 日志不记录完整 URL。
- 错误对象、Snackbar、审计日志、项目诊断都必须使用同一脱敏函数。

示例展示形态：

- `http://u***r:***@proxy.example.com:8080`
- `http://proxy.example.com:8080`

### 权限

网络代理 API 必须挂在现有 admin route group 下，并通过 `require_admin`。普通用户：

- 前端不能看到系统配置入口。
- 访问 `/admin/config` 应被保护路由拦截。
- 访问 `/api/admin/network-proxy*` 应返回权限错误。

### 审计

建议记录以下审计事件：

- 代理模式变更。
- 代理地址变更，记录脱敏摘要。
- NO_PROXY 变更。
- 连通性测试执行，记录目标域名和结果，不记录凭据。
- 配置损坏进入 fail closed。

## 生效策略

| 对象 | 生效方式 |
|---|---|
| Web 平台新建 HTTP 请求 | 保存后使用新版本配置 |
| Web 平台长期持有 client | 版本变化后必须重建；第一阶段不可重建视为不合格 |
| 已运行 rust-platform 项目服务 | 重启项目服务后生效 |
| 新启动 rust-platform 项目服务 | 启动时生效 |
| rust-platform reqwest client | 启动时读取模式和代理变量 |
| Codex 子进程 | 下一次启动 Codex app-server 时生效 |
| Git/Hook 命令 | 下一次执行命令时继承当前 rust-platform 有效配置 |

第一阶段不做 rust-platform 内部热更新，避免一次 agent 任务中网络行为前后不一致。

## 迁移计划

### 阶段 1：全局代理闭环

交付：

- 代理配置模型、脱敏规则、配置优先级。
- 加密存储或等价敏感配置能力。
- 结构化 `/api/admin/network-proxy*` API。
- 通用 `/api/admin/config` 对 `network_proxy.*` 保留命名空间执行所有响应路径过滤和 PUT 拒绝。
- 系统配置页网络代理区域。
- Web 平台统一 HTTP client factory。
- Web 平台启动项目服务时注入哨兵变量和代理环境。
- rust-platform 所有 reqwest 构造点使用统一代理配置。
- Codex、Git、Hook 命令使用统一命令环境封装。
- `proxyConfigVersion` 和运行中项目重启提示。
- 连通性测试和脱敏诊断。

验收：

- 管理员可在 UI 中切换禁用、继承环境变量、手动配置。
- 通用 `/api/admin/config` 不返回代理 URL secret。
- 通用 `/api/admin/config` 不能写入任何 `network_proxy.*` key。
- 手动配置代理后，新启动项目服务能通过代理访问 GitHub/GitLab/Linear。
- 禁用代理后，即使 Web 父进程有代理环境变量，新启动项目服务、rust-platform reqwest、Codex、Hook/Git 命令也不使用代理。
- 含凭据代理不会在 API、UI、日志、Snackbar、测试结果和诊断中明文出现。
- 运行中的项目服务在配置变更后显示“重启后生效”。

### 阶段 2：运行状态和批量操作

交付：

- 受影响项目列表。
- 一键重启受影响项目服务。
- 代理变更审计日志页面。
- 更细粒度测试目标模板。

### 阶段 3：高级策略

可选扩展：

- 项目级代理覆盖。
- 生效范围开关。
- SOCKS5 支持。
- 更完整的 NO_PROXY 兼容矩阵。
- 代理配置导入导出。

## 测试计划

### 后端单元测试

- 解析大写和小写代理环境变量。
- 冲突变量归一化。
- `disabled` 模式生成模式哨兵并移除所有代理变量。
- `manual` 模式 URL 校验。
- 拒绝 `socks5://` 和 `host:port` 形式 NO_PROXY。
- 拒绝把包含 `***` 的 URL 作为 secret 写入。
- secret keep/set/clear 三种动作。
- 脱敏规则覆盖用户名、密码、query string。
- 配置优先级和配置损坏 fail closed。
- HTTP、HTTPS、ALL 代理优先级：HTTP/HTTPS 专用优先，ALL 仅兜底。
- normalized NO_PROXY 规则绑定到 HTTP、HTTPS、ALL 每个代理。
- IPv6 与 host:port 校验区分：允许 `::1`，拒绝 `localhost:3000` 和 `example.com:443`。

### 后端集成测试

- `GET /api/admin/network-proxy` 返回脱敏配置。
- `GET /api/admin/config` 不返回任何 `network_proxy.*` key。
- `PUT /api/admin/config` 拒绝所有 `network_proxy.*` key，不能改变代理版本。
- `PUT /api/admin/config` 保存普通配置后的响应体也不返回任何 `network_proxy.*` key。
- 历史误写入 `system_configs` 的代理 URL secret 迁移后不会通过通用配置接口明文返回。
- `PUT /api/admin/network-proxy` 拒绝非法 URL、脱敏占位符和非法 NO_PROXY。
- `PUT /api/admin/network-proxy` 在 `expectedVersion` 过期时返回版本冲突，覆盖 keep/set/clear 并发冲突。
- `POST /api/admin/network-proxy/test` 能区分代理失败、目标失败、超时、TLS 失败。
- 第一阶段请求体携带 `targetUrl`、`customUrl` 或任意 URL 字段时直接返回 400；未知 `targetId` 返回 `validation_failed`。
- 登记测试目标拦截本机、私网、metadata、IPv4-mapped IPv6、混淆 IP、CNAME 到私网、跨主机重定向和非 HTTP scheme。
- Web client 配置版本变化后，新请求使用新版本 client；旧版本 client 不再处理外部请求，诊断版本与当前配置版本一致。
- 非 admin 访问 `/api/admin/network-proxy*` 返回权限错误。
- 启动项目服务时，子进程环境包含哨兵变量和规范化代理变量。
- 禁用代理时，子进程环境不包含任何标准代理变量。
- Web 父进程存在代理环境变量时，`disabled` 下 Web 自身 AI/token validate/GitHub/GitLab/DingTalk 请求不命中测试代理。
- 生产代码静态检查禁止新增绕过 proxy-aware factory 的裸 `reqwest::Client::new()` 或 `reqwest::Client::builder()`。

### rust-platform 测试

- 启动环境包含 `SYMPHONY_PROXY_MODE` 和 `SYMPHONY_PROXY_VERSION`。
- `disabled` 模式下所有 reqwest builder 显式不使用代理。
- 代理模式下请求命中测试代理。
- `platform/http_client`、Linear tracker、`linear_graphql` 都覆盖代理配置。
- Codex 子进程启动时获得正确代理环境。
- Codex 内部工具命令能继承代理变量，或禁用时确认没有代理变量。
- hook/Git 命令继承或清理代理环境。
- NO_PROXY 对 `localhost`、`127.0.0.1`、`::1`、`[::1]`、域名自身与子域名、`.example.com`、CIDR、`*` 分别验证；先解析 bracketed/unbracketed IPv6，再判定 host:port，拒绝 `localhost:3000` 和 `example.com:443`。
- 生产代码静态检查禁止网络相关路径新增未接入代理封装的裸 `Command::new`。

### 前端测试

- 网络代理配置区按模式启用/禁用字段。
- 保存手动代理配置后展示脱敏值。
- secret keep/set/clear 三种提交。
- 含凭据的代理 URL 不回显密码。
- 输入脱敏占位符时不能保存为新 secret。
- 测试连接的 loading、成功、失败状态完整。
- 非 admin 无法进入系统配置页。
- 配置变更后展示受影响项目数量。
- 网络代理区使用独立 `adminNetworkProxy.ts` API 模块，不混入通用 `adminConfig.ts`；配置页迁移后移除旧的紫色渐变按钮样式，不新增卡片嵌套；窄屏下代理 URL 脱敏文本不溢出。

### E2E 测试

- admin 登录后进入系统配置页，切换到网络代理。
- 配置手动代理，保存，执行连通性测试。
- 启动一个项目服务，验证后端记录使用了新的代理配置版本。
- 切换为禁用代理，重启项目服务，验证标准代理变量被清理。
- 在 Web 父进程设置代理环境变量后，禁用模式仍不会命中测试代理。
- 并发打开两个配置页时，一个管理员保存后另一个管理员继续保存会因 `expectedVersion` 过期被拒绝。
- 保存普通系统配置、刷新页面、切换页签三条路径都不能让通用表格显示 `network_proxy.*`。

## 风险与决策

| 风险 | 影响 | 决策 |
|---|---|---|
| 代理凭据进入通用配置接口 | 高 | 代理 URL secret 不进 `/api/admin/config`，只走结构化脱敏 API |
| 通用配置接口绕过结构化代理 API | 高 | `/api/admin/config` 拒绝所有 `network_proxy.*` 写入 |
| `disabled` 只靠移除环境变量表达 | 高 | 必须传 `SYMPHONY_PROXY_MODE=disabled`，reqwest 显式 `.no_proxy()` |
| 子进程默认继承父环境 | 高 | 所有 Command 必须走统一代理环境封装 |
| 配置损坏 fail open | 高 | 已有配置损坏必须 fail closed |
| 自定义测试 URL SSRF | 高 | 第一阶段禁用任意自定义 URL，只允许后端登记的 `targetId` |
| NO_PROXY 行为差异 | 中 | 第一阶段收窄支持子集，拒绝端口限定 |
| SOCKS5 依赖未启用 | 中 | 第一阶段不支持 SOCKS5 |
| 已运行项目服务无法热更新 | 中 | 版本号可观测，UI 提示重启后生效 |

## 推荐落地顺序

1. 定义代理配置模型、secret 动作、脱敏规则、NO_PROXY 子集和配置优先级。
2. 增加敏感配置存储能力和 DB migration，预置非敏感代理配置 key 或支持 upsert。
3. 改造通用 `/api/admin/config`，过滤/脱敏代理保留命名空间并拒绝写入。
4. 增加结构化 `/api/admin/network-proxy*` API、`expectedVersion` 并发控制和权限测试。
5. 改造 Web 平台统一 HTTP client factory。
6. 改造 Web 平台项目服务启动逻辑，注入模式哨兵和代理环境。
7. 改造 rust-platform 所有 reqwest 构造点。
8. 增加统一 `Command` 代理环境封装并覆盖 Codex、Git、Hook。
9. 改造系统配置页，新增网络代理区域和连通性测试。
10. 增加配置版本和运行中项目重启提示。
11. 补齐单元、集成、前端和 E2E 测试。

## 第一阶段验收标准

- 系统配置页有明确的网络代理入口。
- admin 可完成禁用、继承环境变量、手动配置三种模式切换。
- `/api/admin/config` 的 GET 和 PUT 响应都不返回任何 `network_proxy.*` key。
- `/api/admin/config` 拒绝写入任何 `network_proxy.*` key。
- `/api/admin/network-proxy` 只返回脱敏 display 和 configured 标记。
- PUT 代理配置支持 `expectedVersion` 和 secret keep/set/clear；提交脱敏占位符或过期版本会被拒绝。
- 保存非法代理 URL 或非法 NO_PROXY 会被阻止，并显示字段级错误。
- 新启动项目服务获得 `SYMPHONY_PROXY_MODE`、`SYMPHONY_PROXY_VERSION` 和规范化代理环境变量。
- 禁用代理后，新启动项目服务不会继承宿主机代理环境变量。
- 禁用代理后，Web 平台自身 reqwest 不读取父进程环境或系统代理。
- 禁用代理后，rust-platform reqwest 不读取环境或系统代理。
- Web 侧所有外部 HTTP client 通过统一 factory 使用代理配置。
- 生产代码不能新增绕过 proxy-aware factory 的裸 reqwest client 构造。
- rust-platform 所有 reqwest 构造点使用同一份代理配置。
- Codex 子进程、Codex 工具命令、Git 和 hook 命令按有效配置继承或清理代理环境。
- 网络相关外部命令不能新增未接入代理封装的裸 `Command::new`。
- 含凭据代理保存后，页面、API 响应、Snackbar、测试结果、审计日志、后端日志、项目诊断均不出现明文用户名密码或 query string。
- 运行中的项目服务在配置变更后显示“需重启后生效”。
- 连通性测试能返回成功、代理失败、目标失败、TLS 失败、超时、目标被安全策略拦截等结果。
- 第一阶段连通性测试只接受后端登记的 `targetId`，不接受任何自定义 URL 字段。

## 结论

第一阶段推荐以全局系统代理为边界，但必须把“显式模式哨兵、敏感值隔离、命令环境封装、reqwest 显式禁用、配置版本可观测”作为硬性要求。Web 平台负责代理配置管理、校验、脱敏、连通性测试和项目服务启动时的配置注入；rust-platform 负责把有效代理配置应用到所有 HTTP client、Codex 子进程、Git 和 Hook 命令。

这个方案比单纯继承环境变量多一次结构化建模，但能解决配置来源、禁用语义、敏感信息、诊断和运维入口这些核心问题，并为后续项目级代理策略和更复杂网络环境保留扩展空间。
