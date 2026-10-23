import { useCallback, useEffect, useId, useRef, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  FormControl,
  FormControlLabel,
  FormLabel,
  IconButton,
  MenuItem,
  Radio,
  RadioGroup,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material';
import { Add, DeleteOutline } from '@mui/icons-material';
import GitUrlInput, { parseGitUrl } from './GitUrlInput';
import {
  getProjectRepoCandidates,
  getProjectRepos,
  updateProjectRepos,
} from '../../api/projectRepos';
import type {
  ProjectRepo,
  ProjectRepoCandidate,
  ProjectRepoCheckout,
  ProjectRepoInput,
  ProjectRepoMode,
} from '../../api/projectRepos';

export const PRIMARY_ALIAS = 'primary';
const ALIAS_PATTERN = /^[a-z0-9-]+$/;

type RepoSource = 'linked' | 'manual';

/** Editable draft of a single related repository row. */
export interface RepoDraft {
  key: string;
  alias: string;
  source: RepoSource;
  linkedProjectId: number | '';
  gitUrl: string;
  defaultBranch: string;
  mode: ProjectRepoMode;
  checkout: ProjectRepoCheckout;
  mergeAfter: string[];
  /** Values from the backend; only present for rows that were loaded. */
  original?: ProjectRepo;
}

export interface RepoDraftErrors {
  alias?: string;
  linkedProjectId?: string;
  gitUrl?: string;
  defaultBranch?: string;
  mergeAfter?: string;
}

interface ProjectReposEditorProps {
  projectId: number;
  /** Main project git url, used to rebuild urls of manual rows (namespace/repo only from backend). */
  primaryGitUrl: string;
  /** Whether the current user is the main project's created_by (only they can set 可写). */
  isCreator: boolean;
}

function toDraft(repo: ProjectRepo, key: string, host: string): RepoDraft {
  const manual = repo.linkedProjectId === null;
  return {
    key,
    alias: repo.alias,
    source: manual ? 'manual' : 'linked',
    linkedProjectId: repo.linkedProjectId ?? '',
    gitUrl:
      manual && repo.namespace && repo.repoName && host
        ? `https://${host}/${repo.namespace}/${repo.repoName}.git`
        : '',
    defaultBranch: repo.defaultBranch ?? '',
    mode: repo.mode,
    checkout: repo.checkout,
    mergeAfter: repo.mergeAfter ?? [],
    original: repo,
  };
}

/** Client-side validation. Returns errors keyed by draft key; empty object means valid. */
export function validateRepoDrafts(drafts: RepoDraft[]): Record<string, RepoDraftErrors> {
  const result: Record<string, RepoDraftErrors> = {};
  const aliasCount = new Map<string, number>();
  drafts.forEach((d) => {
    const a = d.alias.trim();
    aliasCount.set(a, (aliasCount.get(a) ?? 0) + 1);
  });
  const knownAliases = new Set([PRIMARY_ALIAS, ...drafts.map((d) => d.alias.trim())]);

  for (const d of drafts) {
    const errors: RepoDraftErrors = {};
    const alias = d.alias.trim();
    if (!alias) {
      errors.alias = '请填写别名';
    } else if (!ALIAS_PATTERN.test(alias)) {
      errors.alias = '别名只能包含小写字母、数字和 -';
    } else if (alias === PRIMARY_ALIAS) {
      errors.alias = '别名 primary 已被主仓库占用';
    } else if ((aliasCount.get(alias) ?? 0) > 1) {
      errors.alias = '别名不能重复';
    }

    if (d.source === 'linked') {
      if (d.linkedProjectId === '') errors.linkedProjectId = '请选择 Symphony 项目';
    } else {
      if (!d.gitUrl.trim()) {
        errors.gitUrl = '请填写仓库地址';
      } else if (!parseGitUrl(d.gitUrl.trim()).isValid) {
        errors.gitUrl = '仓库地址格式不正确';
      }
      if (!d.defaultBranch.trim()) errors.defaultBranch = '手动填写仓库时必须指定默认分支';
    }

    const unknown = d.mergeAfter.filter((m) => m === alias || !knownAliases.has(m));
    if (unknown.length > 0) errors.mergeAfter = `合并依赖无效：${unknown.join(', ')}`;

    if (Object.keys(errors).length > 0) result[d.key] = errors;
  }
  return result;
}

export function draftToInput(d: RepoDraft): ProjectRepoInput {
  const branch = d.defaultBranch.trim();
  const base = {
    alias: d.alias.trim(),
    mode: d.mode,
    checkout: d.checkout,
    mergeAfter: d.mergeAfter,
  };
  if (d.source === 'linked') {
    return {
      ...base,
      linkedProjectId: d.linkedProjectId === '' ? undefined : d.linkedProjectId,
      defaultBranch: branch || undefined,
    };
  }
  return { ...base, gitUrl: d.gitUrl.trim(), defaultBranch: branch };
}

interface RepoRowProps {
  index: number;
  draft: RepoDraft;
  errors?: RepoDraftErrors;
  candidates: ProjectRepoCandidate[];
  otherAliases: string[];
  isCreator: boolean;
  disabled: boolean;
  onChange: (patch: Partial<RepoDraft>) => void;
  onRemove: () => void;
}

function RepoRow({
  index,
  draft,
  errors,
  candidates,
  otherAliases,
  isCreator,
  disabled,
  onChange,
  onRemove,
}: RepoRowProps) {
  const uid = useId();
  const sourceLabelId = `${uid}-source-label`;
  const statusId = `${uid}-status`;
  const invalid = draft.original?.status === 'invalid';
  const rowName = draft.alias.trim() || `#${index + 1}`;

  // Placeholder showing inherited branch when the input is empty.
  let branchPlaceholder = '';
  if (draft.source === 'linked') {
    const sameLink =
      draft.original && draft.original.linkedProjectId === draft.linkedProjectId;
    branchPlaceholder = sameLink && draft.original?.resolvedDefaultBranch
      ? `继承：${draft.original.resolvedDefaultBranch}`
      : '继承被引用项目的默认分支';
  } else if (draft.original?.resolvedDefaultBranch) {
    branchPlaceholder = draft.original.resolvedDefaultBranch;
  }

  const mergeOptions = [PRIMARY_ALIAS, ...otherAliases];
  // Keep stale selections visible so they can be removed.
  draft.mergeAfter.forEach((m) => {
    if (!mergeOptions.includes(m)) mergeOptions.push(m);
  });

  return (
    <Box
      role="group"
      aria-label={`关联仓库 ${rowName}`}
      aria-describedby={invalid ? statusId : undefined}
      data-testid="project-repo-row"
      data-invalid={invalid ? 'true' : 'false'}
      sx={{
        p: 2.5,
        border: '1px solid',
        borderColor: invalid ? 'error.main' : '#c3c6d7',
        bgcolor: invalid ? '#fff5f5' : 'transparent',
        borderRadius: '8px',
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
      }}
    >
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600, flex: 1 }}>
          {rowName}
          {invalid && (
            <Chip label="已失效" size="small" color="error" sx={{ ml: 1 }} />
          )}
        </Typography>
        <Tooltip title="删除">
          <span>
            <IconButton
              aria-label={`删除关联仓库 ${rowName}`}
              onClick={onRemove}
              disabled={disabled}
              size="small"
            >
              <DeleteOutline />
            </IconButton>
          </span>
        </Tooltip>
      </Box>

      {invalid && (
        <Alert severity="error" id={statusId}>
          {draft.original?.statusReason || '该关联已失效'}
        </Alert>
      )}

      <TextField
        label="别名"
        value={draft.alias}
        onChange={(e) => onChange({ alias: e.target.value })}
        error={!!errors?.alias}
        helperText={errors?.alias || '小写字母、数字和 -，作为工作区内的目录名'}
        disabled={disabled}
        required
        fullWidth
      />

      <FormControl component="fieldset" disabled={disabled}>
        <FormLabel id={sourceLabelId} component="legend">
          来源
        </FormLabel>
        <RadioGroup
          row
          aria-labelledby={sourceLabelId}
          value={draft.source}
          onChange={(e) => onChange({ source: e.target.value as RepoSource })}
        >
          <FormControlLabel value="linked" control={<Radio />} label="Symphony 项目" />
          <FormControlLabel value="manual" control={<Radio />} label="手动填写仓库" />
        </RadioGroup>
      </FormControl>

      {draft.source === 'linked' ? (
        <TextField
          select
          label="Symphony 项目"
          value={draft.linkedProjectId}
          onChange={(e) =>
            onChange({ linkedProjectId: e.target.value === '' ? '' : Number(e.target.value) })
          }
          error={!!errors?.linkedProjectId}
          helperText={errors?.linkedProjectId || '只能选择你是 owner 的项目'}
          disabled={disabled}
          required
          fullWidth
        >
          {candidates.length === 0 && (
            <MenuItem value="" disabled>
              暂无可关联的项目
            </MenuItem>
          )}
          {candidates.map((c) => (
            <MenuItem key={c.id} value={c.id}>
              {c.name}
            </MenuItem>
          ))}
        </TextField>
      ) : (
        <GitUrlInput
          value={draft.gitUrl}
          onChange={(v) => onChange({ gitUrl: v })}
          error={errors?.gitUrl}
          disabled={disabled}
        />
      )}

      <TextField
        label="默认分支"
        value={draft.defaultBranch}
        onChange={(e) => onChange({ defaultBranch: e.target.value })}
        placeholder={branchPlaceholder}
        slotProps={{ inputLabel: { shrink: true } }}
        error={!!errors?.defaultBranch}
        helperText={
          errors?.defaultBranch ||
          (draft.source === 'linked' ? '留空则继承被引用项目的默认分支' : '手动填写仓库时必填')
        }
        required={draft.source === 'manual'}
        disabled={disabled}
        fullWidth
      />

      <Box sx={{ display: 'flex', gap: 2, flexWrap: 'wrap' }}>
        <TextField
          select
          label="读写模式"
          value={draft.mode}
          onChange={(e) => onChange({ mode: e.target.value as ProjectRepoMode })}
          helperText={
            isCreator ? '可写仅项目创建者可设置' : '可写仅项目创建者可设置，你当前只能选择只读'
          }
          disabled={disabled}
          sx={{ flex: 1, minWidth: 200 }}
        >
          <MenuItem value="read">只读</MenuItem>
          <MenuItem value="write" disabled={!isCreator}>
            可写
          </MenuItem>
        </TextField>
        <TextField
          select
          label="检出方式"
          value={draft.checkout}
          onChange={(e) => onChange({ checkout: e.target.value as ProjectRepoCheckout })}
          helperText="partial 为部分检出（默认），full 为完整克隆"
          disabled={disabled}
          sx={{ flex: 1, minWidth: 200 }}
        >
          <MenuItem value="partial">partial（默认）</MenuItem>
          <MenuItem value="full">full</MenuItem>
        </TextField>
      </Box>

      <TextField
        select
        label="合并依赖"
        value={draft.mergeAfter}
        onChange={(e) => {
          const v = e.target.value as unknown;
          onChange({
            mergeAfter: typeof v === 'string' ? v.split(',').filter(Boolean) : (v as string[]),
          });
        }}
        slotProps={{
          select: {
            multiple: true,
            renderValue: (selected) => (selected as string[]).join(', '),
          },
        }}
        error={!!errors?.mergeAfter}
        helperText={errors?.mergeAfter || '这些仓库的 MR 合并之后再合并本仓库'}
        disabled={disabled}
        fullWidth
      >
        {mergeOptions.map((a) => (
          <MenuItem key={a} value={a}>
            {a}
          </MenuItem>
        ))}
      </TextField>
    </Box>
  );
}

export default function ProjectReposEditor({
  projectId,
  primaryGitUrl,
  isCreator,
}: ProjectReposEditorProps) {
  const keySeq = useRef(0);
  const nextKey = () => `repo-${keySeq.current++}`;
  const host = parseGitUrl(primaryGitUrl).host;

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [drafts, setDrafts] = useState<RepoDraft[]>([]);
  const [candidates, setCandidates] = useState<ProjectRepoCandidate[]>([]);
  const [errors, setErrors] = useState<Record<string, RepoDraftErrors>>({});
  const [submitted, setSubmitted] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveSuccess, setSaveSuccess] = useState(false);

  const load = useCallback(async () => {
    const [repos, cands] = await Promise.all([
      getProjectRepos(projectId),
      getProjectRepoCandidates(projectId).catch(() => [] as ProjectRepoCandidate[]),
    ]);
    setDrafts(repos.map((r) => toDraft(r, `repo-${keySeq.current++}`, host)));
    setCandidates(cands);
  }, [projectId, host]);

  useEffect(() => {
    setLoading(true);
    load()
      .catch((err: Error) => setSaveError(err?.message || '加载关联仓库失败'))
      .finally(() => setLoading(false));
  }, [load]);

  // Re-validate live once the user has attempted to save.
  useEffect(() => {
    if (submitted) setErrors(validateRepoDrafts(drafts));
  }, [drafts, submitted]);

  // Linked projects already in use are excluded from candidates by the backend; keep them selectable.
  const candidateOptions: ProjectRepoCandidate[] = [...candidates];
  drafts.forEach((d) => {
    const o = d.original;
    if (o?.linkedProjectId != null && !candidateOptions.some((c) => c.id === o.linkedProjectId)) {
      candidateOptions.push({
        id: o.linkedProjectId,
        name: o.linkedProjectName ?? `项目 #${o.linkedProjectId}`,
      });
    }
  });

  const updateDraft = (key: string, patch: Partial<RepoDraft>) => {
    setSaveSuccess(false);
    setDrafts((ds) => ds.map((d) => (d.key === key ? { ...d, ...patch } : d)));
  };

  const addRow = () => {
    setSaveSuccess(false);
    setDrafts((ds) => [
      ...ds,
      {
        key: nextKey(),
        alias: '',
        source: 'linked',
        linkedProjectId: '',
        gitUrl: '',
        defaultBranch: '',
        mode: 'read',
        checkout: 'partial',
        mergeAfter: [],
      },
    ]);
  };

  const removeRow = (key: string) => {
    setSaveSuccess(false);
    setDrafts((ds) => ds.filter((d) => d.key !== key));
  };

  const handleSave = async () => {
    setSubmitted(true);
    setSaveError(null);
    setSaveSuccess(false);
    const errs = validateRepoDrafts(drafts);
    setErrors(errs);
    if (Object.keys(errs).length > 0) return;

    setSaving(true);
    try {
      await updateProjectRepos(projectId, drafts.map(draftToInput));
      await load();
      setSubmitted(false);
      setErrors({});
      setSaveSuccess(true);
    } catch (err) {
      setSaveError((err as Error)?.message || '保存失败');
    } finally {
      setSaving(false);
    }
  };

  const hasErrors = Object.keys(errors).length > 0;

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, maxWidth: 720 }}>
      <Box>
        <Typography variant="subtitle1" component="h2" sx={{ fontWeight: 600 }}>
          关联仓库
        </Typography>
        <Typography variant="body2" color="text.secondary">
          关联仓库会被检出到 Issue 工作区供 agent 参考；默认只读。
        </Typography>
      </Box>

      {loading ? (
        <Box sx={{ display: 'flex', justifyContent: 'center', py: 4 }}>
          <CircularProgress size={32} aria-label="加载关联仓库" />
        </Box>
      ) : (
        <>
          {saveError && (
            <Alert severity="error" role="alert">
              {saveError}
            </Alert>
          )}
          {hasErrors && (
            <Alert severity="error" role="alert">
              请修正标红的字段后再保存
            </Alert>
          )}
          {saveSuccess && <Alert severity="success">关联仓库已保存</Alert>}

          {drafts.length === 0 && (
            <Typography variant="body2" color="text.secondary">
              暂无关联仓库
            </Typography>
          )}

          {drafts.map((d, i) => (
            <RepoRow
              key={d.key}
              index={i}
              draft={d}
              errors={errors[d.key]}
              candidates={candidateOptions}
              otherAliases={drafts
                .filter((o) => o.key !== d.key)
                .map((o) => o.alias.trim())
                .filter((a) => a && a !== PRIMARY_ALIAS)}
              isCreator={isCreator}
              disabled={saving}
              onChange={(patch) => updateDraft(d.key, patch)}
              onRemove={() => removeRow(d.key)}
            />
          ))}

          <Box sx={{ display: 'flex', justifyContent: 'space-between', mt: 1 }}>
            <Button variant="outlined" startIcon={<Add />} onClick={addRow} disabled={saving}>
              添加关联仓库
            </Button>
            <Button
              variant="contained"
              onClick={handleSave}
              disabled={saving}
              startIcon={saving ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              保存关联仓库
            </Button>
          </Box>
        </>
      )}
    </Box>
  );
}
