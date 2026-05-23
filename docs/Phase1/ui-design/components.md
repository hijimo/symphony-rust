# 通用组件设计规范

## 1. 数据表格 (DataTable)

### 概述
可复用的数据表格组件，支持排序、分页、搜索、空状态、加载状态。

### 组件接口
```typescript
interface DataTableProps<T> {
  columns: ColumnDef<T>[];
  data: T[];
  loading?: boolean;
  totalCount?: number;
  page?: number;
  pageSize?: number;
  onPageChange?: (page: number) => void;
  onPageSizeChange?: (size: number) => void;
  onSort?: (field: string, direction: 'asc' | 'desc') => void;
  emptyMessage?: string;
  emptyIcon?: ReactNode;
}

interface ColumnDef<T> {
  field: string;
  headerName: string;
  width?: number | string;
  align?: 'left' | 'center' | 'right';
  sortable?: boolean;
  renderCell?: (row: T) => ReactNode;
}
```

### 视觉规格
| 属性 | 值 |
|------|-----|
| 容器 | Paper, elevation 0, border 1px solid grey.200 |
| 圆角 | 12px |
| 表头背景 | grey.50 |
| 表头字重 | 600 |
| 表头字号 | 14px |
| 行高 | 52px |
| 行 Hover | grey.50 |
| 行边框 | 1px solid grey.100 |
| 内容字号 | 14px |
| 内容颜色 | grey.900 |

### 状态

#### Loading
```
+-------------------------------------------------------+
| 列头1      | 列头2      | 列头3      | 列头4         |
|------------|------------|------------|---------------|
| [████████] | [██████]   | [████]     | [██████████]  |
| [██████]   | [████████] | [██████]   | [████]        |
| [████████] | [████]     | [████████] | [██████]      |
| [██████]   | [██████]   | [████]     | [████████]    |
| [████]     | [████████] | [██████]   | [████]        |
+-------------------------------------------------------+
```
使用 MUI `<Skeleton variant="text" />` 填充 5 行。

#### Empty
```
+-------------------------------------------------------+
|                                                       |
|              [Icon, 64px, grey.300]                   |
|              主提示文字 (body1, grey.700)              |
|              副提示文字 (body2, grey.500)              |
|                                                       |
+-------------------------------------------------------+
```

#### Error
```
+-------------------------------------------------------+
|                                                       |
|              [ErrorOutline, 48px, error.main]         |
|              加载失败 (body1, grey.700)                |
|              [重试] (Button, outlined)                |
|                                                       |
+-------------------------------------------------------+
```

---

## 2. 表单组件

### 2.1 PasswordField (密码输入框)

带显示/隐藏切换的密码输入框。

```typescript
interface PasswordFieldProps extends TextFieldProps {
  showToggle?: boolean; // 默认 true
}
```

#### 视觉规格
- 基于 MUI TextField outlined
- 后缀: IconButton (Visibility / VisibilityOff)
- IconButton size: "edge" (无额外 padding)
- 切换时输入框不失焦

### 2.2 SearchField (搜索输入框)

带搜索图标和清除按钮的输入框。

```typescript
interface SearchFieldProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  debounceMs?: number; // 默认 300
}
```

#### 视觉规格
| 属性 | 值 |
|------|-----|
| 尺寸 | small |
| 前缀 | Search icon, grey.500 |
| 后缀 | Clear icon (仅有值时显示) |
| 宽度 | 280px (可配置) |
| 圆角 | 8px |

### 2.3 FormSection (表单分区)

表单内的逻辑分区组件。

```
+-------------------------------------------------------+
| [Icon] 区域标题                                        |
| 区域描述文字                                           |
+-------------------------------------------------------+
| 表单内容                                               |
+-------------------------------------------------------+
```

---

## 3. 弹窗组件

### 3.1 ConfirmDialog (确认弹窗)

通用确认弹窗，用于删除、重置等危险操作。

```typescript
interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string | ReactNode;
  confirmText?: string;       // 默认 "确认"
  cancelText?: string;        // 默认 "取消"
  confirmColor?: 'primary' | 'error'; // 默认 'primary'
  icon?: ReactNode;           // 标题前图标
  loading?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}
```

#### 视觉规格
| 属性 | 值 |
|------|-----|
| 宽度 | 400px (xs: 全屏 - 32px) |
| 圆角 | 12px |
| 内边距 | 24px |
| 标题 | h6, grey.900 |
| 内容 | body1, grey.700 |
| 按钮区域 | 右对齐, gap: 12px |
| 遮罩 | rgba(0,0,0,0.5) |

#### 删除确认样式
- 图标: WarningAmber, color: warning.main, size: 48px
- 确认按钮: color="error", variant="contained"
- 取消按钮: color="inherit", variant="text"

### 3.2 FormDialog (表单弹窗)

包含表单的弹窗，用于创建/编辑操作。

```typescript
interface FormDialogProps {
  open: boolean;
  title: string;
  maxWidth?: 'xs' | 'sm' | 'md'; // 默认 'sm' (480px)
  loading?: boolean;
  submitText?: string;
  cancelText?: string;
  onSubmit: () => void;
  onCancel: () => void;
  children: ReactNode;
}
```

#### 视觉规格
| 属性 | 值 |
|------|-----|
| 标题栏 | h6 + 右上角 Close IconButton |
| 内容区 | padding 24px, 表单字段 |
| 底部操作栏 | padding 16px 24px, 右对齐 |
| 分隔线 | 标题与内容间、内容与操作间 |

#### 移动端 (xs)
- 全屏模式 (`fullScreen` prop)
- 标题栏固定顶部
- 内容区可滚动
- 操作栏固定底部

---

## 4. 反馈组件

### 4.1 Snackbar (消息提示)

全局消息提示，用于操作反馈。

#### 位置
- 桌面: 底部居中 (`bottom`, `center`)
- 移动端: 底部全宽

#### 类型
| 类型 | 颜色 | 图标 | 示例 |
|------|------|------|------|
| success | success.main | CheckCircle | "用户创建成功" |
| error | error.main | Error | "操作失败：网络错误" |
| warning | warning.main | Warning | "登录尝试过于频繁" |
| info | info.main | Info | "数据已更新" |

#### 规格
| 属性 | 值 |
|------|-----|
| 自动隐藏 | 4000ms (error: 6000ms) |
| 最大宽度 | 400px |
| 圆角 | 8px |
| 阴影 | elevation 4 |
| 动画 | Slide from bottom |
| 可关闭 | 右侧 Close IconButton |

### 4.2 LoadingOverlay (加载遮罩)

页面级或区域级加载状态。

```
+-------------------------------------------------------+
|                                                       |
|              [CircularProgress, 40px]                 |
|              加载中... (可选文字)                      |
|                                                       |
+-------------------------------------------------------+
```

| 属性 | 值 |
|------|-----|
| 背景 | rgba(255,255,255,0.7) |
| position | absolute (区域) / fixed (全屏) |
| z-index | 1000 |
| 动画 | fade in 200ms |

### 4.3 EmptyState (空状态)

通用空状态组件。

```typescript
interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: {
    label: string;
    onClick: () => void;
  };
}
```

| 属性 | 值 |
|------|-----|
| 图标大小 | 64px |
| 图标颜色 | grey.300 |
| 标题 | body1, grey.700, fontWeight: 500 |
| 描述 | body2, grey.500 |
| 垂直间距 | 12px |
| 整体 padding | 48px |
| 对齐 | 居中 |

---

## 5. 导航组件

### 5.1 Breadcrumbs (面包屑)

```typescript
interface BreadcrumbItem {
  label: string;
  path?: string; // 无 path 表示当前页
}
```

| 属性 | 值 |
|------|-----|
| 字号 | 14px |
| 分隔符 | NavigateNext (16px, grey.400) |
| 链接颜色 | grey.600 |
| 链接 Hover | primary.main, underline |
| 当前页颜色 | grey.900 |
| 当前页字重 | 500 |

### 5.2 PageHeader (页面头部)

```
+-------------------------------------------------------+
| 页面标题                              [操作按钮]       |
| 页面描述 (可选)                                        |
+-------------------------------------------------------+
```

```typescript
interface PageHeaderProps {
  title: string;
  description?: string;
  actions?: ReactNode;
}
```

---

## 6. 状态指示组件

### 6.1 StatusChip (状态标签)

```typescript
type StatusType = 'configured' | 'unconfigured' | 'active' | 'inactive';

interface StatusChipProps {
  status: StatusType;
  label?: string;
}
```

| 状态 | 颜色 | 图标 | 默认文字 |
|------|------|------|----------|
| configured | success | CheckCircle | "已配置" |
| unconfigured | default | RadioButtonUnchecked | "未配置" |
| active | success | CheckCircle | "活跃" |
| inactive | default | Block | "未激活" |

### 6.2 RoleChip (角色标签)

| 角色 | 颜色 | 变体 |
|------|------|------|
| admin | primary | filled |
| user | default | outlined |

---

## 7. 工具栏组件

### 7.1 TableToolbar (表格工具栏)

```
+-------------------------------------------------------+
| [搜索...]          [筛选1 v] [筛选2 v]    [刷新] [+]  |
+-------------------------------------------------------+
```

```typescript
interface TableToolbarProps {
  searchValue?: string;
  onSearchChange?: (value: string) => void;
  searchPlaceholder?: string;
  filters?: FilterDef[];
  actions?: ReactNode;
  onRefresh?: () => void;
}
```

| 属性 | 值 |
|------|-----|
| 布局 | flex, row, space-between |
| 间距 | gap: 12px |
| 底部间距 | 16px |
| 响应式 | xs 时 wrap，搜索框全宽 |

---

## 8. 错误边界

### ErrorBoundary

捕获子组件渲染错误，显示友好错误页面。

```
+-------------------------------------------------------+
|                                                       |
|              [BugReport, 64px, grey.400]              |
|              页面出现了问题                             |
|              请刷新页面重试                             |
|              [刷新页面] (Button, outlined)             |
|                                                       |
|              错误详情 (Collapse, 可展开)               |
|              Error: xxx at xxx                         |
|                                                       |
+-------------------------------------------------------+
```

---

## 9. 全局样式约定

### 按钮使用规范
| 场景 | 变体 | 颜色 |
|------|------|------|
| 主要操作 (提交、保存) | contained | primary |
| 次要操作 (取消) | text | inherit |
| 危险操作 (删除) | contained | error |
| 辅助操作 (刷新、导出) | outlined | primary |
| 图标操作 (编辑、删除行) | IconButton | default/error |

### 表单验证显示
- 验证时机: 提交时 (onSubmit)
- 错误样式: TextField error prop + helperText
- 错误颜色: error.main
- 错误图标: 输入框右侧 ErrorOutline (可选)

### 加载状态约定
- 按钮: 内部 CircularProgress (size=20) 替换文字
- 表格: Skeleton rows
- 页面: 居中 CircularProgress (size=40)
- 区域: LoadingOverlay

---

## 10. 代码结构参考

```
src/components/common/
  DataTable/
    DataTable.tsx
    DataTableHead.tsx
    DataTableBody.tsx
    DataTablePagination.tsx
    DataTableEmpty.tsx
    DataTableSkeleton.tsx
  Form/
    PasswordField.tsx
    SearchField.tsx
    FormSection.tsx
  Dialog/
    ConfirmDialog.tsx
    FormDialog.tsx
  Feedback/
    SnackbarProvider.tsx
    useSnackbar.ts
    LoadingOverlay.tsx
    EmptyState.tsx
    ErrorBoundary.tsx
  Navigation/
    Breadcrumbs.tsx
    PageHeader.tsx
  Status/
    StatusChip.tsx
    RoleChip.tsx
  Toolbar/
    TableToolbar.tsx
```
