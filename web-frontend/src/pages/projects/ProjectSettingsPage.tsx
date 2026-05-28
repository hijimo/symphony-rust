import { useState, useEffect, useCallback } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import {
  Box,
  Typography,
  Card,
  CardContent,
  TextField,
  Button,
  Tabs,
  Tab,
  Snackbar,
  Alert,
  Skeleton,
  CircularProgress,
  Chip,
  Switch,
  FormControlLabel,
} from '@mui/material';
import { WarningAmber } from '@mui/icons-material';
import ConfirmDialog from '../../components/ConfirmDialog';
import WorkflowEditor from '../../components/WorkflowEditor';
import ServiceControlPanel from '../../components/ServiceControlPanel';
import { getProject, updateProject, deleteProject, getServiceStatus, startService, stopService, restartService } from '../../api/projects';
import { getWorkflow, updateWorkflow, resetWorkflow } from '../../api/workflow';
import type { Project, ServiceStatusData, WorkflowData } from '../../types';

interface SnackState {
  open: boolean;
  message: string;
  severity: 'success' | 'error';
}

interface TabPanelProps {
  children: React.ReactNode;
  value: number;
  index: number;
}

function TabPanel({ children, value, index }: TabPanelProps) {
  if (value !== index) return null;
  return (
    <Box role="tabpanel" sx={{ pt: 3 }}>
      {children}
    </Box>
  );
}

export default function ProjectSettingsPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const projectId = Number(id);

  const [tab, setTab] = useState(0);
  const [loading, setLoading] = useState(true);
  const [project, setProject] = useState<Project | null>(null);
  const [snack, setSnack] = useState<SnackState>({ open: false, message: '', severity: 'success' });

  // Basic info state
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [defaultBranch, setDefaultBranch] = useState('');
  const [basicSaving, setBasicSaving] = useState(false);

  // Workflow state
  const [workflow, setWorkflow] = useState<WorkflowData | null>(null);
  const [workflowLoading, setWorkflowLoading] = useState(false);
  const [workflowSaving, setWorkflowSaving] = useState(false);
  const [workflowResetting, setWorkflowResetting] = useState(false);
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false);

  // Service state
  const [serviceStatus, setServiceStatus] = useState<ServiceStatusData | null>(null);
  const [serviceLoading, setServiceLoading] = useState(false);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [autoRestart, setAutoRestart] = useState(true);
  const [maxAgents, setMaxAgents] = useState(2);
  const [configSaving, setConfigSaving] = useState(false);

  // Danger zone
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteLoading, setDeleteLoading] = useState(false);

  // Agent config state
  const [hooksAfterCreate, setHooksAfterCreate] = useState('');
  const [hooksBeforeRemove, setHooksBeforeRemove] = useState('');
  const [codexCommand, setCodexCommand] = useState('');
  const [codexApprovalPolicy, setCodexApprovalPolicy] = useState('never');
  const [codexSandbox, setCodexSandbox] = useState('workspace-write');
  const [agentConfigSaving, setAgentConfigSaving] = useState(false);

  // Testing agent config state
  const [testingEnabled, setTestingEnabled] = useState(false);
  const [testingMaxAttempts, setTestingMaxAttempts] = useState(3);
  const [testingMaxTurns, setTestingMaxTurns] = useState(12);
  const [testingSkipLabels, setTestingSkipLabels] = useState('');
  const [testingAllowedCommands, setTestingAllowedCommands] = useState('');
  const [testingConfigSaving, setTestingConfigSaving] = useState(false);

  const showSnack = (message: string, severity: 'success' | 'error') => {
    setSnack({ open: true, message, severity });
  };

  const loadProject = useCallback(async () => {
    try {
      const p = await getProject(projectId);
      setProject(p);
      setName(p.name);
      setDescription(p.description || '');
      setDefaultBranch(p.default_branch);
      setAutoRestart(p.auto_restart);
      setMaxAgents(p.max_concurrent_agents);
      setHooksAfterCreate(p.hooks_after_create || '');
      setHooksBeforeRemove(p.hooks_before_remove || '');
      setCodexCommand(p.codex_command || '');
      setCodexApprovalPolicy(p.codex_approval_policy || 'never');
      setCodexSandbox(p.codex_sandbox || 'workspace-write');
      setTestingEnabled(p.testing_enabled);
      setTestingMaxAttempts(p.testing_max_attempts);
      setTestingMaxTurns(p.testing_max_turns);
      setTestingSkipLabels(p.testing_skip_labels || '');
      setTestingAllowedCommands(p.testing_allowed_commands || '');
    } catch (err: any) {
      showSnack(err?.message || '加载项目失败', 'error');
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => {
    loadProject();
  }, [loadProject]);

  // Load workflow when tab changes
  useEffect(() => {
    if (tab === 1 && !workflow) {
      setWorkflowLoading(true);
      getWorkflow(projectId)
        .then(setWorkflow)
        .catch((err: any) => showSnack(err?.message || '加载工作流失败', 'error'))
        .finally(() => setWorkflowLoading(false));
    }
  }, [tab, projectId, workflow]);

  // Load service status when tab changes
  useEffect(() => {
    if (tab === 2 && !serviceStatus) {
      setServiceLoading(true);
      getServiceStatus(projectId)
        .then(setServiceStatus)
        .catch((err: any) => showSnack(err?.message || '加载服务状态失败', 'error'))
        .finally(() => setServiceLoading(false));
    }
  }, [tab, projectId, serviceStatus]);

  // Basic info save
  const handleBasicSave = async () => {
    setBasicSaving(true);
    try {
      const updated = await updateProject(projectId, {
        name: name.trim(),
        description: description.trim() || undefined,
        default_branch: defaultBranch.trim(),
      });
      setProject(updated);
      showSnack('项目信息已更新', 'success');
    } catch (err: any) {
      showSnack(err?.message || '更新失败', 'error');
    } finally {
      setBasicSaving(false);
    }
  };

  // Workflow save
  const handleWorkflowSave = async (mode: 'default' | 'custom', content: string) => {
    setWorkflowSaving(true);
    try {
      const updated = await updateWorkflow(projectId, {
        template_mode: mode,
        content: mode === 'custom' ? content : undefined,
      });
      setWorkflow(updated);
      showSnack('工作流配置已保存', 'success');
    } catch (err: any) {
      showSnack(err?.message || '保存失败', 'error');
    } finally {
      setWorkflowSaving(false);
    }
  };

  // Workflow reset
  const handleWorkflowReset = async () => {
    setResetConfirmOpen(false);
    setWorkflowResetting(true);
    try {
      const updated = await resetWorkflow(projectId);
      setWorkflow(updated);
      showSnack('已重置为默认模板', 'success');
    } catch (err: any) {
      showSnack(err?.message || '重置失败', 'error');
    } finally {
      setWorkflowResetting(false);
    }
  };

  // Service actions
  const handleServiceAction = async (action: 'start' | 'stop' | 'restart') => {
    setActionLoading(action);
    try {
      let result: ServiceStatusData;
      switch (action) {
        case 'start':
          result = await startService(projectId);
          break;
        case 'stop':
          result = await stopService(projectId);
          break;
        case 'restart':
          result = await restartService(projectId);
          break;
      }
      setServiceStatus(result);
      showSnack(
        action === 'start' ? '服务已启动' : action === 'stop' ? '服务已停止' : '服务已重启',
        'success',
      );
    } catch (err: any) {
      showSnack(err?.message || '操作失败', 'error');
    } finally {
      setActionLoading(null);
    }
  };

  // Save service config
  const handleSaveConfig = async () => {
    setConfigSaving(true);
    try {
      const updated = await updateProject(projectId, {
        auto_restart: autoRestart,
        max_concurrent_agents: maxAgents,
      });
      setProject(updated);
      showSnack('服务配置已保存', 'success');
    } catch (err: any) {
      showSnack(err?.message || '保存失败', 'error');
    } finally {
      setConfigSaving(false);
    }
  };

  const configChanged =
    project !== null &&
    (autoRestart !== project.auto_restart || maxAgents !== project.max_concurrent_agents);

  // Save agent config
  const handleAgentConfigSave = async () => {
    setAgentConfigSaving(true);
    try {
      const updated = await updateProject(projectId, {
        hooks_after_create: hooksAfterCreate || undefined,
        hooks_before_remove: hooksBeforeRemove || undefined,
        codex_command: codexCommand || undefined,
        codex_approval_policy: codexApprovalPolicy || undefined,
        codex_sandbox: codexSandbox || undefined,
      });
      setProject(updated);
      showSnack('Agent 配置已保存', 'success');
    } catch (err: any) {
      showSnack(err?.message || '保存失败', 'error');
    } finally {
      setAgentConfigSaving(false);
    }
  };

  const agentConfigChanged =
    project !== null &&
    (hooksAfterCreate !== (project.hooks_after_create || '') ||
      hooksBeforeRemove !== (project.hooks_before_remove || '') ||
      codexCommand !== (project.codex_command || '') ||
      codexApprovalPolicy !== (project.codex_approval_policy || 'never') ||
      codexSandbox !== (project.codex_sandbox || 'workspace-write'));

  // Testing agent config
  const handleTestingConfigSave = async () => {
    setTestingConfigSaving(true);
    try {
      const updated = await updateProject(projectId, {
        testing_enabled: testingEnabled,
        testing_max_attempts: testingMaxAttempts,
        testing_max_turns: testingMaxTurns,
        testing_skip_labels: testingSkipLabels || undefined,
        testing_allowed_commands: testingAllowedCommands || undefined,
      });
      setProject(updated);
      showSnack('测试 Agent 配置已保存', 'success');
    } catch (err: any) {
      showSnack(err?.message || '保存失败', 'error');
    } finally {
      setTestingConfigSaving(false);
    }
  };

  const testingConfigChanged =
    project !== null &&
    (testingEnabled !== project.testing_enabled ||
      testingMaxAttempts !== project.testing_max_attempts ||
      testingMaxTurns !== project.testing_max_turns ||
      testingSkipLabels !== (project.testing_skip_labels || '') ||
      testingAllowedCommands !== (project.testing_allowed_commands || ''));

  // Delete project
  const handleDelete = async () => {
    setDeleteLoading(true);
    try {
      await deleteProject(projectId);
      showSnack('项目已删除', 'success');
      navigate('/projects', { replace: true });
    } catch (err: any) {
      showSnack(err?.message || '删除失败', 'error');
      setDeleteOpen(false);
    } finally {
      setDeleteLoading(false);
    }
  };

  const basicChanged =
    project !== null &&
    (name.trim() !== project.name ||
      (description.trim() || '') !== (project.description || '') ||
      defaultBranch.trim() !== project.default_branch);

  if (loading) {
    return (
      <Box>
        <Skeleton variant="text" width={200} height={40} sx={{ mb: 3 }} />
        <Skeleton variant="rounded" height={400} sx={{ borderRadius: '8px' }} />
      </Box>
    );
  }

  if (!project) {
    return (
      <Box sx={{ textAlign: 'center', py: 6 }}>
        <Typography variant="h6" color="text.secondary">
          项目不存在或无权访问
        </Typography>
      </Box>
    );
  }

  return (
    <Box>
      {/* Page Title */}
      <Typography variant="h5" color="text.primary" sx={{ mb: 3 }}>
        项目设置 - {project.name}
      </Typography>

      {/* Tabs */}
      <Card>
        <CardContent sx={{ p: 3 }}>
          <Tabs
            value={tab}
            onChange={(_, v) => setTab(v)}
            aria-label="项目设置标签页"
            sx={{
              borderBottom: '1px solid #c3c6d7',
              '& .MuiTab-root': { textTransform: 'none', fontWeight: 500 },
            }}
          >
            <Tab label="基本信息" />
            <Tab label="工作流" />
            <Tab label="服务控制" />
            <Tab label="Agent 配置" />
            <Tab label="危险操作" />
          </Tabs>

          {/* Tab 1: Basic Info */}
          <TabPanel value={tab} index={0}>
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2.5, maxWidth: 500 }}>
              <TextField
                label="项目名称"
                value={name}
                onChange={(e) => setName(e.target.value)}
                fullWidth
              />
              <TextField
                label="项目描述"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                fullWidth
                multiline
                rows={3}
              />
              <TextField
                label="默认分支"
                value={defaultBranch}
                onChange={(e) => setDefaultBranch(e.target.value)}
                fullWidth
              />
              <Box>
                <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
                  Git URL
                </Typography>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                  <Chip
                    label={project.platform}
                    size="small"
                    color="primary"
                    variant="outlined"
                  />
                  <Typography variant="subtitle2" sx={{ fontFamily: 'monospace', wordBreak: 'break-all' }}>
                    {project.git_url}
                  </Typography>
                </Box>
              </Box>
            </Box>
            <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 3 }}>
              <Button
                variant="contained"
                onClick={handleBasicSave}
                disabled={!basicChanged || basicSaving}
                startIcon={basicSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
              >
                保存
              </Button>
            </Box>
          </TabPanel>

          {/* Tab 2: Workflow */}
          <TabPanel value={tab} index={1}>
            {workflowLoading ? (
              <Box sx={{ display: 'flex', justifyContent: 'center', py: 4 }}>
                <CircularProgress size={32} />
              </Box>
            ) : workflow ? (
              <WorkflowEditor
                templateMode={workflow.template_mode}
                content={workflow.content}
                updatedAt={workflow.updated_at}
                saving={workflowSaving}
                onSave={handleWorkflowSave}
                onReset={() => setResetConfirmOpen(true)}
                resetting={workflowResetting}
              />
            ) : null}
          </TabPanel>

          {/* Tab 3: Service Control */}
          <TabPanel value={tab} index={2}>
            <ServiceControlPanel
              status={serviceStatus}
              loading={serviceLoading}
              actionLoading={actionLoading}
              autoRestart={autoRestart}
              maxConcurrentAgents={maxAgents}
              onStart={() => handleServiceAction('start')}
              onStop={() => handleServiceAction('stop')}
              onRestart={() => handleServiceAction('restart')}
              onAutoRestartChange={setAutoRestart}
              onMaxAgentsChange={setMaxAgents}
              onSaveConfig={handleSaveConfig}
              configSaving={configSaving}
              configChanged={configChanged}
            />
          </TabPanel>

          {/* Tab 4: Agent Config */}
          <TabPanel value={tab} index={3}>
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3, maxWidth: 600 }}>
              <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
                Hooks 配置
              </Typography>
              <TextField
                label="after_create"
                helperText="Agent workspace 创建后执行的脚本（如 git clone、依赖安装）"
                value={hooksAfterCreate}
                onChange={(e) => setHooksAfterCreate(e.target.value)}
                fullWidth
                multiline
                rows={4}
                sx={{ '& .MuiInputBase-root': { fontFamily: 'monospace', fontSize: '13px' } }}
              />
              <TextField
                label="before_remove"
                helperText="Agent workspace 移除前执行的清理脚本"
                value={hooksBeforeRemove}
                onChange={(e) => setHooksBeforeRemove(e.target.value)}
                fullWidth
                multiline
                rows={3}
                sx={{ '& .MuiInputBase-root': { fontFamily: 'monospace', fontSize: '13px' } }}
              />

              <Typography variant="subtitle1" sx={{ fontWeight: 600, mt: 2 }}>
                Codex 配置
              </Typography>
              <TextField
                label="command"
                helperText="Codex 启动命令（如 codex --config ... app-server）"
                value={codexCommand}
                onChange={(e) => setCodexCommand(e.target.value)}
                fullWidth
                sx={{ '& .MuiInputBase-root': { fontFamily: 'monospace', fontSize: '13px' } }}
              />
              <TextField
                label="approval_policy"
                helperText="审批策略：never / auto-edit / manual"
                value={codexApprovalPolicy}
                onChange={(e) => setCodexApprovalPolicy(e.target.value)}
                fullWidth
              />
              <TextField
                label="thread_sandbox"
                helperText="沙箱模式：workspace-write / full-access"
                value={codexSandbox}
                onChange={(e) => setCodexSandbox(e.target.value)}
                fullWidth
              />
            </Box>
            <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 3 }}>
              <Button
                variant="contained"
                onClick={handleAgentConfigSave}
                disabled={!agentConfigChanged || agentConfigSaving}
                startIcon={agentConfigSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
              >
                保存
              </Button>
            </Box>

            {/* Testing Agent Config */}
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3, maxWidth: 600, mt: 5 }}>
              <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
                测试 Agent
              </Typography>
              <FormControlLabel
                control={
                  <Switch
                    checked={testingEnabled}
                    onChange={(e) => setTestingEnabled(e.target.checked)}
                  />
                }
                label="启用测试 Agent"
              />
              <TextField
                label="最大打回次数"
                type="number"
                helperText="测试失败后最多打回几次（1-5）"
                value={testingMaxAttempts}
                onChange={(e) => {
                  const v = parseInt(e.target.value, 10);
                  if (!isNaN(v)) setTestingMaxAttempts(Math.min(5, Math.max(1, v)));
                }}
                fullWidth
                inputProps={{ min: 1, max: 5 }}
                disabled={!testingEnabled}
              />
              <TextField
                label="最大 Turns"
                type="number"
                helperText="测试 Agent 单次执行的最大对话轮数（5-30）"
                value={testingMaxTurns}
                onChange={(e) => {
                  const v = parseInt(e.target.value, 10);
                  if (!isNaN(v)) setTestingMaxTurns(Math.min(30, Math.max(5, v)));
                }}
                fullWidth
                inputProps={{ min: 5, max: 30 }}
                disabled={!testingEnabled}
              />
              <TextField
                label="跳过测试标签"
                helperText="带有这些标签的 Issue 跳过测试，直接进入 Human Review（逗号分隔）"
                value={testingSkipLabels}
                onChange={(e) => setTestingSkipLabels(e.target.value)}
                fullWidth
                disabled={!testingEnabled}
              />
              <TextField
                label="额外允许命令"
                helperText="测试 Agent 额外允许执行的命令（逗号分隔，如 make test, pytest）"
                value={testingAllowedCommands}
                onChange={(e) => setTestingAllowedCommands(e.target.value)}
                fullWidth
                disabled={!testingEnabled}
              />
            </Box>
            <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 3 }}>
              <Button
                variant="contained"
                onClick={handleTestingConfigSave}
                disabled={!testingConfigChanged || testingConfigSaving}
                startIcon={testingConfigSaving ? <CircularProgress size={16} color="inherit" /> : undefined}
              >
                保存
              </Button>
            </Box>
          </TabPanel>

          {/* Tab 5: Danger Zone */}
          <TabPanel value={tab} index={4}>
            <Box
              sx={{
                p: 3,
                border: '1px solid',
                borderColor: 'error.light',
                borderRadius: '8px',
              }}
            >
              <Typography variant="subtitle1" color="error.main" sx={{ mb: 1, fontWeight: 600 }}>
                删除项目
              </Typography>
              <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
                删除项目将永久移除所有关联数据（成员关系、配置等），此操作不可撤销。
                服务必须处于停止状态才能删除。
              </Typography>
              <Button
                variant="contained"
                color="error"
                onClick={() => setDeleteOpen(true)}
              >
                删除此项目
              </Button>
            </Box>
          </TabPanel>
        </CardContent>
      </Card>

      {/* Reset Workflow Confirm */}
      <ConfirmDialog
        open={resetConfirmOpen}
        title="重置工作流"
        message="确定要将 WORKFLOW.md 重置为默认模板吗？自定义内容将被清除，此操作不可撤销。"
        confirmText="确认重置"
        confirmColor="error"
        icon={<WarningAmber sx={{ fontSize: 48, color: 'warning.main' }} />}
        onConfirm={handleWorkflowReset}
        onCancel={() => setResetConfirmOpen(false)}
      />

      {/* Delete Project Confirm */}
      <ConfirmDialog
        open={deleteOpen}
        title="删除项目"
        message={
          <>
            确定要删除项目 "{project.name}" 吗？
            <br />
            <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>
              此操作不可撤销，所有关联数据将被永久删除。
            </Typography>
          </>
        }
        confirmText="确认删除"
        confirmColor="error"
        icon={<WarningAmber sx={{ fontSize: 48, color: 'warning.main' }} />}
        loading={deleteLoading}
        onConfirm={handleDelete}
        onCancel={() => setDeleteOpen(false)}
      />

      {/* Snackbar */}
      <Snackbar
        open={snack.open}
        autoHideDuration={snack.severity === 'error' ? 6000 : 4000}
        onClose={() => setSnack((s) => ({ ...s, open: false }))}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      >
        <Alert
          severity={snack.severity}
          onClose={() => setSnack((s) => ({ ...s, open: false }))}
          variant="filled"
          sx={{ borderRadius: '4px' }}
        >
          {snack.message}
        </Alert>
      </Snackbar>
    </Box>
  );
}
