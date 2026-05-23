# 前端测试框架设计

## 1. 测试工具选型

| 层级 | 工具 | 版本 | 用途 |
|------|------|------|------|
| 单元测试 | Vitest | ^2.0 | 组件/逻辑单元测试 |
| 组件测试 | @testing-library/react | ^16.0 | React 组件渲染与交互 |
| DOM 断言 | @testing-library/jest-dom | ^6.0 | DOM 状态断言扩展 |
| 用户事件 | @testing-library/user-event | ^14.0 | 模拟真实用户交互 |
| API Mock | MSW (Mock Service Worker) | ^2.0 | 网络层拦截与模拟 |
| E2E 测试 | Playwright | ^1.45 | 浏览器自动化端到端测试 |
| 覆盖率 | @vitest/coverage-v8 | ^2.0 | 代码覆盖率收集 |
| 快照 | Vitest 内置 | - | 组件渲染快照 |

## 2. 测试目录结构

```
web-management-ui/
├── src/
│   ├── components/
│   │   ├── LoginForm/
│   │   │   ├── LoginForm.tsx
│   │   │   ├── LoginForm.test.tsx      # 组件单元测试
│   │   │   └── index.ts
│   │   ├── UserTable/
│   │   │   ├── UserTable.tsx
│   │   │   ├── UserTable.test.tsx
│   │   │   └── index.ts
│   │   └── ...
│   ├── pages/
│   │   ├── Login/
│   │   │   ├── LoginPage.tsx
│   │   │   ├── LoginPage.test.tsx      # 页面级集成测试
│   │   │   └── index.ts
│   │   ├── UserManagement/
│   │   │   ├── UserManagementPage.tsx
│   │   │   ├── UserManagementPage.test.tsx
│   │   │   └── index.ts
│   │   └── Settings/
│   │       ├── SettingsPage.tsx
│   │       ├── SettingsPage.test.tsx
│   │       └── index.ts
│   ├── stores/
│   │   ├── authStore.ts
│   │   ├── authStore.test.ts           # Store 逻辑测试
│   │   ├── userStore.ts
│   │   └── userStore.test.ts
│   ├── utils/
│   │   ├── validators.ts
│   │   ├── validators.test.ts          # 工具函数测试
│   │   ├── api.ts
│   │   └── api.test.ts
│   ├── hooks/
│   │   ├── useAuth.ts
│   │   ├── useAuth.test.ts
│   │   └── ...
│   └── test/
│       ├── setup.ts                    # Vitest 全局 setup
│       ├── mocks/
│       │   ├── handlers.ts             # MSW request handlers
│       │   ├── server.ts               # MSW server 实例
│       │   └── data.ts                 # Mock 数据工厂
│       ├── utils/
│       │   ├── render.tsx              # 自定义 render（含 providers）
│       │   └── auth.ts                 # 认证辅助函数
│       └── fixtures/
│           ├── users.ts                # 用户测试数据
│           └── configs.ts              # 配置测试数据
├── e2e/
│   ├── fixtures/
│   │   ├── auth.fixture.ts            # Playwright 认证 fixture
│   │   └── base.fixture.ts            # 基础 fixture
│   ├── pages/                          # Page Object Model
│   │   ├── LoginPage.ts
│   │   ├── UserManagementPage.ts
│   │   └── SettingsPage.ts
│   ├── tests/
│   │   ├── login.spec.ts
│   │   ├── user-management.spec.ts
│   │   └── settings.spec.ts
│   └── playwright.config.ts
├── vitest.config.ts
└── package.json
```

## 3. 测试基础设施

### 3.1 Vitest 配置

```typescript
// vitest.config.ts
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov', 'html'],
      include: ['src/**/*.{ts,tsx}'],
      exclude: [
        'src/test/**',
        'src/**/*.test.{ts,tsx}',
        'src/**/*.d.ts',
        'src/main.tsx',
      ],
      thresholds: {
        global: {
          branches: 80,
          functions: 80,
          lines: 80,
          statements: 80,
        },
      },
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
});
```

### 3.2 测试 Setup

```typescript
// src/test/setup.ts
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach, beforeAll, afterAll } from 'vitest';
import { server } from './mocks/server';

// MSW 服务器生命周期
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => {
  cleanup();
  server.resetHandlers();
});
afterAll(() => server.close());
```

### 3.3 自定义 Render

```typescript
// src/test/utils/render.tsx
import { render, RenderOptions } from '@testing-library/react';
import { BrowserRouter } from 'react-router-dom';
import { ThemeProvider } from '@mui/material/styles';
import { QueryClientProvider, QueryClient } from '@tanstack/react-query';
import { theme } from '@/theme';

interface CustomRenderOptions extends Omit<RenderOptions, 'wrapper'> {
  initialRoute?: string;
  authenticated?: boolean;
}

export function renderWithProviders(
  ui: React.ReactElement,
  options: CustomRenderOptions = {}
) {
  const { initialRoute = '/', authenticated = false, ...renderOptions } = options;

  // 设置认证状态
  if (authenticated) {
    localStorage.setItem('token', 'mock-valid-token');
  }

  window.history.pushState({}, '', initialRoute);

  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <ThemeProvider theme={theme}>
          <BrowserRouter>{children}</BrowserRouter>
        </ThemeProvider>
      </QueryClientProvider>
    );
  }

  return {
    ...render(ui, { wrapper: Wrapper, ...renderOptions }),
    queryClient,
  };
}
```

## 4. Mock 策略 (MSW)

### 4.1 Request Handlers

```typescript
// src/test/mocks/handlers.ts
import { http, HttpResponse } from 'msw';
import { mockUsers, mockConfigs } from './data';

const BASE_URL = '/api';

export const handlers = [
  // Auth
  http.post(`${BASE_URL}/auth/login`, async ({ request }) => {
    const body = await request.json() as { username: string; password: string };
    if (body.username === 'admin' && body.password === 'Admin@123456') {
      return HttpResponse.json({
        token: 'mock-jwt-token',
        user: mockUsers.admin,
      });
    }
    return HttpResponse.json(
      { error: { code: 'INVALID_CREDENTIALS', message: 'Invalid credentials' } },
      { status: 401 }
    );
  }),

  http.put(`${BASE_URL}/auth/password`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json({ message: 'Password updated' });
  }),

  // User
  http.get(`${BASE_URL}/user/profile`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json(mockUsers.admin);
  }),

  http.put(`${BASE_URL}/user/profile`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json({ message: 'Profile updated' });
  }),

  http.get(`${BASE_URL}/user/config`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json(mockConfigs.default);
  }),

  http.put(`${BASE_URL}/user/config`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json({ message: 'Config updated' });
  }),

  // Admin
  http.get(`${BASE_URL}/admin/users`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json({
      data: mockUsers.list,
      pagination: { total: mockUsers.list.length, page: 1, size: 20 },
    });
  }),

  http.post(`${BASE_URL}/admin/users`, async ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    const body = await request.json();
    return HttpResponse.json({ id: 99, ...body }, { status: 201 });
  }),

  http.delete(`${BASE_URL}/admin/users/:id`, ({ request, params }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json({ message: 'User deleted' });
  }),

  http.put(`${BASE_URL}/admin/users/:id/reset-password`, ({ request }) => {
    const token = request.headers.get('Authorization');
    if (!token) return HttpResponse.json({}, { status: 401 });
    return HttpResponse.json({ temporary_password: 'TempPass@123' });
  }),

  // Health
  http.get(`${BASE_URL}/health`, () => {
    return HttpResponse.json({ status: 'ok' });
  }),
];
```

### 4.2 Mock 数据工厂

```typescript
// src/test/mocks/data.ts
export const mockUsers = {
  admin: {
    id: 1,
    username: 'admin',
    display_name: 'Administrator',
    role: 'admin',
    created_at: '2024-01-01T00:00:00Z',
  },
  regularUser: {
    id: 2,
    username: 'john',
    display_name: 'John Doe',
    role: 'user',
    created_at: '2024-01-02T00:00:00Z',
  },
  list: [
    // admin + 多个普通用户
  ],
};

export const mockConfigs = {
  default: {
    gitlab_token: '***masked***',
    gitlab_host: 'https://gitlab.example.com',
    github_token: null,
  },
  empty: {
    gitlab_token: null,
    gitlab_host: null,
    github_token: null,
  },
};

// 数据工厂函数
export function createMockUser(overrides = {}) {
  return {
    id: Math.floor(Math.random() * 1000),
    username: `user_${Date.now()}`,
    display_name: 'Test User',
    role: 'user',
    created_at: new Date().toISOString(),
    ...overrides,
  };
}
```

### 4.3 动态 Handler 覆盖

```typescript
// 在单个测试中覆盖默认 handler
import { server } from '@/test/mocks/server';
import { http, HttpResponse } from 'msw';

it('shows error when API returns 500', async () => {
  server.use(
    http.get('/api/admin/users', () => {
      return HttpResponse.json(
        { error: { code: 'INTERNAL_ERROR', message: 'Server error' } },
        { status: 500 }
      );
    })
  );

  // 测试错误 UI 展示...
});
```

## 5. E2E 测试 (Playwright)

### 5.1 Playwright 配置

```typescript
// e2e/playwright.config.ts
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [
    ['html', { open: 'never' }],
    ['junit', { outputFile: 'results/junit.xml' }],
  ],
  use: {
    baseURL: 'http://localhost:5173',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
  ],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
  },
});
```

### 5.2 Page Object Model

```typescript
// e2e/pages/LoginPage.ts
import { Page, Locator } from '@playwright/test';

export class LoginPage {
  readonly page: Page;
  readonly usernameInput: Locator;
  readonly passwordInput: Locator;
  readonly submitButton: Locator;
  readonly errorMessage: Locator;

  constructor(page: Page) {
    this.page = page;
    this.usernameInput = page.getByLabel('用户名');
    this.passwordInput = page.getByLabel('密码');
    this.submitButton = page.getByRole('button', { name: '登录' });
    this.errorMessage = page.getByRole('alert');
  }

  async goto() {
    await this.page.goto('/login');
  }

  async login(username: string, password: string) {
    await this.usernameInput.fill(username);
    await this.passwordInput.fill(password);
    await this.submitButton.click();
  }

  async expectError(message: string) {
    await expect(this.errorMessage).toContainText(message);
  }
}
```

### 5.3 认证 Fixture

```typescript
// e2e/fixtures/auth.fixture.ts
import { test as base } from '@playwright/test';
import { LoginPage } from '../pages/LoginPage';

type AuthFixtures = {
  authenticatedPage: Page;
  adminPage: Page;
};

export const test = base.extend<AuthFixtures>({
  authenticatedPage: async ({ page }, use) => {
    const loginPage = new LoginPage(page);
    await loginPage.goto();
    await loginPage.login('user', 'User@123456');
    await page.waitForURL('/dashboard');
    await use(page);
  },
  adminPage: async ({ page }, use) => {
    const loginPage = new LoginPage(page);
    await loginPage.goto();
    await loginPage.login('admin', 'Admin@123456');
    await page.waitForURL('/dashboard');
    await use(page);
  },
});
```

## 6. CI 集成方案

### 6.1 GitHub Actions Workflow

```yaml
name: Frontend Tests

on:
  pull_request:
    paths:
      - 'web-management-ui/**'
  push:
    branches: [main]
    paths:
      - 'web-management-ui/**'

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web-management-ui
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'
          cache-dependency-path: web-management-ui/package-lock.json
      - run: npm ci
      - name: Run unit tests with coverage
        run: npx vitest run --coverage
      - name: Upload coverage
        uses: codecov/codecov-action@v4
        with:
          files: web-management-ui/coverage/lcov.info
          flags: frontend

  e2e-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    defaults:
      run:
        working-directory: web-management-ui
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'
          cache-dependency-path: web-management-ui/package-lock.json
      - run: npm ci
      - name: Install Playwright browsers
        run: npx playwright install --with-deps chromium
      - name: Start backend (mock or real)
        run: |
          # 启动后端测试服务器
          cd ../web-management && cargo build --release
          ./target/release/web-management --test-mode &
          sleep 3
      - name: Run E2E tests
        run: npx playwright test
      - name: Upload test results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: playwright-report
          path: web-management-ui/playwright-report/
```

### 6.2 覆盖率门禁

| 模块 | 最低覆盖率 |
|------|-----------|
| stores/ | 90% |
| utils/ | 90% |
| components/ | 80% |
| pages/ | 75% |
| hooks/ | 85% |
| 整体 | 80% |

## 7. 测试编写规范

### 7.1 命名规范

```typescript
// 文件命名：与被测文件同名 + .test.tsx
// LoginForm.tsx → LoginForm.test.tsx

// describe 块：组件/模块名
describe('LoginForm', () => {
  // it 块：行为描述
  it('renders username and password inputs', () => {});
  it('shows validation error when username is empty', () => {});
  it('calls onSubmit with credentials when form is valid', () => {});
  it('disables submit button while loading', () => {});
});
```

### 7.2 测试结构 (AAA)

```typescript
it('shows error message on login failure', async () => {
  // Arrange
  server.use(
    http.post('/api/auth/login', () => {
      return HttpResponse.json({ error: { message: '密码错误' } }, { status: 401 });
    })
  );

  // Act
  const { getByLabelText, getByRole, findByRole } = renderWithProviders(<LoginPage />);
  await userEvent.type(getByLabelText('用户名'), 'admin');
  await userEvent.type(getByLabelText('密码'), 'wrong');
  await userEvent.click(getByRole('button', { name: '登录' }));

  // Assert
  const alert = await findByRole('alert');
  expect(alert).toHaveTextContent('密码错误');
});
```

### 7.3 异步测试最佳实践

```typescript
// 使用 findBy* 等待异步渲染
const element = await screen.findByText('加载完成');

// 使用 waitFor 等待状态变化
await waitFor(() => {
  expect(screen.getByRole('table')).toBeInTheDocument();
});

// 使用 waitForElementToBeRemoved 等待消失
await waitForElementToBeRemoved(() => screen.queryByText('加载中...'));
```

## 8. API 拦截器测试策略

```typescript
// src/utils/api.test.ts
describe('API Interceptor', () => {
  it('attaches Authorization header to requests', async () => {
    localStorage.setItem('token', 'test-token');
    const request = await captureRequest('/api/user/profile');
    expect(request.headers.get('Authorization')).toBe('Bearer test-token');
  });

  it('redirects to login on 401 response', async () => {
    server.use(
      http.get('/api/user/profile', () => HttpResponse.json({}, { status: 401 }))
    );
    await api.get('/user/profile');
    expect(window.location.pathname).toBe('/login');
  });

  it('clears token on 401 response', async () => {
    localStorage.setItem('token', 'expired-token');
    server.use(
      http.get('/api/user/profile', () => HttpResponse.json({}, { status: 401 }))
    );
    await api.get('/user/profile').catch(() => {});
    expect(localStorage.getItem('token')).toBeNull();
  });

  it('shows global error notification on 500', async () => {
    server.use(
      http.get('/api/user/profile', () => HttpResponse.json({}, { status: 500 }))
    );
    // 验证全局错误提示出现
  });
});
```

## 9. 路由守卫测试策略

```typescript
// src/components/ProtectedRoute.test.tsx
describe('ProtectedRoute', () => {
  it('redirects to /login when not authenticated', () => {
    renderWithProviders(<App />, { initialRoute: '/dashboard', authenticated: false });
    expect(window.location.pathname).toBe('/login');
  });

  it('renders children when authenticated', () => {
    renderWithProviders(<App />, { initialRoute: '/dashboard', authenticated: true });
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
  });

  it('redirects non-admin from admin routes', () => {
    // mock user role = 'user'
    renderWithProviders(<App />, { initialRoute: '/admin/users', authenticated: true });
    expect(window.location.pathname).toBe('/dashboard');
  });

  it('allows admin to access admin routes', () => {
    // mock user role = 'admin'
    renderWithProviders(<App />, { initialRoute: '/admin/users', authenticated: true });
    expect(screen.getByText('用户管理')).toBeInTheDocument();
  });
});
```
