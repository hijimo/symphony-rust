import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, it, expect, beforeEach } from "vitest";
import { ThemeProvider } from "@mui/material/styles";
import { http, HttpResponse } from "msw";
import theme from "../../theme";
import AdminConfig from "../AdminConfig";
import { server } from "../../test/mocks/server";
import { useAuthStore } from "../../store/auth";

const BASE_URL = "*/api";

type SecretAction = "keep" | "set" | "clear";

interface SecretUpdate {
  action: SecretAction;
  value?: string;
}

interface UpdateProxyBody {
  expectedVersion: string;
  mode: "disabled" | "inherit_env" | "manual";
  httpProxy: SecretUpdate;
  httpsProxy: SecretUpdate;
  allProxy: SecretUpdate;
  noProxy: string;
  autoBypassLocal: boolean;
}

interface TestProxyBody {
  targetId: string;
  useDraftConfig: boolean;
  draftConfig?: UpdateProxyBody;
}

function proxyConfig(overrides = {}) {
  return {
    mode: "disabled",
    version: "proxy-v1",
    source: "database",
    httpProxy: {
      configured: true,
      displayValue: "http://us***@proxy.internal:8080",
      updatedAt: "2026-05-01T00:00:00Z",
    },
    httpsProxy: {
      configured: false,
      displayValue: "",
      updatedAt: null,
    },
    allProxy: {
      configured: true,
      displayValue: "socks5://so***@proxy.internal:1080",
      updatedAt: "2026-05-01T00:00:00Z",
    },
    noProxy: "localhost,127.0.0.1",
    autoBypassLocal: true,
    needsRestartProjectCount: 3,
    updatedAt: "2026-05-01T00:00:00Z",
    warnings: [],
    ...overrides,
  };
}

function ok<T>(data: T) {
  return HttpResponse.json({
    success: true,
    retCode: "SUCCESS",
    retMsg: "ok",
    data,
  });
}

function renderAdminConfig() {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={["/admin/config"]}>
        <AdminConfig />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

function installBaseHandlers(initialProxy = proxyConfig()) {
  let currentProxy = initialProxy;
  let latestUpdateBody: UpdateProxyBody | null = null;

  server.use(
    http.get(`${BASE_URL}/admin/config`, () =>
      ok([
        {
          key: "global_concurrency_limit",
          value: "10",
          description: "全局并发上限",
          updatedAt: "2026-05-01T00:00:00Z",
        },
        {
          key: "network_proxy.http_url",
          value: "http://should-not-render",
          description: "历史误写入代理配置",
          updatedAt: "2026-05-01T00:00:00Z",
        },
      ]),
    ),
    http.get(`${BASE_URL}/admin/stats`, () =>
      ok({
        totalProjects: 6,
        runningServices: 4,
        totalUsers: 2,
        globalConcurrencyLimit: 10,
        globalConcurrencyUsed: 1,
      }),
    ),
    http.get(`${BASE_URL}/admin/network-proxy`, () => ok(currentProxy)),
    http.put(`${BASE_URL}/admin/network-proxy`, async ({ request }) => {
      latestUpdateBody = (await request.json()) as UpdateProxyBody;
      currentProxy = proxyConfig({
        mode: latestUpdateBody.mode,
        version: "proxy-v2",
        httpProxy:
          latestUpdateBody.httpProxy.action === "set"
            ? {
                configured: true,
                displayValue: "http://ne***@proxy.internal:8080",
                updatedAt: "2026-05-02T00:00:00Z",
              }
            : latestUpdateBody.httpProxy.action === "clear"
              ? { configured: false, displayValue: "", updatedAt: null }
              : currentProxy.httpProxy,
        httpsProxy:
          latestUpdateBody.httpsProxy.action === "set"
            ? {
                configured: true,
                displayValue: "http://ht***@proxy.internal:8443",
                updatedAt: "2026-05-02T00:00:00Z",
              }
            : latestUpdateBody.httpsProxy.action === "clear"
              ? { configured: false, displayValue: "", updatedAt: null }
              : currentProxy.httpsProxy,
        allProxy:
          latestUpdateBody.allProxy.action === "set"
            ? {
                configured: true,
                displayValue: "socks5://ne***@proxy.internal:1080",
                updatedAt: "2026-05-02T00:00:00Z",
              }
            : latestUpdateBody.allProxy.action === "clear"
              ? { configured: false, displayValue: "", updatedAt: null }
              : currentProxy.allProxy,
      });
      return ok(currentProxy);
    }),
  );

  return {
    getLatestUpdateBody: () => latestUpdateBody,
  };
}

describe("AdminConfig network proxy panel", () => {
  beforeEach(() => {
    useAuthStore.setState({
      token: "mock-token",
      expiresAt: "2099-01-01T00:00:00Z",
      user: {
        id: 1,
        username: "admin",
        displayName: "Administrator",
        role: "admin",
      },
      isAuthenticated: true,
    });
    localStorage.setItem("token", "mock-token");
    localStorage.setItem("expiresAt", "2099-01-01T00:00:00Z");
  });

  it("按代理模式启用或禁用手动代理字段，并展示受影响项目数量", async () => {
    const user = userEvent.setup();
    installBaseHandlers();

    renderAdminConfig();

    const httpInput = await screen.findByLabelText("HTTP 代理");
    expect(httpInput).toBeDisabled();
    expect(screen.getByText("需重启服务：3")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存代理" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "手动配置" }));

    expect(httpInput).toBeEnabled();
    expect(screen.getByLabelText("HTTPS 代理")).toBeEnabled();
    expect(screen.getByLabelText("ALL 代理")).toBeEnabled();
    expect(screen.getByRole("button", { name: "保存代理" })).toBeEnabled();
  });

  it("保存手动代理后展示后端返回的脱敏值", async () => {
    const user = userEvent.setup();
    installBaseHandlers();

    renderAdminConfig();

    await screen.findByLabelText("HTTP 代理");
    await user.click(screen.getByRole("button", { name: "手动配置" }));
    const httpInput = screen.getByLabelText("HTTP 代理");
    await user.clear(httpInput);
    await user.type(httpInput, "http://new-secret@proxy.internal:8080");

    await user.click(screen.getByRole("button", { name: "保存代理" }));

    await waitFor(() => {
      expect(screen.getByText("网络代理配置已保存")).toBeInTheDocument();
    });
    expect(screen.getByLabelText("HTTP 代理")).toHaveValue(
      "http://ne***@proxy.internal:8080",
    );
  });

  it("提交 secret keep、set、clear 三种动作", async () => {
    const user = userEvent.setup();
    const harness = installBaseHandlers(proxyConfig({ mode: "manual" }));

    renderAdminConfig();

    await screen.findByLabelText("HTTP 代理");
    await user.type(
      screen.getByLabelText("HTTPS 代理"),
      "http://https-secret@proxy.internal:8443",
    );
    await user.clear(screen.getByLabelText("ALL 代理"));

    await user.click(screen.getByRole("button", { name: "保存代理" }));

    await waitFor(() => {
      expect(harness.getLatestUpdateBody()).not.toBeNull();
    });
    expect(harness.getLatestUpdateBody()?.httpProxy).toEqual({
      action: "keep",
    });
    expect(harness.getLatestUpdateBody()?.httpsProxy).toEqual({
      action: "set",
      value: "http://https-secret@proxy.internal:8443",
    });
    expect(harness.getLatestUpdateBody()?.allProxy).toEqual({
      action: "clear",
    });
  });

  it("未配置的 secret 输入脱敏占位符时不提交 set", async () => {
    const user = userEvent.setup();
    const harness = installBaseHandlers(
      proxyConfig({
        mode: "manual",
        httpProxy: { configured: false, displayValue: "", updatedAt: null },
      }),
    );

    renderAdminConfig();

    const httpInput = await screen.findByLabelText("HTTP 代理");
    await user.type(httpInput, "http://***@proxy.internal:8080");
    await user.click(screen.getByRole("button", { name: "保存代理" }));

    await waitFor(() => {
      expect(
        screen.getByText("不能保存脱敏占位符，请输入完整代理地址"),
      ).toBeInTheDocument();
    });
    expect(harness.getLatestUpdateBody()).toBeNull();
  });

  it("展示测试连接 loading、成功和失败状态", async () => {
    const user = userEvent.setup();
    installBaseHandlers(proxyConfig({ mode: "manual" }));
    let testCount = 0;
    let latestTestBody: TestProxyBody | null = null;
    server.use(
      http.post(`${BASE_URL}/admin/network-proxy/test`, async ({ request }) => {
        latestTestBody = (await request.json()) as TestProxyBody;
        testCount += 1;
        await new Promise((resolve) => setTimeout(resolve, 30));
        if (testCount === 1) {
          return ok({
            status: "success",
            targetHost: "github.com",
            proxyUsed: true,
            proxySummary: "HTTP proxy",
            durationMs: 42,
            message: "连接成功",
          });
        }
        return ok({
          status: "failed",
          targetHost: "github.com",
          proxyUsed: true,
          proxySummary: "HTTP proxy",
          durationMs: 51,
          message: "代理连接失败",
        });
      }),
    );

    renderAdminConfig();

    await screen.findByLabelText("HTTP 代理");
    await user.type(
      screen.getByLabelText("HTTPS 代理"),
      "http://draft-secret@proxy.internal:8443",
    );
    await user.click(screen.getByRole("button", { name: "测试连接" }));
    expect(screen.getByRole("button", { name: "测试中" })).toBeDisabled();
    await waitFor(() => {
      expect(screen.getByText(/github\.com：连接成功/)).toBeInTheDocument();
    });
    expect(latestTestBody).toMatchObject({
      targetId: "github",
      useDraftConfig: true,
      draftConfig: {
        expectedVersion: "proxy-v1",
        mode: "manual",
        httpProxy: { action: "keep" },
        httpsProxy: {
          action: "set",
          value: "http://draft-secret@proxy.internal:8443",
        },
        allProxy: { action: "keep" },
        noProxy: "localhost,127.0.0.1",
        autoBypassLocal: true,
      },
    });

    await user.click(screen.getByRole("button", { name: "测试连接" }));
    await waitFor(() => {
      expect(screen.getByText(/github\.com：代理连接失败/)).toBeInTheDocument();
    });
  });

  it("通用配置表不展示 network_proxy.* 配置项", async () => {
    installBaseHandlers();

    renderAdminConfig();

    await screen.findByText("global_concurrency_limit");
    const table = screen.getByRole("table");
    expect(
      within(table).queryByText("network_proxy.http_url"),
    ).not.toBeInTheDocument();
  });
});
