import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  CircularProgress,
  TextField,
  Typography,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome';
import { startFlow } from '../../api/changes';
import { useIssueStore } from '../../store/issueStore';

/** Turn a free-form AI issue draft into the intent skeleton sections. */
export function toIntentBody(title: string, idea: string, generated: string): string {
  const problem = generated.trim() || idea.trim();
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

/** "新建需求": quick description → intent.md draft → workbench (§2 入口). */
export default function NewChangePage() {
  const { id } = useParams<{ id: string }>();
  const projectId = Number(id);
  const navigate = useNavigate();
  const { aiStatus, generatedContent, generatedTitle, aiError, startGenerate, stopGenerate, resetAI } = useIssueStore();
  const [title, setTitle] = useState('');
  const [idea, setIdea] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (aiStatus === 'done' && generatedTitle && !title) setTitle(generatedTitle);
  }, [aiStatus, generatedTitle, title]);

  useEffect(() => () => resetAI(), [resetAI]);

  const create = async () => {
    setBusy(true);
    setError('');
    try {
      const v = await startFlow(projectId, {
        track: 'full',
        title: title.trim(),
        summary: idea.trim() || undefined,
        initialContent: toIntentBody(title.trim(), idea, aiStatus === 'done' ? generatedContent : ''),
      });
      navigate(`/projects/${projectId}/changes/${v.flow.issueIid}`, { replace: true });
    } catch (e) {
      setError((e as Error).message);
    } finally {
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
            用一句话描述想法，AI 会按 intent 模板生成草稿；创建后进入文档工作台继续完善（路径 track:full）。
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
                onClick={() => startGenerate(projectId, { prompt: idea.trim() })}
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
          <TextField label="标题" value={title} onChange={(e) => setTitle(e.target.value)} required inputProps={{ maxLength: 200 }} />
          {error && <Alert severity="error">{error}</Alert>}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end' }}>
            <Button
              variant="contained"
              disabled={busy || !title.trim() || aiStatus === 'generating'}
              onClick={create}
              startIcon={busy ? <CircularProgress size={16} color="inherit" /> : undefined}
            >
              创建并进入工作台
            </Button>
          </Box>
        </CardContent>
      </Card>
    </Box>
  );
}
