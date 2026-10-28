import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import {
  Alert,
  Box,
  Button,
  IconButton,
  Tab,
  Tabs,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material';
import CloseIcon from '@mui/icons-material/Close';
import LockIcon from '@mui/icons-material/Lock';
import AutoFixHighIcon from '@mui/icons-material/AutoFixHigh';
import type { OnMount } from '@monaco-editor/react';
import { useWorkbenchStore, type SelectionRef } from '../../store/workbenchStore';
import MarkdownPreview from './MarkdownPreview';

const monacoReady = () => import('./monacoSetup');
const Editor = lazy(() => monacoReady().then(() => import('@monaco-editor/react')).then((m) => ({ default: m.default })));
const DiffEditor = lazy(() =>
  monacoReady().then(() => import('@monaco-editor/react')).then((m) => ({ default: m.DiffEditor })),
);

type ViewMode = 'source' | 'split' | 'preview';

function languageOf(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    md: 'markdown', rs: 'rust', ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    json: 'json', yml: 'yaml', yaml: 'yaml', toml: 'ini', py: 'python', go: 'go', java: 'java', sql: 'sql',
    css: 'css', html: 'html', sh: 'shell',
  };
  return map[ext] ?? 'plaintext';
}

/** Fallback editor used when Monaco is unavailable (tests, slow networks). */
function PlainEditor({ value, readOnly, onChange }: { value: string; readOnly: boolean; onChange: (v: string) => void }) {
  return (
    <textarea
      aria-label="编辑器"
      value={value}
      readOnly={readOnly}
      onChange={(e) => onChange(e.target.value)}
      style={{ width: '100%', height: '100%', border: 0, resize: 'none', padding: 12, fontFamily: '"JetBrains Mono", monospace', fontSize: 13 }}
    />
  );
}

export const useMonaco = () => (import.meta.env.MODE !== 'test' && !(window as unknown as { __PLAIN_EDITOR__?: boolean }).__PLAIN_EDITOR__);

export default function EditorPane({ onAskAi }: { onAskAi: (sel: SelectionRef) => void }) {
  const { tabs, active, buffers, setActive, closeTab, edit, save, resolveConflict, stages, stage, revealLine: reveal } = useWorkbenchStore();
  const [mode, setMode] = useState<ViewMode>(() => (window.matchMedia?.('(max-width:900px)').matches ? 'source' : 'split'));
  const [diffOpen, setDiffOpen] = useState(false);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);
  const [selection, setSelection] = useState<SelectionRef | null>(null);
  const buf = active ? buffers[active] : null;
  const isMd = active?.endsWith('.md') ?? false;
  const locked = stages[stage].running && buf?.writable;
  const monaco = useMonaco();

  const onMount: OnMount = (ed) => {
    editorRef.current = ed;
    ed.onDidChangeCursorSelection(() => {
      const sel = ed.getSelection();
      const model = ed.getModel();
      if (!sel || !model || sel.isEmpty() || !active) {
        setSelection(null);
        return;
      }
      setSelection({ path: active, startLine: sel.startLineNumber, endLine: sel.endLineNumber, text: model.getValueInRange(sel) });
    });
  };

  const revealLine = (line: number) => {
    editorRef.current?.revealLineInCenter(line);
    editorRef.current?.setPosition({ lineNumber: line, column: 1 });
    editorRef.current?.focus();
  };

  useEffect(() => {
    if (reveal && reveal.path === active && editorRef.current) {
      revealLine(reveal.line);
      useWorkbenchStore.setState({ revealLine: null });
    }
  }, [reveal, active]);

  if (!buf || !active) {
    return (
      <Box sx={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#737686' }}>
        <Typography variant="body2">从左侧目录打开一个文件</Typography>
      </Box>
    );
  }
  const readOnly = !buf.writable || Boolean(locked) || buf.binary || buf.tooLarge;
  const lastDiff = stages[stage].lastDiff;
  const editor = monaco ? (
    <Suspense fallback={<Typography sx={{ p: 2 }}>编辑器加载中…</Typography>}>
      <Editor
        key={active}
        path={active}
        defaultValue={buf.content}
        value={buf.content}
        language={languageOf(active)}
        onChange={(v) => {
          // Guard (verify B B1): only accept edits from this path's own model.
          const modelPath = editorRef.current?.getModel()?.uri.path.replace(/^\//, '');
          if (modelPath !== undefined && modelPath !== active) return;
          if ((v ?? '') === useWorkbenchStore.getState().buffers[active]?.content) return;
          edit(active, v ?? '');
        }}
        onMount={onMount}
        options={{ readOnly, minimap: { enabled: false }, wordWrap: isMd ? 'on' : 'off', fontSize: 13 }}
      />
    </Suspense>
  ) : (
    <PlainEditor key={active} value={buf.content} readOnly={readOnly} onChange={(v) => edit(active, v)} />
  );

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', bgcolor: '#fff' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', borderBottom: '1px solid #e7e7f3' }}>
        <Tabs value={active} onChange={(_, v) => setActive(v)} variant="scrollable" sx={{ minHeight: 32, flex: 1 }}>
          {tabs.map((t) => (
            <Tab
              key={t}
              value={t}
              sx={{ minHeight: 32, py: 0, textTransform: 'none', fontSize: 12 }}
              label={
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
                  {!buffers[t]?.writable && <LockIcon sx={{ fontSize: 12 }} aria-label="只读" />}
                  <span>{t.split('/').pop()}</span>
                  {buffers[t]?.dirty && <span aria-label="未保存">●</span>}
                  <IconButton
                    component="span"
                    size="small"
                    aria-label={`关闭 ${t}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeTab(t);
                    }}
                    sx={{ p: 0.1 }}
                  >
                    <CloseIcon sx={{ fontSize: 12 }} />
                  </IconButton>
                </Box>
              }
            />
          ))}
        </Tabs>
        {isMd && (
          <ToggleButtonGroup size="small" exclusive value={mode} onChange={(_, v) => v && setMode(v)} sx={{ mr: 1 }}>
            <ToggleButton value="source" sx={{ py: 0.2, fontSize: 11 }}>源码</ToggleButton>
            <ToggleButton value="split" sx={{ py: 0.2, fontSize: 11 }}>分栏</ToggleButton>
            <ToggleButton value="preview" sx={{ py: 0.2, fontSize: 11 }}>预览</ToggleButton>
          </ToggleButtonGroup>
        )}
        {lastDiff && lastDiff.files.includes(active) && (
          <Button size="small" onClick={() => setDiffOpen(!diffOpen)} sx={{ mr: 1 }}>
            {diffOpen ? '关闭 diff' : '本轮 AI 改动'}
          </Button>
        )}
        {buf.writable && (
          <Button size="small" variant="contained" disabled={readOnly} onClick={() => save(active, true)} sx={{ mr: 1 }}>
            保存
          </Button>
        )}
      </Box>
      {locked && <Alert severity="info" sx={{ py: 0 }}>AI 正在修改文档，本轮结束后可以编辑</Alert>}
      {buf.tooLarge && <Alert severity="warning" sx={{ py: 0 }}>文件超过 1MB，只读显示元信息</Alert>}
      {buf.binary && <Alert severity="warning" sx={{ py: 0 }}>二进制文件不支持预览</Alert>}
      {buf.conflict && (
        <Alert
          severity="warning"
          sx={{ py: 0 }}
          action={
            <>
              <Button size="small" onClick={() => setDiffOpen(true)}>查看 diff</Button>
              <Button size="small" onClick={() => resolveConflict(active, 'mine')}>保留我的</Button>
              <Button size="small" onClick={() => resolveConflict(active, 'theirs')}>采用新的</Button>
            </>
          }
        >
          文件已被 AI 或其他人修改
        </Alert>
      )}
      {selection && buf.writable && !readOnly && (
        <Box sx={{ px: 1, py: 0.3, bgcolor: '#f3f3fe', display: 'flex', alignItems: 'center', gap: 1 }}>
          <Typography sx={{ fontSize: 12, color: '#434655' }}>
            已选中第 {selection.startLine}-{selection.endLine} 行
          </Typography>
          <Button size="small" startIcon={<AutoFixHighIcon />} onClick={() => onAskAi(selection)}>
            让 AI 修改这段
          </Button>
        </Box>
      )}
      <Box sx={{ flex: 1, minHeight: 0, display: 'flex' }}>
        {diffOpen && monaco ? (
          <Suspense fallback={null}>
            <DiffEditor
              original={buf.conflict ? buf.content : (lastDiffOriginal(active) ?? '')}
              modified={buf.conflict ? buf.conflict.content : buf.content}
              language={languageOf(active)}
              options={{ readOnly: true, renderSideBySide: true }}
            />
          </Suspense>
        ) : (
          <>
            {(!isMd || mode !== 'preview') && <Box sx={{ flex: 1, minWidth: 0 }}>{editor}</Box>}
            {isMd && mode !== 'source' && (
              <Box sx={{ flex: 1, minWidth: 0, borderLeft: mode === 'split' ? '1px solid #e7e7f3' : 0 }}>
                <MarkdownPreview content={buf.content} onLineClick={mode === 'split' ? revealLine : undefined} />
              </Box>
            )}
          </>
        )}
      </Box>
    </Box>
  );
}

/** Pre-turn content cache for the diff view (filled by AiPanel). */
const originals: Record<string, string> = {};
export function setLastDiffOriginal(path: string, content: string | null) {
  if (content === null) delete originals[path];
  else originals[path] = content;
}
function lastDiffOriginal(path: string): string | undefined {
  return originals[path];
}
