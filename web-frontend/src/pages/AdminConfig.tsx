import { useState, useEffect, useCallback } from "react";
import {
  Box,
  Paper,
  Typography,
  Button,
  TextField,
  Snackbar,
  Alert,
  CircularProgress,
  Divider,
  FormControlLabel,
  MenuItem,
  Switch,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Chip,
  IconButton,
  Tooltip,
  ToggleButton,
  ToggleButtonGroup,
} from "@mui/material";
import {
  NetworkCheck,
  Save,
  Refresh,
  Settings,
  TrendingUp,
} from "@mui/icons-material";
import {
  getSystemConfig,
  updateSystemConfig,
  getSystemStats,
} from "../api/adminConfig";
import type { SystemConfigItem, SystemStats } from "../api/adminConfig";
import {
  getNetworkProxy,
  testNetworkProxyDraft,
  updateNetworkProxy,
} from "../api/adminNetworkProxy";
import type {
  NetworkProxyConfig,
  ProxyMode,
  ProxySecretDisplay,
  ProxyTestResult,
  SecretUpdate,
  UpdateNetworkProxyRequest,
} from "../api/adminNetworkProxy";

interface SnackState {
  open: boolean;
  message: string;
  severity: "success" | "error";
}

interface EditableConfig extends SystemConfigItem {
  editValue: string;
  dirty: boolean;
}

function isNetworkProxyConfigKey(key: string) {
  return key.startsWith("network_proxy.");
}

function toEditableConfigs(items: SystemConfigItem[]): EditableConfig[] {
  return items
    .filter((c) => !isNetworkProxyConfigKey(c.key))
    .map((c) => ({
      ...c,
      editValue: c.value,
      dirty: false,
    }));
}

export default function AdminConfig() {
  const [configs, setConfigs] = useState<EditableConfig[]>([]);
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [statsLoading, setStatsLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [snack, setSnack] = useState<SnackState>({
    open: false,
    message: "",
    severity: "success",
  });

  const fetchConfigs = useCallback(async () => {
    setLoading(true);
    try {
      const data = await getSystemConfig();
      setConfigs(toEditableConfigs(data));
    } catch (err) {
      setSnack({
        open: true,
        message: (err as Error).message,
        severity: "error",
      });
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchStats = useCallback(async () => {
    setStatsLoading(true);
    try {
      const data = await getSystemStats();
      setStats(data);
    } catch (err) {
      setSnack({
        open: true,
        message: (err as Error).message,
        severity: "error",
      });
    } finally {
      setStatsLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchConfigs();
    fetchStats();
  }, [fetchConfigs, fetchStats]);

  const handleValueChange = (key: string, newValue: string) => {
    setConfigs((prev) =>
      prev.map((c) =>
        c.key === key
          ? { ...c, editValue: newValue, dirty: newValue !== c.value }
          : c,
      ),
    );
  };

  const handleSave = async () => {
    const dirtyConfigs = configs.filter((c) => c.dirty);
    if (dirtyConfigs.length === 0) return;

    setSaving(true);
    try {
      const updated = await updateSystemConfig({
        configs: dirtyConfigs.map((c) => ({ key: c.key, value: c.editValue })),
      });
      setConfigs(toEditableConfigs(updated));
      setSnack({ open: true, message: "配置已保存", severity: "success" });
      // Refresh stats since config changes may affect them
      fetchStats();
    } catch (err) {
      setSnack({
        open: true,
        message: (err as Error).message,
        severity: "error",
      });
    } finally {
      setSaving(false);
    }
  };

  const hasDirty = configs.some((c) => c.dirty);

  return (
    <Box sx={{ p: 3, maxWidth: 1200, mx: "auto" }}>
      {/* Header */}
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          mb: 3,
        }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5 }}>
          <Settings sx={{ fontSize: 28, color: "text.secondary" }} />
          <Typography variant="h5" fontWeight={600}>
            系统配置
          </Typography>
        </Box>
        <Box sx={{ display: "flex", gap: 1 }}>
          <Tooltip title="刷新">
            <span>
              <IconButton
                onClick={() => {
                  fetchConfigs();
                  fetchStats();
                }}
                disabled={loading}
              >
                <Refresh />
              </IconButton>
            </span>
          </Tooltip>
          <Button
            variant="contained"
            startIcon={saving ? <CircularProgress size={18} /> : <Save />}
            onClick={handleSave}
            disabled={!hasDirty || saving}
            sx={{
              bgcolor: hasDirty ? "#0053db" : undefined,
              "&:hover": { bgcolor: hasDirty ? "#0048c1" : undefined },
              borderRadius: "4px",
              textTransform: "none",
            }}
          >
            保存修改
          </Button>
        </Box>
      </Box>

      {/* Stats Cards */}
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))",
          gap: 2,
          mb: 3,
        }}
      >
        <StatCard
          label="项目总数"
          value={stats?.totalProjects}
          loading={statsLoading}
        />
        <StatCard
          label="运行中服务"
          value={stats?.runningServices}
          loading={statsLoading}
          color="#4caf50"
        />
        <StatCard
          label="用户总数"
          value={stats?.totalUsers}
          loading={statsLoading}
        />
        <StatCard
          label="并发使用"
          value={
            stats
              ? `${stats.globalConcurrencyUsed} / ${stats.globalConcurrencyLimit}`
              : undefined
          }
          loading={statsLoading}
          color="#ff9800"
        />
      </Box>

      <NetworkProxyPanel onNotify={setSnack} />

      {/* Config Table */}
      <Paper
        sx={{ borderRadius: "8px", overflow: "hidden" }}
        elevation={0}
        variant="outlined"
      >
        <TableContainer>
          <Table>
            <TableHead>
              <TableRow sx={{ bgcolor: "action.hover" }}>
                <TableCell sx={{ fontWeight: 600, width: "25%" }}>
                  配置项
                </TableCell>
                <TableCell sx={{ fontWeight: 600, width: "30%" }}>值</TableCell>
                <TableCell sx={{ fontWeight: 600, width: "30%" }}>
                  说明
                </TableCell>
                <TableCell sx={{ fontWeight: 600, width: "15%" }}>
                  更新时间
                </TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {loading ? (
                <TableRow>
                  <TableCell colSpan={4} align="center" sx={{ py: 4 }}>
                    <CircularProgress size={32} />
                  </TableCell>
                </TableRow>
              ) : configs.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={4}
                    align="center"
                    sx={{ py: 4, color: "text.secondary" }}
                  >
                    暂无配置项
                  </TableCell>
                </TableRow>
              ) : (
                configs.map((config) => (
                  <TableRow key={config.key} hover>
                    <TableCell>
                      <Box
                        sx={{ display: "flex", alignItems: "center", gap: 1 }}
                      >
                        <Typography
                          variant="body2"
                          fontFamily="monospace"
                          fontWeight={500}
                        >
                          {config.key}
                        </Typography>
                        {config.dirty && (
                          <Chip
                            label="已修改"
                            size="small"
                            color="warning"
                            variant="outlined"
                          />
                        )}
                      </Box>
                    </TableCell>
                    <TableCell>
                      <TextField
                        size="small"
                        value={config.editValue}
                        onChange={(e) =>
                          handleValueChange(config.key, e.target.value)
                        }
                        variant="filled"
                        hiddenLabel
                        sx={{
                          width: "100%",
                          "& .MuiFilledInput-root": {
                            borderRadius: "4px",
                            bgcolor: config.dirty ? "warning.50" : undefined,
                          },
                        }}
                        inputProps={{ style: { fontFamily: "monospace" } }}
                      />
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2" color="text.secondary">
                        {config.description || "-"}
                      </Typography>
                    </TableCell>
                    <TableCell>
                      <Typography variant="body2" color="text.secondary">
                        {config.updatedAt}
                      </Typography>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </TableContainer>
      </Paper>

      {/* Snackbar */}
      <Snackbar
        open={snack.open}
        autoHideDuration={4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: "top", horizontal: "center" }}
      >
        <Alert
          severity={snack.severity}
          onClose={() => setSnack((s) => ({ ...s, open: false }))}
          variant="filled"
        >
          {snack.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}

interface NetworkProxyForm {
  mode: ProxyMode;
  httpProxy: string;
  httpsProxy: string;
  allProxy: string;
  noProxy: string;
  autoBypassLocal: boolean;
  targetId: string;
}

interface NetworkProxyPanelProps {
  onNotify: (state: SnackState) => void;
}

function NetworkProxyPanel({ onNotify }: NetworkProxyPanelProps) {
  const [config, setConfig] = useState<NetworkProxyConfig | null>(null);
  const [form, setForm] = useState<NetworkProxyForm>({
    mode: "inherit_env",
    httpProxy: "",
    httpsProxy: "",
    allProxy: "",
    noProxy: "",
    autoBypassLocal: true,
    targetId: "github",
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ProxyTestResult | null>(null);

  const applyConfig = useCallback((next: NetworkProxyConfig) => {
    setConfig(next);
    setForm((prev) => ({
      ...prev,
      mode: next.mode,
      httpProxy: next.httpProxy.displayValue,
      httpsProxy: next.httpsProxy.displayValue,
      allProxy: next.allProxy.displayValue,
      noProxy: next.noProxy,
      autoBypassLocal: next.autoBypassLocal,
    }));
  }, []);

  const fetchProxy = useCallback(async () => {
    setLoading(true);
    try {
      applyConfig(await getNetworkProxy());
    } catch (err) {
      onNotify({
        open: true,
        message: (err as Error).message,
        severity: "error",
      });
    } finally {
      setLoading(false);
    }
  }, [applyConfig, onNotify]);

  useEffect(() => {
    fetchProxy();
  }, [fetchProxy]);

  const secretUpdate = (
    value: string,
    original: ProxySecretDisplay,
  ): SecretUpdate => {
    const trimmed = value.trim();
    if (!trimmed) return { action: "clear" };
    if (original.configured && trimmed === original.displayValue)
      return { action: "keep" };
    return { action: "set", value: trimmed };
  };

  const hasMaskedPlaceholderInput = () => {
    const candidates: Array<[string, ProxySecretDisplay]> = [
      [
        form.httpProxy,
        config?.httpProxy ?? {
          configured: false,
          displayValue: "",
          updatedAt: null,
        },
      ],
      [
        form.httpsProxy,
        config?.httpsProxy ?? {
          configured: false,
          displayValue: "",
          updatedAt: null,
        },
      ],
      [
        form.allProxy,
        config?.allProxy ?? {
          configured: false,
          displayValue: "",
          updatedAt: null,
        },
      ],
    ];

    return candidates.some(([value, original]) => {
      const trimmed = value.trim();
      return trimmed.includes("***") && trimmed !== original.displayValue;
    });
  };

  const buildProxyRequest = (): UpdateNetworkProxyRequest | null => {
    if (!config) return null;
    return {
      expectedVersion: config.version,
      mode: form.mode,
      httpProxy: secretUpdate(form.httpProxy, config.httpProxy),
      httpsProxy: secretUpdate(form.httpsProxy, config.httpsProxy),
      allProxy: secretUpdate(form.allProxy, config.allProxy),
      noProxy: form.noProxy,
      autoBypassLocal: form.autoBypassLocal,
    };
  };

  const hasProxyChanges = Boolean(
    config &&
    (form.mode !== config.mode ||
      form.httpProxy !== config.httpProxy.displayValue ||
      form.httpsProxy !== config.httpsProxy.displayValue ||
      form.allProxy !== config.allProxy.displayValue ||
      form.noProxy !== config.noProxy ||
      form.autoBypassLocal !== config.autoBypassLocal),
  );

  const validateProxyDraft = () => {
    if (
      form.mode === "manual" &&
      !form.httpProxy.trim() &&
      !form.httpsProxy.trim() &&
      !form.allProxy.trim()
    ) {
      onNotify({
        open: true,
        message: "手动模式至少需要一个代理地址",
        severity: "error",
      });
      return false;
    }
    if (hasMaskedPlaceholderInput()) {
      onNotify({
        open: true,
        message: "不能保存脱敏占位符，请输入完整代理地址",
        severity: "error",
      });
      return false;
    }
    return true;
  };

  const handleSaveProxy = async () => {
    if (!config) return;
    if (!validateProxyDraft()) return;
    const request = buildProxyRequest();
    if (!request) return;

    setSaving(true);
    try {
      const updated = await updateNetworkProxy(request);
      applyConfig(updated);
      onNotify({
        open: true,
        message: "网络代理配置已保存",
        severity: "success",
      });
    } catch (err) {
      onNotify({
        open: true,
        message: (err as Error).message,
        severity: "error",
      });
    } finally {
      setSaving(false);
    }
  };

  const handleTestProxy = async () => {
    if (!validateProxyDraft()) return;
    const request = buildProxyRequest();
    if (!request) return;

    setTesting(true);
    try {
      setTestResult(
        await testNetworkProxyDraft({
          targetId: form.targetId,
          useDraftConfig: true,
          draftConfig: request,
        }),
      );
    } catch (err) {
      onNotify({
        open: true,
        message: (err as Error).message,
        severity: "error",
      });
    } finally {
      setTesting(false);
    }
  };

  const manualDisabled = form.mode !== "manual";

  return (
    <Paper
      sx={{ p: 2.5, borderRadius: "8px", mb: 3 }}
      elevation={0}
      variant="outlined"
    >
      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 2,
          mb: 2,
        }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <NetworkCheck color="primary" />
          <Typography variant="h6" fontWeight={600}>
            网络代理
          </Typography>
          {config && (
            <Chip
              size="small"
              label={modeLabel(config.mode)}
              color={modeColor(config.mode)}
            />
          )}
        </Box>
        <Button startIcon={<Refresh />} onClick={fetchProxy} disabled={loading}>
          刷新
        </Button>
      </Box>

      {config?.warnings.map((warning) => (
        <Alert key={warning.code} severity={warning.severity} sx={{ mb: 2 }}>
          {warning.message}
        </Alert>
      ))}

      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: { xs: "1fr", md: "220px 1fr" },
          gap: 2,
          mb: 2,
        }}
      >
        <Typography variant="body2" color="text.secondary">
          当前状态
        </Typography>
        <Box sx={{ display: "flex", flexWrap: "wrap", gap: 1 }}>
          <Chip size="small" label={`来源：${config?.source ?? "-"}`} />
          <Chip size="small" label={`版本：${config?.version ?? "-"}`} />
          <Chip
            size="small"
            color={
              (config?.needsRestartProjectCount ?? 0) > 0
                ? "warning"
                : "default"
            }
            label={`需重启服务：${config?.needsRestartProjectCount ?? 0}`}
          />
        </Box>

        <Typography variant="body2" color="text.secondary">
          代理模式
        </Typography>
        <ToggleButtonGroup
          exclusive
          size="small"
          value={form.mode}
          onChange={(_, value: ProxyMode | null) =>
            value && setForm((prev) => ({ ...prev, mode: value }))
          }
        >
          <ToggleButton value="disabled">禁用</ToggleButton>
          <ToggleButton value="inherit_env">继承环境变量</ToggleButton>
          <ToggleButton value="manual">手动配置</ToggleButton>
        </ToggleButtonGroup>

        <Typography variant="body2" color="text.secondary">
          手动代理
        </Typography>
        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: { xs: "1fr", md: "repeat(3, 1fr)" },
            gap: 1.5,
          }}
        >
          <TextField
            size="small"
            label="HTTP 代理"
            value={form.httpProxy}
            disabled={manualDisabled}
            onChange={(e) =>
              setForm((prev) => ({ ...prev, httpProxy: e.target.value }))
            }
          />
          <TextField
            size="small"
            label="HTTPS 代理"
            value={form.httpsProxy}
            disabled={manualDisabled}
            onChange={(e) =>
              setForm((prev) => ({ ...prev, httpsProxy: e.target.value }))
            }
          />
          <TextField
            size="small"
            label="ALL 代理"
            value={form.allProxy}
            disabled={manualDisabled}
            onChange={(e) =>
              setForm((prev) => ({ ...prev, allProxy: e.target.value }))
            }
          />
        </Box>

        <Typography variant="body2" color="text.secondary">
          绕过代理
        </Typography>
        <Box>
          <TextField
            fullWidth
            multiline
            minRows={2}
            size="small"
            value={form.noProxy}
            onChange={(e) =>
              setForm((prev) => ({ ...prev, noProxy: e.target.value }))
            }
            placeholder="localhost,127.0.0.1,::1,.example.com,10.0.0.0/8"
          />
          <FormControlLabel
            control={
              <Switch
                checked={form.autoBypassLocal}
                onChange={(e) =>
                  setForm((prev) => ({
                    ...prev,
                    autoBypassLocal: e.target.checked,
                  }))
                }
              />
            }
            label="自动绕过本机地址"
          />
        </Box>
      </Box>

      <Divider sx={{ my: 2 }} />

      <Box
        sx={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: 1.5,
        }}
      >
        <TextField
          select
          size="small"
          label="测试目标"
          value={form.targetId}
          onChange={(e) =>
            setForm((prev) => ({ ...prev, targetId: e.target.value }))
          }
          sx={{ minWidth: 180 }}
        >
          <MenuItem value="github">GitHub</MenuItem>
          <MenuItem value="gitlab">GitLab</MenuItem>
          <MenuItem value="linear">Linear</MenuItem>
          <MenuItem value="openai">OpenAI</MenuItem>
        </TextField>
        <Button
          variant="outlined"
          onClick={handleTestProxy}
          disabled={testing || loading}
        >
          {testing ? "测试中" : "测试连接"}
        </Button>
        <Button
          variant="contained"
          startIcon={saving ? <CircularProgress size={18} /> : <Save />}
          onClick={handleSaveProxy}
          disabled={saving || loading || !hasProxyChanges}
          sx={{
            bgcolor: "#0053db",
            "&:hover": { bgcolor: "#0048c1" },
            borderRadius: "4px",
          }}
        >
          保存代理
        </Button>
      </Box>

      {testResult && (
        <Alert
          severity={testResult.status === "success" ? "success" : "warning"}
          sx={{ mt: 2 }}
        >
          {testResult.targetHost || "测试目标"}：{testResult.message}（
          {testResult.durationMs}ms）
        </Alert>
      )}
    </Paper>
  );
}

function modeLabel(mode: ProxyMode) {
  if (mode === "disabled") return "禁用";
  if (mode === "manual") return "手动配置";
  return "继承环境变量";
}

function modeColor(mode: ProxyMode): "default" | "primary" | "warning" {
  if (mode === "disabled") return "default";
  if (mode === "manual") return "primary";
  return "warning";
}

interface StatCardProps {
  label: string;
  value: number | string | undefined;
  loading: boolean;
  color?: string;
}

function StatCard({ label, value, loading, color }: StatCardProps) {
  return (
    <Paper
      sx={{
        p: 2.5,
        borderRadius: "8px",
        display: "flex",
        alignItems: "center",
        gap: 2,
      }}
      elevation={0}
      variant="outlined"
    >
      <TrendingUp sx={{ color: color || "primary.main", fontSize: 32 }} />
      <Box>
        <Typography variant="body2" color="text.secondary">
          {label}
        </Typography>
        {loading ? (
          <CircularProgress size={20} />
        ) : (
          <Typography
            variant="h6"
            fontWeight={600}
            sx={{ color: color || "text.primary" }}
          >
            {value ?? "-"}
          </Typography>
        )}
      </Box>
    </Paper>
  );
}
