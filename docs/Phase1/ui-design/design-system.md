# Symphony Web - 设计系统规范

## 1. 颜色方案

### 主色 (Primary)
| Token | 色值 | 用途 |
|-------|------|------|
| primary.main | `#1976d2` | 主按钮、链接、活跃状态 |
| primary.light | `#42a5f5` | Hover 状态 |
| primary.dark | `#1565c0` | Active/Pressed 状态 |
| primary.contrastText | `#ffffff` | 主色上的文字 |

### 次色 (Secondary)
| Token | 色值 | 用途 |
|-------|------|------|
| secondary.main | `#9c27b0` | 次要强调 |
| secondary.light | `#ba68c8` | 次要 Hover |
| secondary.dark | `#7b1fa2` | 次要 Active |

### 中性色 (Neutral)
| Token | 色值 | 用途 |
|-------|------|------|
| grey.50 | `#fafafa` | 页面背景 |
| grey.100 | `#f5f5f5` | 卡片背景、侧边栏背景 |
| grey.200 | `#eeeeee` | 分割线、边框 |
| grey.300 | `#e0e0e0` | 禁用状态边框 |
| grey.500 | `#9e9e9e` | 占位文字 |
| grey.700 | `#616161` | 次要文字 |
| grey.900 | `#212121` | 主要文字 |

### 语义色 (Semantic)
| Token | 色值 | 用途 |
|-------|------|------|
| error.main | `#d32f2f` | 错误提示、删除操作 |
| error.light | `#ef5350` | 错误背景 |
| warning.main | `#ed6c02` | 警告提示 |
| warning.light | `#ff9800` | 警告背景 |
| success.main | `#2e7d32` | 成功提示、已配置状态 |
| success.light | `#4caf50` | 成功背景 |
| info.main | `#0288d1` | 信息提示 |

### 背景色
| Token | 色值 | 用途 |
|-------|------|------|
| background.default | `#fafafa` | 页面背景 |
| background.paper | `#ffffff` | 卡片、弹窗背景 |

---

## 2. 字体规范

### 字体族
```
fontFamily: '"Inter", "Roboto", "Helvetica Neue", Arial, sans-serif'
```

### 字号层级
| 级别 | 字号 | 行高 | 字重 | 用途 |
|------|------|------|------|------|
| h4 | 34px (2.125rem) | 1.235 | 400 | 页面大标题（登录页标题） |
| h5 | 24px (1.5rem) | 1.334 | 400 | 区域标题 |
| h6 | 20px (1.25rem) | 1.6 | 500 | 卡片标题、弹窗标题 |
| subtitle1 | 16px (1rem) | 1.75 | 400 | 副标题 |
| subtitle2 | 14px (0.875rem) | 1.57 | 500 | 小标题、标签 |
| body1 | 16px (1rem) | 1.5 | 400 | 正文 |
| body2 | 14px (0.875rem) | 1.43 | 400 | 次要正文、表格内容 |
| caption | 12px (0.75rem) | 1.66 | 400 | 辅助说明、时间戳 |
| button | 14px (0.875rem) | 1.75 | 500 | 按钮文字 |

---

## 3. 间距系统

基于 8px 网格系统（MUI spacing factor = 8px）。

| Token | 值 | 用途 |
|-------|-----|------|
| spacing(0.5) | 4px | 极小间距（图标与文字） |
| spacing(1) | 8px | 紧凑间距（表单元素内部） |
| spacing(1.5) | 12px | 小间距 |
| spacing(2) | 16px | 标准间距（表单字段间距） |
| spacing(3) | 24px | 中等间距（区域内部 padding） |
| spacing(4) | 32px | 大间距（区域之间） |
| spacing(5) | 40px | 较大间距 |
| spacing(6) | 48px | 页面级间距 |
| spacing(8) | 64px | 超大间距（登录页垂直居中偏移） |

### 常用间距场景
- 页面内容 padding: `spacing(3)` = 24px
- 卡片内部 padding: `spacing(3)` = 24px
- 表单字段垂直间距: `spacing(2.5)` = 20px
- 按钮组间距: `spacing(1.5)` = 12px
- 表格行高: 52px
- 导航栏高度: 64px
- 侧边栏宽度: 240px（展开）/ 64px（收起）

---

## 4. 圆角规范

| 组件 | 圆角 |
|------|------|
| 按钮 | 8px (`borderRadius: 2`) |
| 卡片 | 12px (`borderRadius: 3`) |
| 输入框 | 8px |
| 弹窗 | 12px |
| 头像 | 50% (圆形) |
| Chip/Tag | 16px |

---

## 5. 阴影规范

| 级别 | 值 | 用途 |
|------|-----|------|
| elevation 0 | none | 平面元素 |
| elevation 1 | `0 1px 3px rgba(0,0,0,0.12)` | 卡片默认 |
| elevation 2 | `0 3px 6px rgba(0,0,0,0.16)` | 卡片 Hover |
| elevation 4 | `0 6px 12px rgba(0,0,0,0.12)` | 弹窗 |
| elevation 8 | `0 12px 24px rgba(0,0,0,0.12)` | 下拉菜单 |
| elevation 16 | `0 24px 48px rgba(0,0,0,0.12)` | Modal |

---

## 6. 响应式断点

| 断点 | 宽度 | 设备 |
|------|------|------|
| xs | 0 - 599px | 手机 |
| sm | 600 - 899px | 平板竖屏 |
| md | 900 - 1199px | 平板横屏 / 小笔记本 |
| lg | 1200 - 1535px | 笔记本 / 桌面 |
| xl | >= 1536px | 大屏桌面 |

### 响应式行为
- **xs/sm**: 侧边栏隐藏（汉堡菜单触发），内容全宽
- **md**: 侧边栏收起为图标模式（64px），内容自适应
- **lg/xl**: 侧边栏完全展开（240px），内容自适应

---

## 7. 动画规范

| 场景 | 时长 | 缓动函数 |
|------|------|----------|
| 按钮 Hover | 150ms | ease-in-out |
| 页面切换 | 225ms | cubic-bezier(0.4, 0, 0.2, 1) |
| 弹窗出现 | 225ms | cubic-bezier(0.4, 0, 0.2, 1) |
| 弹窗消失 | 195ms | cubic-bezier(0.4, 0, 0.2, 1) |
| 侧边栏展开/收起 | 225ms | cubic-bezier(0.4, 0, 0.2, 1) |
| Snackbar 出现 | 225ms | ease-out |
| Snackbar 消失 | 195ms | ease-in |

---

## 8. 图标规范

使用 `@mui/icons-material`，统一 24px 尺寸。

### 常用图标映射
| 场景 | 图标 |
|------|------|
| 用户管理 | `PeopleOutline` |
| 个人设置 | `SettingsOutline` |
| 系统配置 | `TuneOutline` |
| 退出登录 | `LogoutOutline` |
| 添加 | `Add` |
| 编辑 | `EditOutline` |
| 删除 | `DeleteOutline` |
| 搜索 | `SearchOutline` |
| 显示密码 | `VisibilityOutline` |
| 隐藏密码 | `VisibilityOffOutline` |
| 成功状态 | `CheckCircleOutline` |
| 错误状态 | `ErrorOutline` |
| 菜单 | `MenuOutline` |

---

## 9. MUI 主题配置

```typescript
import { createTheme } from '@mui/material/styles';

const theme = createTheme({
  palette: {
    primary: {
      main: '#1976d2',
      light: '#42a5f5',
      dark: '#1565c0',
    },
    secondary: {
      main: '#9c27b0',
    },
    background: {
      default: '#fafafa',
      paper: '#ffffff',
    },
  },
  typography: {
    fontFamily: '"Inter", "Roboto", "Helvetica Neue", Arial, sans-serif',
    button: {
      textTransform: 'none', // 按钮文字不全大写
    },
  },
  shape: {
    borderRadius: 8,
  },
  components: {
    MuiButton: {
      styleOverrides: {
        root: {
          borderRadius: 8,
          padding: '8px 20px',
          fontWeight: 500,
        },
        sizeLarge: {
          padding: '12px 24px',
          fontSize: '1rem',
        },
      },
    },
    MuiTextField: {
      defaultProps: {
        variant: 'outlined',
        size: 'medium',
      },
    },
    MuiCard: {
      styleOverrides: {
        root: {
          borderRadius: 12,
        },
      },
    },
    MuiDialog: {
      styleOverrides: {
        paper: {
          borderRadius: 12,
        },
      },
    },
  },
});
```

---

## 10. Tailwind CSS 集成

Tailwind 用于辅助布局和快速样式调整，不替代 MUI 组件样式。

### 使用场景
- 页面级布局（flex、grid）
- 间距微调
- 响应式工具类
- 自定义背景、渐变

### 避免使用
- 不覆盖 MUI 组件的核心样式（用 theme 配置）
- 不用 Tailwind 的颜色系统替代 MUI palette

### tailwind.config.js 扩展
```javascript
module.exports = {
  content: ['./src/**/*.{ts,tsx}'],
  important: '#root', // 避免与 MUI 冲突
  theme: {
    extend: {
      colors: {
        primary: {
          main: '#1976d2',
          light: '#42a5f5',
          dark: '#1565c0',
        },
      },
    },
  },
  corePlugins: {
    preflight: false, // 禁用 Tailwind reset，避免与 MUI 冲突
  },
};
```
