# 个人设置页设计规范 (`/settings`)

## 1. 页面布局

```
+------------------------------------------------------------------+
| [=] Symphony Web                    [搜索...]  [Avatar] user1  [>]|
+------+-----------------------------------------------------------+
|      |                                                           |
| 侧   |  面包屑: 个人设置                                         |
| 边   |                                                           |
| 栏   |  +-------------------------------------------------------+|
|      |  | 个人信息                                               ||
| [齿] |  +-------------------------------------------------------+|
| 个人 |  |                                                       ||
| 设置 |  |  显示名                                                ||
|      |  |  +---------------------------+                        ||
|      |  |  | 张三                      |                        ||
|      |  |  +---------------------------+                        ||
|      |  |                                                       ||
|      |  |  修改密码                                              ||
|      |  |  +---------------------------+                        ||
|      |  |  | 当前密码             [o]  |                        ||
|      |  |  +---------------------------+                        ||
|      |  |  +---------------------------+                        ||
|      |  |  | 新密码               [o]  |                        ||
|      |  |  +---------------------------+                        ||
|      |  |  +---------------------------+                        ||
|      |  |  | 确认新密码           [o]  |                        ||
|      |  |  +---------------------------+                        ||
|      |  |                                                       ||
|      |  |                    [保存个人信息]                      ||
|      |  +-------------------------------------------------------+|
|      |                                                           |
|      |  +-------------------------------------------------------+|
|      |  | Token 配置                                             ||
|      |  +-------------------------------------------------------+|
|      |  |                                                       ||
|      |  |  GitLab Token          状态: [✓ 已配置]               ||
|      |  |  +---------------------------+                        ||
|      |  |  | ••••••••••••••••     [o]  |                        ||
|      |  |  +---------------------------+                        ||
|      |  |  提示: 用于访问 GitLab API                             ||
|      |  |                                                       ||
|      |  |  GitLab Host (可选)                                    ||
|      |  |  +---------------------------+                        ||
|      |  |  | https://gitlab.com        |                        ||
|      |  |  +---------------------------+                        ||
|      |  |                                                       ||
|      |  |  GitHub Token          状态: [○ 未配置]               ||
|      |  |  +---------------------------+                        ||
|      |  |  | (请输入 GitHub Token) [o] |                        ||
|      |  |  +---------------------------+                        ||
|      |  |  提示: 用于访问 GitHub API                             ||
|      |  |                                                       ||
|      |  |                    [保存 Token 配置]                   ||
|      |  +-------------------------------------------------------+|
|      |                                                           |
+------+-----------------------------------------------------------+
```

---

## 2. 组件层次结构

```
SettingsPage
├── AppLayout (通用布局)
│   ├── TopNav
│   ├── Sidebar
│   └── MainContent
│       ├── Breadcrumbs ("个人设置")
│       ├── Typography h5 "个人设置"
│       ├── Card (个人信息区域)
│       │   ├── CardHeader
│       │   │   ├── Avatar (PersonOutline)
│       │   │   └── Typography h6 "个人信息"
│       │   └── CardContent
│       │       ├── TextField (显示名)
│       │       ├── Divider + Typography "修改密码"
│       │       ├── TextField (当前密码)
│       │       ├── TextField (新密码)
│       │       ├── TextField (确认新密码)
│       │       └── Box (按钮区域)
│       │           └── Button "保存个人信息"
│       └── Card (Token 配置区域)
│           ├── CardHeader
│           │   ├── Avatar (KeyOutline)
│           │   └── Typography h6 "Token 配置"
│           └── CardContent
│               ├── Box (GitLab Token)
│               │   ├── Box (标签 + 状态指示)
│               │   │   ├── Typography "GitLab Token"
│               │   │   └── Chip (已配置/未配置)
│               │   ├── TextField (token 输入)
│               │   └── Typography caption (提示)
│               ├── Box (GitLab Host)
│               │   └── TextField (host 输入)
│               ├── Divider
│               ├── Box (GitHub Token)
│               │   ├── Box (标签 + 状态指示)
│               │   │   ├── Typography "GitHub Token"
│               │   │   └── Chip (已配置/未配置)
│               │   ├── TextField (token 输入)
│               │   └── Typography caption (提示)
│               └── Box (按钮区域)
│                   └── Button "保存 Token 配置"
```

---

## 3. 详细规格

### 页面布局
| 属性 | 值 |
|------|-----|
| 内容最大宽度 | 720px |
| 卡片间距 | 24px |
| 内容区 padding | 24px |

### 个人信息卡片

#### 卡片头部
| 属性 | 值 |
|------|-----|
| 图标 | PersonOutline, 包裹在 Avatar 中 |
| Avatar 背景 | primary.light |
| 标题 | h6 "个人信息" |

#### 显示名字段
| 属性 | 值 |
|------|-----|
| Label | "显示名" |
| 变体 | outlined |
| 全宽 | true |
| 最大宽度 | 400px |
| helperText | "其他用户看到的名称" |

#### 修改密码区域
| 属性 | 值 |
|------|-----|
| 分隔 | Divider + 小标题 "修改密码" |
| 小标题样式 | subtitle2, grey.700, margin-top: 24px |
| 提示 | caption "留空则不修改密码" |

#### 密码字段
| 字段 | Label | Placeholder |
|------|-------|-------------|
| 当前密码 | "当前密码" | "请输入当前密码" |
| 新密码 | "新密码" | "请输入新密码（至少6位）" |
| 确认新密码 | "确认新密码" | "请再次输入新密码" |

所有密码字段：
- 最大宽度: 400px
- 带显示/隐藏切换
- 字段间距: 16px

#### 保存按钮
| 属性 | 值 |
|------|-----|
| 变体 | contained |
| 颜色 | primary |
| 位置 | 右对齐 |
| margin-top | 24px |

---

### Token 配置卡片

#### 卡片头部
| 属性 | 值 |
|------|-----|
| 图标 | VpnKeyOutline, 包裹在 Avatar 中 |
| Avatar 背景 | secondary.light |
| 标题 | h6 "Token 配置" |
| 副标题 | "配置第三方平台访问令牌" |

#### Token 状态指示
| 状态 | Chip 样式 |
|------|-----------|
| 已配置 | color="success", variant="outlined", icon=CheckCircle, label="已配置" |
| 未配置 | color="default", variant="outlined", icon=RadioButtonUnchecked, label="未配置" |

#### GitLab Token 字段
| 属性 | 值 |
|------|-----|
| Label | "GitLab Token" |
| 类型 | password (可切换) |
| Placeholder | 已配置时: "••••••••（已保存，输入新值覆盖）"; 未配置时: "请输入 GitLab Personal Access Token" |
| helperText | "用于访问 GitLab API，需要 api 和 read_repository 权限" |
| 全宽 | true |

#### GitLab Host 字段
| 属性 | 值 |
|------|-----|
| Label | "GitLab Host" |
| 类型 | url |
| Placeholder | "https://gitlab.com" |
| helperText | "自建 GitLab 实例地址，使用 gitlab.com 可留空" |
| 全宽 | true |

#### GitHub Token 字段
| 属性 | 值 |
|------|-----|
| Label | "GitHub Token" |
| 类型 | password (可切换) |
| Placeholder | 已配置时: "••••••••（已保存，输入新值覆盖）"; 未配置时: "请输入 GitHub Personal Access Token" |
| helperText | "用于访问 GitHub API，需要 repo 权限" |
| 全宽 | true |

#### Token 分组间距
- GitLab Token 与 GitLab Host 间距: 16px
- GitLab 组与 GitHub 组之间: Divider + 24px 间距

#### 保存按钮
| 属性 | 值 |
|------|-----|
| 变体 | contained |
| 颜色 | primary |
| 位置 | 右对齐 |
| margin-top | 24px |

---

## 4. 交互状态

### 4.1 页面加载
- 显示 Skeleton 占位（2个卡片形状）
- 加载完成后填充当前用户数据
- Token 字段：已配置的显示掩码占位符，未配置的显示空

### 4.2 个人信息保存

#### 仅修改显示名
- 验证: 非空
- 成功: Snackbar success "个人信息已更新"
- 失败: Snackbar error + 具体错误

#### 修改密码
- 验证规则:
  - 当前密码不能为空
  - 新密码最少6位
  - 确认密码必须与新密码一致
- 验证失败: 对应字段显示 error + helperText
- 当前密码错误: 当前密码字段 error, helperText "当前密码不正确"
- 成功: Snackbar success "密码修改成功"

### 4.3 Token 保存
- 空值提交: 不更新该 token（保持原值）
- 新值提交: 更新 token
- 成功: Snackbar success "Token 配置已保存" + 状态 Chip 更新
- 失败: Snackbar error + 具体错误

### 4.4 表单变更检测
- 未修改时保存按钮 disabled（灰色）
- 有修改时保存按钮 enabled
- 离开页面时如有未保存修改，显示确认弹窗

### 4.5 Loading 状态
- 保存按钮显示 CircularProgress
- 表单字段 disabled

---

## 5. 响应式行为

### Desktop (lg+)
- 内容区最大宽度 720px，左对齐
- 所有字段最大宽度 400px

### Tablet (md)
- 内容区全宽，padding 24px
- 字段全宽

### Mobile (xs - sm)
- 内容区全宽，padding 16px
- 卡片内边距减小为 16px
- 字段全宽
- 按钮全宽

---

## 6. 安全考虑

- Token 值从 API 获取时只返回是否已配置（布尔值），不返回实际值
- 前端不缓存 Token 明文
- 密码字段不使用浏览器自动填充 (`autoComplete="new-password"`)
- 当前密码验证在服务端执行

---

## 7. 无障碍

- 每个表单区域使用 `<fieldset>` 和 `<legend>` 语义
- Token 状态 Chip 有 `aria-label`（如 "GitLab Token 状态：已配置"）
- 密码强度无视觉-only 指示（用文字说明）
- 保存成功/失败的 Snackbar 使用 `aria-live="polite"`
- 表单验证错误关联到对应字段 (`aria-describedby`)

---

## 8. 代码结构参考

```
src/pages/
  SettingsPage.tsx              # 页面组件
src/components/settings/
  ProfileSection.tsx            # 个人信息区域
  PasswordChangeForm.tsx        # 密码修改表单
  TokenConfigSection.tsx        # Token 配置区域
  TokenField.tsx                # 单个 Token 输入组件
  TokenStatusChip.tsx           # Token 状态指示
```
