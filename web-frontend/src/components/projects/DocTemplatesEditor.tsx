import { useEffect, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Link,
  Paper,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material';
import { getDocTemplates, saveDocTemplate, trialDocTemplate } from '../../api/changes';
import MarkdownPreview from '../workbench/MarkdownPreview';
import type { DocType, ProjectDocTemplate, SavedDocTemplate, TemplateTrialResult } from '../../types/changes';

const DOCS: DocType[] = ['intent', 'spec', 'plan'];

/**
 * Project document templates (P2 §5 A7): effective template of the default
 * branch, saved through an MR, with a trial draft that is not committed.
 */
export default function DocTemplatesEditor({ projectId }: { projectId: number }) {
  const [templates, setTemplates] = useState<ProjectDocTemplate[] | null>(null);
  const [loadError, setLoadError] = useState('');
  const [doc, setDoc] = useState<DocType>('intent');
  const [drafts, setDrafts] = useState<Partial<Record<DocType, string>>>({});
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<SavedDocTemplate | null>(null);
  const [error, setError] = useState('');
  const [sample, setSample] = useState('');
  const [trialing, setTrialing] = useState(false);
  const [trial, setTrial] = useState<TemplateTrialResult | null>(null);

  useEffect(() => {
    let alive = true;
    getDocTemplates(projectId)
      .then((t) => alive && setTemplates(t))
      .catch((e: Error) => alive && setLoadError(e.message));
    return () => {
      alive = false;
    };
  }, [projectId]);

  if (loadError) return <Alert severity="error">{loadError}</Alert>;
  if (!templates) return <CircularProgress size={24} />;

  const current = templates.find((t) => t.docType === doc);
  const content = drafts[doc] ?? current?.content ?? '';
  const changed = current !== undefined && content !== current.content;

  const switchDoc = (d: DocType) => {
    setDoc(d);
    setSaved(null);
    setError('');
    setTrial(null);
  };

  const save = async () => {
    setSaving(true);
    setError('');
    setSaved(null);
    try {
      setSaved(await saveDocTemplate(projectId, doc, content));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  const runTrial = async () => {
    setTrialing(true);
    setError('');
    setTrial(null);
    try {
      setTrial(await trialDocTemplate(projectId, doc, content, sample));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setTrialing(false);
    }
  };

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Typography variant="body2" color="text.secondary">
        模板保存在仓库的 <code>docs/changes/_templates/&lt;doc&gt;.md</code>。front matter 里的
        <code> symphony_template.prompt </code>和<code> checklist </code>覆盖起草提示和审阅清单，其余内容是文档骨架。
        保存时会新建分支并创建 MR，合并后才生效。
      </Typography>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
        <ToggleButtonGroup size="small" exclusive value={doc} onChange={(_, v: DocType | null) => v && switchDoc(v)}>
          {DOCS.map((d) => (
            <ToggleButton key={d} value={d} sx={{ textTransform: 'none', px: 2 }}>
              {d}
            </ToggleButton>
          ))}
        </ToggleButtonGroup>
        <Chip
          size="small"
          data-testid="template-source"
          color={current?.source === 'project' ? 'primary' : 'default'}
          variant="outlined"
          label={current?.source === 'project' ? `项目模板 · ${current.path}` : '内置模板'}
        />
      </Box>

      <TextField
        multiline
        minRows={16}
        maxRows={32}
        fullWidth
        value={content}
        onChange={(e) => setDrafts({ ...drafts, [doc]: e.target.value })}
        inputProps={{ 'aria-label': '模板内容', spellCheck: false }}
        sx={{ '& .MuiInputBase-root': { fontFamily: 'monospace', fontSize: '13px' } }}
      />

      {error && <Alert severity="error">{error}</Alert>}
      {saved && (
        <Alert severity="success">
          已创建 MR{' '}
          <Link href={saved.mrUrl} target="_blank" rel="noreferrer">
            !{saved.mrIid}
          </Link>
          （分支 {saved.branch}），合并后生效。
        </Alert>
      )}

      <Box sx={{ display: 'flex', justifyContent: 'flex-end' }}>
        <Button
          variant="contained"
          onClick={() => void save()}
          disabled={!changed || saving}
          startIcon={saving ? <CircularProgress size={16} color="inherit" /> : undefined}
        >
          保存（创建 MR）
        </Button>
      </Box>

      <Paper variant="outlined" sx={{ p: 2, display: 'flex', flexDirection: 'column', gap: 1.5 }}>
        <Typography variant="subtitle2">试生成</Typography>
        <Typography variant="caption" color="text.secondary">
          用当前编辑中的模板，在临时工作区让 AI 起草一次。结果只在这里显示，不会提交；占用一个文档起草会话名额。
        </Typography>
        <TextField
          label="示例需求"
          multiline
          minRows={2}
          value={sample}
          onChange={(e) => setSample(e.target.value)}
          inputProps={{ maxLength: 20000 }}
        />
        <Box>
          <Button
            variant="outlined"
            onClick={() => void runTrial()}
            disabled={!sample.trim() || trialing}
            startIcon={trialing ? <CircularProgress size={16} /> : undefined}
          >
            试生成
          </Button>
        </Box>
        {trial && (
          <Box data-testid="trial-result">
            {trial.status !== 'completed' && (
              <Alert severity="warning" sx={{ mb: 1 }}>
                起草未完成（{trial.status}）{trial.error ? `：${trial.error}` : ''}
              </Alert>
            )}
            <MarkdownPreview content={trial.content} />
          </Box>
        )}
      </Paper>
    </Box>
  );
}
