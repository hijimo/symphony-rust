import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Chip,
  CircularProgress,
  TextField,
  Typography,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import AttachFileIcon from '@mui/icons-material/AttachFile';
import CloseIcon from '@mui/icons-material/Close';
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome';
import { startFlow, uploadFile } from '../../api/changes';
import { MAX_UPLOAD_BYTES, normalizeSubdir, uploadTarget } from '../../components/workbench/UploadDialog';
import { useIssueStore } from '../../store/issueStore';
import { aiGenerateIssue } from '../../api/aiGenerate';

const INTENT_HEADING = /^#\s*Intent\s*:.*$/m;

/**
 * Build the intent.md body. When the AI already returned a full intent
 * document (`# Intent: ...` heading), keep it and only sync the heading with
 * the final title; otherwise put the idea/draft under Problem.
 */
export function toIntentBody(title: string, idea: string, generated: string): string {
  const draft = generated.trim();
  if (INTENT_HEADING.test(draft)) {
    const doc = draft.slice(draft.search(INTENT_HEADING)).replace(INTENT_HEADING, `# Intent: ${title}`);
    return `${doc}\n`;
  }
  const problem = draft || idea.trim();
  return [
    `# Intent: ${title}`,
    '',
    '## Problem',
    problem,
    '',
    '## Proposed outcome',
    '<!-- 更好的状态是什么样 -->',
    '',
    '## Affected users and systems',
    '<!-- 受影响的用户和系统 -->',
    '',
    '## Constraints',
    '<!-- 约束：安全、合规、性能、兼容性等 -->',
    '',
    '## Open questions',
    '<!-- 还没想清楚的问题 -->',
    '',
  ].join('\n');
}

/** How long 创建 waits for an automatic AI draft before falling back. */
export const AUTO_DRAFT_TIMEOUT_MS = 30000;

/**
 * Generate an intent draft for `idea` (used by 创建 when no draft exists yet).
 * Resolves with the draft, or with `error` set when generation fails or
 * takes longer than `timeoutMs`; never rejects.
 */
export function generateIntentDraft(
  projectId: number,
  idea: string,
  timeoutMs = AUTO_DRAFT_TIMEOUT_MS,
): Promise<{ content: string; title: string; error: string | null }> {
  const controller = new AbortController();
  return new Promise((resolve) => {
    let settled = false;
    const finish = (r: { content: string; title: string; error: string | null }) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      controller.abort();
      resolve(r);
    };
    const timer = setTimeout(
      () => finish({ content: '', title: '', error: `AI 生成超过 ${Math.round(timeoutMs / 1000)} 秒未完成` }),
      timeoutMs,
    );
    aiGenerateIssue(
      projectId,
      { prompt: idea, template: 'intent' },
      {
        onChunk: () => {},
        onDone: (content, title) =>
          finish(content.trim() ? { content, title: title ?? '', error: null } : { content: '', title: '', error: 'AI 返回内容为空' }),
        onError: (error) => finish({ content: '', title: '', error }),
      },
      controller.signal,
    ).catch((e: unknown) => finish({ content: '', title: '', error: (e as Error)?.message || 'AI 服务不可用' }));
  });
}

/** "新建需求": quick description → intent.md draft → workbench (§2 入口). */
export default function NewChangePage() {
  const { id } = useParams<{ id: string }>();
  const projectId = Number(id);
  const navigate = useNavigate();
  const { aiStatus, generatedContent, generatedTitle, aiError, startGenerate, stopGenerate, resetAI } = useIssueStore();
  const [title, setTitle] = useState('');
  const [idea, setIdea] = useState('');
  const [busy, setBusy] = useState(false);
  const [drafting, setDrafting] = useState(false);
  const [error, setError] = useState('');
  const [files, setFiles] = useState<File[]>([]);
  const [subdir, setSubdir] = useState('assets');
  const dir = normalizeSubdir(subdir);
  const tooBig = files.filter((f) => f.size > MAX_UPLOAD_BYTES);
  const docClash = dir === '' && files.some((f) => ['intent.md', 'spec.md', 'plan.md'].includes(f.name));
  const filesOk = files.length === 0 || (dir !== null && tooBig.length === 0 && !docClash);

  useEffect(() => {
    if (aiStatus === 'done' && generatedTitle && !title) setTitle(generatedTitle);
  }, [aiStatus, generatedTitle, title]);

  useEffect(() => () => resetAI(), [resetAI]);

  const create = async () => {
    setBusy(true);
    setError('');
    try {
      // No finished draft yet: generate one now so intent.md follows the
      // template instead of only carrying the raw idea (fallback on failure).
      let draft = aiStatus === 'done' ? generatedContent : '';
      let draftError: string | null = null;
      if (!draft.trim() && idea.trim().length >= 5) {
        setDrafting(true);
        const r = await generateIntentDraft(projectId, idea.trim());
        setDrafting(false);
        draft = r.content;
        draftError = r.error;
      }
      const v = await startFlow(projectId, {
        track: 'full',
        title: title.trim(),
        summary: idea.trim() || undefined,
        initialContent: toIntentBody(title.trim(), idea, draft),
      });
      const iid = v.flow.issueIid;
      // Attachments need the flow's workspace, so they go in after creation.
      const failed: string[] = [];
      for (const f of files) {
        try {
          await uploadFile(projectId, iid, uploadTarget(v.writableDir, dir ?? '', f.name), f);
        } catch (e) {
          failed.push(`${f.name}：${(e as Error).message}`);
        }
      }
      const state: { uploadErrors?: string[]; draftError?: string } = {};
      if (failed.length) state.uploadErrors = failed;
      if (draftError) state.draftError = draftError;
      navigate(`/projects/${projectId}/changes/${iid}`, {
        replace: true,
        state: Object.keys(state).length ? state : undefined,
      });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setDrafting(false);
      setBusy(false);
    }
  };

  return (
    <Box>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, mb: 3 }}>
        <Button variant="text" startIcon={<ArrowBackIcon />} onClick={() => navigate(-1)}>
          返回
        </Button>
        <Typography variant="h5">新建需求</Typography>
      </Box>
      <Card sx={{ maxWidth: 800, border: '1px solid #c3c6d7' }}>
        <CardContent sx={{ p: 3, display: 'flex', flexDirection: 'column', gap: 2 }}>
          <Typography variant="body2" color="text.secondary">
            用一句话描述想法，AI 会按 intent 模板生成草稿（没有先点「AI 生成草稿」时，创建时会自动生成，最多等 30 秒）；创建后进入文档工作台继续完善（路径 track:full）。
          </Typography>
          <TextField
            label="想法"
            multiline
            minRows={3}
            value={idea}
            onChange={(e) => setIdea(e.target.value)}
            placeholder="例如：客户想在门户里看到理赔进度"
          />
          <Box sx={{ display: 'flex', gap: 1 }}>
            {aiStatus === 'generating' ? (
              <Button variant="outlined" onClick={stopGenerate}>停止生成</Button>
            ) : (
              <Button
                variant="outlined"
                startIcon={<AutoAwesomeIcon />}
                disabled={idea.trim().length < 5}
                onClick={() => startGenerate(projectId, { prompt: idea.trim(), template: 'intent' })}
              >
                AI 生成草稿
              </Button>
            )}
          </Box>
          {aiError && <Alert severity="warning">{aiError}（可以直接创建，草稿只包含你的描述）</Alert>}
          {generatedContent && (
            <Box component="pre" sx={{ bgcolor: '#f3f3fe', p: 1.5, borderRadius: 1, fontSize: 13, whiteSpace: 'pre-wrap', maxHeight: 260, overflow: 'auto' }}>
              {generatedContent}
            </Box>
          )}
          <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
              <Button variant="outlined" component="label" startIcon={<AttachFileIcon />} disabled={busy}>
                添加附件
                <input
                  hidden
                  multiple
                  type="file"
                  aria-label="添加附件"
                  onChange={(e) => {
                    const picked = Array.from(e.target.files ?? []);
                    setFiles((fs) => [...fs.filter((f) => !picked.some((p) => p.name === f.name)), ...picked]);
                    e.target.value = '';
                  }}
                />
              </Button>
              {files.map((f) => (
                <Chip
                  key={f.name}
                  size="small"
                  label={`${f.name}（${(f.size / 1024).toFixed(1)} KB）`}
                  onDelete={() => setFiles((fs) => fs.filter((x) => x !== f))}
                  deleteIcon={<CloseIcon aria-label={`移除 ${f.name}`} />}
                />
              ))}
            </Box>
            {files.length > 0 && (
              <TextField
                size="small"
                label="附件目录"
                value={subdir}
                onChange={(e) => setSubdir(e.target.value)}
                error={dir === null}
                helperText={
                  dir === null
                    ? '目录不能包含 ..、. 或以 . 开头的名称'
                    : `创建后保存到本需求文档目录下的 ${dir ? `${dir}/` : '根目录'}，可在工作台继续上传或调整`
                }
                inputProps={{ 'aria-label': '附件目录' }}
              />
            )}
            {tooBig.length > 0 && <Alert severity="error">单个文件不能超过 10MB：{tooBig.map((f) => f.name).join('、')}</Alert>}
            {docClash && <Alert severity="error">intent.md / spec.md / plan.md 不能作为附件</Alert>}
          </Box>
          <TextField label="标题" value={title} onChange={(e) => setTitle(e.target.value)} required inputProps={{ maxLength: 200 }} />
          {error && <Alert severity="error">{error}</Alert>}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end' }}>
            <Button
              variant="contained"
              disabled={busy || !title.trim() || aiStatus === 'generating' || !filesOk}
              onClick={create}
              startIcon={busy ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              {drafting ? 'AI 正在生成 intent 草稿…' : '创建并进入工作台'}
            </Button>
          </Box>
        </CardContent>
      </Card>
    </Box>
  );
}
