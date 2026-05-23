# 通用布局设计规范 (AppLayout)

## 1. 整体布局结构

```
+------------------------------------------------------------------+
|                        TopNav (64px)                               |
| [=] [Logo] Symphony Web              [搜索] [Avatar] Name [退出] |
+------+-----------------------------------------------------------+
|      |                                                           |
| Side |                    MainContent                            |
| bar  |                                                           |
| 240px|   Breadcrumbs                                             |
|      |                                                           |
|      |   Page Content                                            |
|      |                                                           |
|      |                                                           |
|      |                                                           |
|      |                                                           |
|      |                                                           |
|      |                                                           |
+------+-----------------------------------------------------------+
```

---

## 2. 组件层次结构

```
AppLayout
├── AppBar (TopNav)
│   ├── Toolbar
│   │   ├── IconButton (Menu - 移动端汉堡菜单)
│   │   ├── Box (Logo 区域)
│   │   │   ├── MusicNote Icon (24px)
│   │   │   └── Typography h6 "Symphony Web"
│   │   ├── Box (flex-grow spacer)
│   │   ├── Box (右侧操作区)
│   │   │   ├── Avatar (用户头像, 32px)
│   │   │   ├── Typography body2 (用户名)
│   │   │   └── IconButton (Logout)
├── Drawer (Sidebar)
│   ├── Toolbar (占位，与 AppBar 对齐)
│   ├── Divider
│   └── List (导航菜单)
│       ├── ListItem (菜单项 - 循环)
│       │   ├── ListItemIcon
│       │   └── ListItemText
│       └── Divider (分组间)
├── Box (MainContent)
│   ├── Toolbar (占位，与 AppBar 对齐)
│   ├── Breadcrumbs
│   └── Box (页面内容 - children)
```

---

## 3. 顶部导航栏 (TopNav)

### 规格
| 属性 | 值 |
|------|-----|
| 高度 | 64px |
| 背景 | #ffffff |
| 阴影 | elevation 1 (subtle) |
| position | fixed |
| z-index | 1200 (MUI AppBar default) |
| 内边距 | 0 16px (左右) |

### 左侧区域
| 元素 | 规格 |
|------|------|
| 汉堡菜单 (移动端) | IconButton, 仅 xs/sm 显示 |
| Logo 图标 | MusicNote, 24px, primary.main |
| 系统名称 | h6, "Symphony Web", grey.900, fontWeight: 600 |
| Logo 与名称间距 | 8px |

### 右侧区域
| 元素 | 规格 |
|------|------|
| 用户头像 | Avatar 32px, 首字母, primary.main 背景 |
| 用户名 | body2, grey.700, 仅 md+ 显示 |
| 退出按钮 | IconButton (Logout), Tooltip "退出登录" |
| 元素间距 | 8px |

### 退出交互
- 点击退出按钮 → 确认弹窗 "确定要退出登录吗？"
- 确认 → 清除 token → 跳转 /login
- 取消 → 关闭弹窗

---

## 4. 侧边栏 (Sidebar)

### 规格
| 属性 | 值 |
|------|-----|
| 宽度 (展开) | 240px |
| 宽度 (收起) | 64px |
| 背景 | #ffffff |
| 边框右侧 | 1px solid grey.200 |
| position | fixed (desktop) / temporary drawer (mobile) |
| top | 64px (AppBar 下方) |
| 高度 | calc(100vh - 64px) |

### 菜单结构

#### Admin 角色
```
+---------------------------+
|                           |
|  管理                     |  (分组标题, caption, grey.500)
|  [人] 用户管理            |
|  [齿] 系统配置            |
|                           |
|  ─────────────────────    |  (Divider)
|                           |
|  个人                     |  (分组标题)
|  [设] 个人设置            |
|                           |
+---------------------------+
```

#### User 角色
```
+---------------------------+
|                           |
|  个人                     |  (分组标题)
|  [设] 个人设置            |
|                           |
+---------------------------+
```

### 菜单项规格
| 属性 | 值 |
|------|-----|
| 高度 | 44px |
| 左内边距 | 16px |
| 图标大小 | 24px |
| 图标颜色 (默认) | grey.600 |
| 图标颜色 (选中) | primary.main |
| 文字颜色 (默认) | grey.800 |
| 文字颜色 (选中) | primary.main |
| 选中背景 | primary.main + alpha(0.08) |
| 选中左边框 | 3px solid primary.main |
| Hover 背景 | grey.100 |
| 圆角 | 0 8px 8px 0 (右侧圆角) |
| 字号 | body2 (14px) |
| 字重 (默认) | 400 |
| 字重 (选中) | 500 |

### 分组标题
| 属性 | 值 |
|------|-----|
| 字号 | caption (12px) |
| 字重 | 500 |
| 颜色 | grey.500 |
| 左内边距 | 16px |
| 上间距 | 16px |
| 下间距 | 4px |
| 文字转换 | uppercase (可选) |

### 收起模式 (md 断点)
- 仅显示图标，隐藏文字
- 宽度 64px
- 图标居中
- Hover 显示 Tooltip（菜单项名称）

---

## 5. 内容区域 (MainContent)

### 规格
| 属性 | 值 |
|------|-----|
| margin-left | 240px (侧边栏展开) / 64px (收起) / 0 (移动端) |
| margin-top | 64px (AppBar 高度) |
| padding | 24px |
| min-height | calc(100vh - 64px) |
| 背景 | grey.50 (#fafafa) |
| 过渡 | margin-left 225ms cubic-bezier(0.4, 0, 0.2, 1) |

### 面包屑
| 属性 | 值 |
|------|-----|
| 位置 | 内容区顶部 |
| 底部间距 | 16px |
| 字号 | body2 (14px) |
| 分隔符 | NavigateNext icon (16px) |
| 当前页颜色 | grey.900 |
| 父级颜色 | grey.600 (可点击) |
| 父级 Hover | primary.main + underline |

### 面包屑映射
| 路由 | 面包屑 |
|------|--------|
| /admin/users | 管理 > 用户管理 |
| /settings | 个人设置 |

---

## 6. 响应式行为

### Desktop (lg+, >= 1200px)
- 侧边栏完全展开 (240px)
- 内容区 margin-left: 240px
- 顶部导航显示用户名

### Tablet (md, 900-1199px)
- 侧边栏收起为图标模式 (64px)
- 内容区 margin-left: 64px
- 顶部导航显示用户名

### Mobile (xs-sm, < 900px)
- 侧边栏隐藏（Temporary Drawer）
- 顶部导航显示汉堡菜单按钮
- 点击汉堡菜单 → 侧边栏从左侧滑出（overlay 模式）
- 点击遮罩层或菜单项 → 侧边栏关闭
- 内容区全宽
- 隐藏用户名，仅显示头像

---

## 7. 导航状态管理

### 路由与菜单高亮
```typescript
const menuItems = [
  {
    group: '管理',
    roles: ['admin'],
    items: [
      { path: '/admin/users', label: '用户管理', icon: PeopleOutline },
      { path: '/admin/config', label: '系统配置', icon: TuneOutline },
    ],
  },
  {
    group: '个人',
    roles: ['admin', 'user'],
    items: [
      { path: '/settings', label: '个人设置', icon: SettingsOutline },
    ],
  },
];
```

- 当前路由匹配的菜单项高亮
- 使用 `useLocation()` 判断当前路径
- 支持前缀匹配（如 `/admin/users/123` 高亮 "用户管理"）

---

## 8. 动画与过渡

| 场景 | 动画 |
|------|------|
| 侧边栏展开/收起 | width 225ms ease |
| 移动端侧边栏滑入 | transform 225ms cubic-bezier |
| 移动端侧边栏滑出 | transform 195ms cubic-bezier |
| 内容区 margin 变化 | margin-left 225ms ease |
| 页面切换 | opacity fade 200ms (可选) |

---

## 9. 无障碍

- AppBar 使用 `role="banner"`
- 侧边栏使用 `role="navigation"` 和 `aria-label="主导航"`
- 当前菜单项使用 `aria-current="page"`
- 汉堡菜单按钮有 `aria-label="打开导航菜单"`
- 移动端 Drawer 打开时 focus trap
- 退出按钮有 `aria-label="退出登录"`
- 面包屑使用 `<nav aria-label="面包屑导航">`
- Skip to content 链接（可选，提升键盘导航体验）

---

## 10. 代码结构参考

```
src/layouts/
  AppLayout.tsx              # 主布局组件
src/components/layout/
  TopNav.tsx                 # 顶部导航栏
  Sidebar.tsx                # 侧边栏
  SidebarMenuItem.tsx        # 菜单项组件
  Breadcrumbs.tsx            # 面包屑
  LogoutConfirmDialog.tsx    # 退出确认弹窗
src/config/
  navigation.ts             # 导航菜单配置
```
