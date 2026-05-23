# Phase 5 前端测试框架设计

## 概述

本文档定义 Phase 5（告警与通知）前端的完整测试策略，覆盖单元测试、集成测试和 E2E 测试。

## GitLab CI 信息

- GitLab URL: http://gitlab.jushuitan-inc.com:8081/
- 项目: /zimei10525/symphony_e2e_test_repo
- GITLAB_TOKEN: gitlab-token-example

---

## 1. 单元测试 (Vitest + Testing Library)

### 1.1 目录结构

```
web-frontend/src/
├── components/
│   └── alerts/
│       └── __tests__/
│           ├── SeverityBadge.test.tsx
│           ├── AlertHistoryTable.test.tsx
│           ├── AlertRuleCard.test.tsx
│           ├── ChannelConfigCard.test.tsx
│           └── TestNotificationButton.test.tsx
├── store/
│   └── __tests__/
│       └── alertStore.test.ts
├── api/
│   └── __tests__/
│       └── alerts.test.ts
├── pages/
│   └── __tests__/
│       └── AdminAlerts.test.tsx
└── test/
    ├── setup.ts
    ├── utils.tsx
    └── mocks/
        ├── handlers.ts (扩展 Phase 5 handlers)
        └── server.ts
```

### 1.2 SeverityBadge 组件测试

```tsx
// src/components/alerts/__tests__/SeverityBadge.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import SeverityBadge from '../SeverityBadge';

describe('SeverityBadge', () => {
  it('renders critical with red color', () => {
    render(<SeverityBadge severity="critical" />);
    const badge = screen.getByText('严重');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveStyle({ backgroundColor: expect.stringContaining('error') });
  });

  it('renders warning with orange color', () => {
    render(<SeverityBadge severity="warning" />);
    expect(screen.getByText('警告')).toBeInTheDocument();
  });

  it('renders info with blue color', () => {
    render(<SeverityBadge severity="info" />);
    expect(screen.getByText('信息')).toBeInTheDocument();
  });
});
```

### 1.3 AlertRuleCard 组件测试

```tsx
// src/components/alerts/__tests__/AlertRuleCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import AlertRuleCard from '../AlertRuleCard';

const mockRule = {
  ruleId: 'task_timeout',
  name: '任务超时',
  description: 'Codex 单任务运行时间超过阈值',
  severity: 'warning' as const,
  enabled: true,
  thresholdValue: 30,
  thresholdUnit: 'minutes',
  cooldownSeconds: 300,
};

describe('AlertRuleCard', () => {
  it('renders rule name and description', () => {
    render(<AlertRuleCard rule={mockRule} onChange={vi.fn()} />);
    expect(screen.getByText('任务超时')).toBeInTheDocument();
    expect(screen.getByText(/单任务运行时间/)).toBeInTheDocument();
  });

  it('toggle enabled calls onChange', () => {
    const onChange = vi.fn();
    render(<AlertRuleCard rule={mockRule} onChange={onChange} />);
    fireEvent.click(screen.getByRole('checkbox'));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ enabled: false }));
  });

  it('threshold input updates value', () => {
    const onChange = vi.fn();
    render(<AlertRuleCard rule={mockRule} onChange={onChange} />);
    const input = screen.getByDisplayValue('30');
    fireEvent.change(input, { target: { value: '45' } });
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ thresholdValue: 45 }));
  });
});
```

### 1.4 ChannelConfigCard 组件测试

```tsx
// src/components/alerts/__tests__/ChannelConfigCard.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import ChannelConfigCard from '../ChannelConfigCard';

const mockChannel = {
  channelId: 'ch_1',
  channelType: 'dingtalk',
  name: '研发群',
  enabled: true,
  config: { webhookUrl: 'https://oapi.dingtalk.com/robot/send?access_token=xxx', secret: 'SEC***' },
  severityFilter: ['critical', 'warning'],
  createdAt: '2026-01-01T00:00:00Z',
  updatedAt: '2026-01-01T00:00:00Z',
};

describe('ChannelConfigCard', () => {
  it('renders channel name and type', () => {
    render(<ChannelConfigCard channel={mockChannel} onChange={vi.fn()} onTest={vi.fn()} />);
    expect(screen.getByText('研发群')).toBeInTheDocument();
    expect(screen.getByText('钉钉')).toBeInTheDocument();
  });

  it('webhook URL input is present', () => {
    render(<ChannelConfigCard channel={mockChannel} onChange={vi.fn()} onTest={vi.fn()} />);
    expect(screen.getByDisplayValue(/oapi.dingtalk.com/)).toBeInTheDocument();
  });

  it('test button triggers onTest', () => {
    const onTest = vi.fn();
    render(<ChannelConfigCard channel={mockChannel} onChange={vi.fn()} onTest={onTest} />);
    fireEvent.click(screen.getByText('测试通知'));
    expect(onTest).toHaveBeenCalledWith('ch_1');
  });

  it('severity filter checkboxes work', () => {
    const onChange = vi.fn();
    render(<ChannelConfigCard channel={mockChannel} onChange={onChange} onTest={vi.fn()} />);
    const infoCheckbox = screen.getByLabelText('信息');
    fireEvent.click(infoCheckbox);
    expect(onChange).toHaveBeenCalled();
  });
});
```

### 1.5 TestNotificationButton 组件测试

```tsx
// src/components/alerts/__tests__/TestNotificationButton.test.tsx
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import TestNotificationButton from '../TestNotificationButton';

describe('TestNotificationButton', () => {
  it('shows loading state during test', async () => {
    const onTest = vi.fn(() => new Promise(resolve => setTimeout(resolve, 100)));
    render(<TestNotificationButton onTest={onTest} />);
    fireEvent.click(screen.getByText('测试通知'));
    expect(screen.getByRole('progressbar')).toBeInTheDocument();
  });

  it('shows success result', async () => {
    const onTest = vi.fn().mockResolvedValue({ success: true, message: '发送成功', durationMs: 150 });
    render(<TestNotificationButton onTest={onTest} />);
    fireEvent.click(screen.getByText('测试通知'));
    await waitFor(() => expect(screen.getByText(/发送成功/)).toBeInTheDocument());
  });

  it('shows error result', async () => {
    const onTest = vi.fn().mockResolvedValue({ success: false, message: 'Webhook 不可达' });
    render(<TestNotificationButton onTest={onTest} />);
    fireEvent.click(screen.getByText('测试通知'));
    await waitFor(() => expect(screen.getByText(/不可达/)).toBeInTheDocument());
  });
});
```

### 1.6 Alert Store 测试

```tsx
// src/store/__tests__/alertStore.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useAlertStore } from '../alertStore';
import * as alertsApi from '../../api/alerts';

vi.mock('../../api/alerts');

describe('alertStore', () => {
  beforeEach(() => {
    useAlertStore.getState().reset();
    vi.clearAllMocks();
  });

  it('fetchRules loads rules and sets loading state', async () => {
    const mockRules = [{ ruleId: 'task_timeout', name: '任务超时', enabled: true }];
    vi.mocked(alertsApi.getAlertRules).mockResolvedValue(mockRules);

    await useAlertStore.getState().fetchRules();

    expect(useAlertStore.getState().rules).toEqual(mockRules);
    expect(useAlertStore.getState().rulesLoading).toBe(false);
  });

  it('fetchRules sets error on failure', async () => {
    vi.mocked(alertsApi.getAlertRules).mockRejectedValue(new Error('Network error'));

    await useAlertStore.getState().fetchRules();

    expect(useAlertStore.getState().rulesError).toBe('Network error');
  });

  it('fetchHistory loads paginated data', async () => {
    const mockData = { records: [], totalCount: 0, pageNo: 1, pageSize: 20, pages: 0 };
    vi.mocked(alertsApi.getAlertHistory).mockResolvedValue(mockData);

    await useAlertStore.getState().fetchHistory({});

    expect(useAlertStore.getState().history).toEqual(mockData);
  });

  it('updateRules calls API and refreshes', async () => {
    vi.mocked(alertsApi.updateAlertRules).mockResolvedValue([]);
    vi.mocked(alertsApi.getAlertRules).mockResolvedValue([]);

    await useAlertStore.getState().updateRules({ rules: [{ ruleId: 'task_timeout', enabled: false }] });

    expect(alertsApi.updateAlertRules).toHaveBeenCalled();
  });

  it('testNotification returns result', async () => {
    const mockResult = { success: true, channelType: 'dingtalk', message: 'ok', durationMs: 100 };
    vi.mocked(alertsApi.testNotification).mockResolvedValue(mockResult);

    const result = await useAlertStore.getState().testNotification({ channelId: 'ch_1' });

    expect(result).toEqual(mockResult);
  });
});
```

### 1.7 API Client 测试

```tsx
// src/api/__tests__/alerts.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { getAlertRules, getAlertHistory, updateAlertRules } from '../alerts';
import client from '../client';

vi.mock('../client');

describe('alerts API', () => {
  beforeEach(() => vi.clearAllMocks());

  it('getAlertRules calls correct endpoint', async () => {
    vi.mocked(client.get).mockResolvedValue({ data: { data: [] } });
    await getAlertRules();
    expect(client.get).toHaveBeenCalledWith('/admin/alerts/rules');
  });

  it('getAlertHistory passes query params', async () => {
    vi.mocked(client.get).mockResolvedValue({ data: { data: { records: [] } } });
    await getAlertHistory({ severity: 'critical', page_no: 2 });
    expect(client.get).toHaveBeenCalledWith('/admin/alerts', { params: { severity: 'critical', page_no: 2 } });
  });

  it('updateAlertRules sends PUT with body', async () => {
    vi.mocked(client.put).mockResolvedValue({ data: { data: [] } });
    const req = { rules: [{ ruleId: 'task_timeout', enabled: false }] };
    await updateAlertRules(req);
    expect(client.put).toHaveBeenCalledWith('/admin/alerts/rules', req);
  });
});
```

---

## 2. 集成测试 (Vitest + MSW)

### 2.1 MSW Handlers

```tsx
// src/test/mocks/handlers.ts (Phase 5 扩展)
import { http, HttpResponse } from 'msw';

const mockRules = [
  { ruleId: 'task_timeout', name: '任务超时', severity: 'warning', enabled: true, thresholdValue: 30, thresholdUnit: 'minutes', cooldownSeconds: 300 },
  { ruleId: 'task_failure', name: '任务失败', severity: 'critical', enabled: true, thresholdValue: 1, thresholdUnit: 'count', cooldownSeconds: 300 },
  // ... 6 rules total
];

export const phase5Handlers = [
  http.get('/api/admin/alerts/rules', () => {
    return HttpResponse.json({ data: mockRules, success: true, retCode: '0', retMsg: 'ok' });
  }),
  http.put('/api/admin/alerts/rules', async ({ request }) => {
    const body = await request.json();
    return HttpResponse.json({ data: mockRules, success: true, retCode: '0', retMsg: 'ok' });
  }),
  http.get('/api/admin/alerts/channels', () => {
    return HttpResponse.json({ data: [], success: true, retCode: '0', retMsg: 'ok' });
  }),
  http.get('/api/admin/alerts', ({ request }) => {
    const url = new URL(request.url);
    return HttpResponse.json({
      data: { records: [], totalCount: 0, pageNo: 1, pageSize: 20, pages: 0 },
      success: true, retCode: '0', retMsg: 'ok'
    });
  }),
  http.post('/api/admin/alerts/test', () => {
    return HttpResponse.json({
      data: { success: true, channelType: 'dingtalk', message: '测试通知发送成功', durationMs: 120 },
      success: true, retCode: '0', retMsg: 'ok'
    });
  }),
];
```

### 2.2 AdminAlerts 页面集成测试

```tsx
// src/pages/__tests__/AdminAlerts.test.tsx
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { setupServer } from 'msw/node';
import { phase5Handlers } from '../../test/mocks/handlers';
import AdminAlerts from '../AdminAlerts';
import { MemoryRouter } from 'react-router-dom';

const server = setupServer(...phase5Handlers);
beforeAll(() => server.listen());
afterAll(() => server.close());

describe('AdminAlerts Page', () => {
  const renderPage = () => render(
    <MemoryRouter><AdminAlerts /></MemoryRouter>
  );

  it('renders with history tab by default', async () => {
    renderPage();
    await waitFor(() => expect(screen.getByText('告警历史')).toBeInTheDocument());
  });

  it('switches to rules tab', async () => {
    renderPage();
    fireEvent.click(screen.getByText('告警规则'));
    await waitFor(() => expect(screen.getByText('任务超时')).toBeInTheDocument());
  });

  it('switches to channels tab', async () => {
    renderPage();
    fireEvent.click(screen.getByText('通知渠道'));
    await waitFor(() => expect(screen.getByText(/添加渠道|暂无/)).toBeInTheDocument());
  });

  it('rule save shows success toast', async () => {
    renderPage();
    fireEvent.click(screen.getByText('告警规则'));
    await waitFor(() => screen.getByText('任务超时'));
    fireEvent.click(screen.getByText('保存'));
    await waitFor(() => expect(screen.getByText(/保存成功/)).toBeInTheDocument());
  });
});
```

---

## 3. E2E 测试 (Playwright)

### 3.1 Playwright 配置

```typescript
// playwright.config.ts (扩展)
export default defineConfig({
  projects: [
    { name: 'phase5-alerts', testMatch: /.*alerts.*\.spec\.ts/ },
  ],
});
```

### 3.2 告警管理 E2E 测试

```typescript
// e2e/alerts.spec.ts
import { test, expect } from '@playwright/test';

test.describe('Alert Management', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/login');
    await page.fill('[name=username]', 'admin');
    await page.fill('[name=password]', 'admin123');
    await page.click('button[type=submit]');
    await page.waitForURL('/projects');
    await page.goto('/admin/alerts');
  });

  test('view alert history with empty state', async ({ page }) => {
    await expect(page.getByText('告警历史')).toBeVisible();
    await expect(page.getByText(/暂无告警/)).toBeVisible();
  });

  test('configure alert rules', async ({ page }) => {
    await page.click('text=告警规则');
    await expect(page.getByText('任务超时')).toBeVisible();

    // Disable a rule
    const toggle = page.locator('[data-rule-id="task_timeout"] input[type="checkbox"]');
    await toggle.click();

    // Change threshold
    const thresholdInput = page.locator('[data-rule-id="task_timeout"] input[type="number"]').first();
    await thresholdInput.fill('45');

    // Save
    await page.click('text=保存');
    await expect(page.getByText(/保存成功/)).toBeVisible();
  });

  test('configure dingtalk channel', async ({ page }) => {
    await page.click('text=通知渠道');
    await page.click('text=添加渠道');

    await page.fill('[name=name]', '研发告警群');
    await page.fill('[name=webhookUrl]', 'https://oapi.dingtalk.com/robot/send?access_token=test');
    await page.fill('[name=secret]', 'SEC123456');

    // Select severity filters
    await page.check('label:has-text("严重") input');
    await page.check('label:has-text("警告") input');

    await page.click('text=保存');
    await expect(page.getByText(/保存成功/)).toBeVisible();
  });

  test('test notification', async ({ page }) => {
    await page.click('text=通知渠道');
    // Assuming channel already configured
    await page.click('text=测试通知');
    await expect(page.getByText(/发送成功|发送失败/)).toBeVisible({ timeout: 10000 });
  });

  test('alert history filtering', async ({ page }) => {
    // Select severity filter
    await page.click('[data-testid="severity-filter"]');
    await page.click('text=严重');

    // Verify filter applied
    await expect(page.url()).toContain('severity=critical');
  });

  test('non-admin cannot access alerts page', async ({ page, context }) => {
    // Login as regular user
    await context.clearCookies();
    await page.goto('/login');
    await page.fill('[name=username]', 'user1');
    await page.fill('[name=password]', 'user123');
    await page.click('button[type=submit]');

    await page.goto('/admin/alerts');
    // Should redirect or show 403
    await expect(page).not.toHaveURL('/admin/alerts');
  });
});
```

---

## 4. CI 集成

```yaml
# .gitlab-ci.yml
test-phase5-frontend-unit:
  stage: test
  script:
    - cd web-frontend
    - npm ci
    - npx vitest run --reporter=verbose src/components/alerts/ src/store/__tests__/alertStore src/api/__tests__/alerts

test-phase5-frontend-e2e:
  stage: test
  script:
    - cd web-frontend
    - npm ci
    - npx playwright install --with-deps chromium
    - npx playwright test e2e/alerts.spec.ts
  variables:
    BASE_URL: http://localhost:3100
```
