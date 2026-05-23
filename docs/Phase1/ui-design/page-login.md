# 登录页设计规范 (`/login`)

## 1. 页面布局

```
+------------------------------------------------------------------+
|                                                                    |
|                                                                    |
|                                                                    |
|                    +----------------------------+                   |
|                    |                            |                   |
|                    |        [Logo Icon]         |                   |
|                    |       Symphony Web         |                   |
|                    |                            |                   |
|                    |  +----------------------+  |                   |
|                    |  | Username             |  |                   |
|                    |  +----------------------+  |                   |
|                    |                            |                   |
|                    |  +----------------------+  |                   |
|                    |  | Password         [o] |  |                   |
|                    |  +----------------------+  |                   |
|                    |                            |                   |
|                    |  [====== 登录 ======]      |                   |
|                    |                            |                   |
|                    |  (错误提示区域)             |                   |
|                    |                            |                   |
|                    +----------------------------+                   |
|                                                                    |
|                         v0.1.0 - Phase 1                           |
+------------------------------------------------------------------+
```

### 背景
- 纯色背景: `grey.50` (#fafafa)
- 可选：左侧或底部添加淡色几何装饰图案（CSS 渐变实现）

---

## 2. 组件层次结构

```
LoginPage
├── Box (全屏容器, flex, center)
│   ├── Card (登录卡片)
│   │   ├── CardContent
│   │   │   ├── Box (Logo 区域)
│   │   │   │   ├── MusicNote Icon (或自定义 Logo)
│   │   │   │   └── Typography h4 "Symphony Web"
│   │   │   ├── Typography subtitle1 "统一工作流管理平台"
│   │   │   ├── Box (表单区域)
│   │   │   │   ├── TextField (用户名)
│   │   │   │   ├── TextField (密码, with InputAdornment)
│   │   │   │   └── Button (登录)
│   │   │   └── Collapse (错误提示)
│   │   │       └── Alert (error severity)
│   │   └── CardActions (可选: 忘记密码链接)
│   └── Typography caption (版本号)
```

---

## 3. 详细规格

### 登录卡片
| 属性 | 值 |
|------|-----|
| 宽度 | 400px (xs: 100% - 32px padding) |
| 最大宽度 | 400px |
| 内边距 | 40px (xs: 24px) |
| 圆角 | 12px |
| 阴影 | elevation 2 |
| 背景 | #ffffff |

### Logo 区域
| 属性 | 值 |
|------|-----|
| 图标大小 | 48px x 48px |
| 图标颜色 | primary.main |
| 标题字号 | h4 (34px) |
| 标题字重 | 400 |
| 标题颜色 | grey.900 |
| 副标题字号 | subtitle1 (16px) |
| 副标题颜色 | grey.700 |
| Logo 与标题间距 | 12px |
| 标题与副标题间距 | 4px |
| Logo 区域底部间距 | 32px |

### 用户名输入框
| 属性 | 值 |
|------|-----|
| 变体 | outlined |
| 尺寸 | medium |
| 全宽 | true |
| Label | "用户名" |
| Placeholder | "请输入用户名" |
| 前缀图标 | PersonOutline (可选) |
| 自动聚焦 | true |
| autoComplete | "username" |

### 密码输入框
| 属性 | 值 |
|------|-----|
| 变体 | outlined |
| 尺寸 | medium |
| 全宽 | true |
| Label | "密码" |
| Placeholder | "请输入密码" |
| 类型 | password (可切换 text) |
| 后缀图标 | Visibility / VisibilityOff (IconButton) |
| autoComplete | "current-password" |
| 与用户名间距 | 20px |

### 登录按钮
| 属性 | 值 |
|------|-----|
| 变体 | contained |
| 颜色 | primary |
| 尺寸 | large |
| 全宽 | true |
| 高度 | 48px |
| 字号 | 16px |
| 字重 | 500 |
| 与密码框间距 | 24px |
| 圆角 | 8px |
| Loading 状态 | CircularProgress (size=24, white) 替换文字 |

---

## 4. 交互状态

### 4.1 默认状态 (Default)
- 用户名输入框自动聚焦
- 登录按钮可点击
- 无错误提示

### 4.2 输入验证
- 用户名为空时提交 → 输入框显示 error 状态，helperText: "请输入用户名"
- 密码为空时提交 → 输入框显示 error 状态，helperText: "请输入密码"
- 验证在点击登录时触发（非实时）

### 4.3 Loading 状态
- 登录按钮显示 CircularProgress，文字隐藏
- 按钮 disabled
- 输入框 disabled
- 防止重复提交

### 4.4 错误状态
| 错误类型 | 提示文案 | Alert severity |
|----------|----------|----------------|
| 用户名或密码错误 | "用户名或密码错误，请重试" | error |
| 账户被锁定 | "账户已被锁定，请联系管理员" | error |
| 请求频率限制 | "登录尝试过于频繁，请稍后再试" | warning |
| 网络错误 | "网络连接失败，请检查网络后重试" | error |
| 服务器错误 | "服务器异常，请稍后再试" | error |

错误提示使用 `<Alert>` 组件，带 `<Collapse>` 动画展开，位于登录按钮下方 16px。

### 4.5 成功状态
- 登录成功后跳转到首页（`/settings` 或 `/admin/users`，根据角色）
- 无需显示成功提示（直接跳转）

---

## 5. 响应式行为

### Desktop (lg+)
- 卡片宽度 400px，垂直水平居中
- 内边距 40px

### Tablet (sm - md)
- 卡片宽度 400px，垂直水平居中
- 内边距 32px

### Mobile (xs)
- 卡片宽度 100%，左右 margin 16px
- 内边距 24px
- Logo 图标缩小为 40px
- 标题字号降为 h5 (24px)

---

## 6. 键盘交互

| 按键 | 行为 |
|------|------|
| Tab | 在用户名 → 密码 → 登录按钮间切换 |
| Enter | 在任意输入框中按下触发登录 |
| Space | 在登录按钮聚焦时触发登录 |

---

## 7. 无障碍 (Accessibility)

- 所有输入框有明确的 `label` 和 `aria-label`
- 错误提示使用 `role="alert"` 和 `aria-live="polite"`
- 密码显示/隐藏按钮有 `aria-label="显示密码"` / `"隐藏密码"`
- 登录按钮 loading 时有 `aria-busy="true"`
- 颜色对比度满足 WCAG AA 标准 (4.5:1)

---

## 8. 代码结构参考

```
src/pages/
  LoginPage.tsx          # 页面组件
src/components/auth/
  LoginForm.tsx          # 登录表单组件
  PasswordField.tsx      # 密码输入框（带显示/隐藏）
```
