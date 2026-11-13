import { useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Autocomplete,
  Box,
  Button,
  Checkbox,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControlLabel,
  TextField,
  Typography,
} from '@mui/material';
import { useWorkbenchStore } from '../../store/workbenchStore';
import * as api from '../../api/changes';

/** Largest attachment accepted by the server. */
export const MAX_UPLOAD_BYTES = 10 * 1024 * 1024;

const DOC_FILES = ['intent.md', 'spec.md', 'plan.md'];

/**
 * Normalize a user-typed directory inside the writable root: trims slashes,
 * rejects `..`, `.` segments and hidden names. Returns `null` when invalid.
 */
export function normalizeSubdir(input: string): string | null {
  const parts = input
    .replace(/\\/g, '/')
    .split('/')
    .map((p) => p.trim())
    .filter((p) => p !== '');
  if (parts.some((p) => p === '..' || p === '.' || p.startsWith('.'))) return null;
  return parts.join('/');
}

/** Join the writable root, an optional subdirectory and a file name. */
export function uploadTarget(writableDir: string, subdir: string, name: string): string {
  return [writableDir, subdir, name].filter((p) => p !== '').join('/');
}

export default function UploadDialog({
  open,
  initialDir,
  initialFiles,
  onClose,
}: {
  open: boolean;
  /** Pre-selected directory (workspace-relative, inside the writable root). */
  initialDir?: string;
  initialFiles?: File[];
  onClose: () => void;
}) {
  const { projectId, iid, flow, tree, loadDir, openFile, pushNotice } = useWorkbenchStore();
  const root = flow?.writableDir ?? '';
  const [subdir, setSubdir] = useState('');
  const [files, setFiles] = useState<File[]>([]);
  const [overwrite, setOverwrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const rel = initialDir && initialDir.startsWith(root) ? initialDir.slice(root.length).replace(/^\//, '') : '';
    setSubdir(rel);
    setFiles(initialFiles ?? []);
    setOverwrite(false);
    setError(null);
    if (root) void loadDir(root).catch(() => undefined);
  }, [open, initialDir, initialFiles, root, loadDir]);

  // Existing subdirectories already listed in the tree (choose or type a new one).
  const dirOptions = useMemo(() => {
    const out = new Set<string>();
    Object.entries(tree).forEach(([dir, entries]) => {
      if (dir !== root && !dir.startsWith(`${root}/`)) return;
      entries
        .filter((e) => e.kind === 'dir')
        .forEach((e) => out.add(e.path.slice(root.length).replace(/^\//, '')));
    });
    return [...out].sort();
  }, [tree, root]);

  const normalized = normalizeSubdir(subdir);
  const tooBig = files.filter((f) => f.size > MAX_UPLOAD_BYTES);
  const docClash = normalized === '' && files.some((f) => DOC_FILES.includes(f.name));
  const canUpload = !busy && files.length > 0 && normalized !== null && tooBig.length === 0 && !docClash;

  const upload = async () => {
    if (normalized === null) return;
    setBusy(true);
    setError(null);
    const done: string[] = [];
    try {
      for (const f of files) {
        const path = uploadTarget(root, normalized, f.name);
        await api.uploadFile(projectId, iid, path, f, overwrite);
        done.push(path);
      }
      await loadDir(root).catch(() => undefined);
      if (normalized) await loadDir(uploadTarget(root, normalized, '')).catch(() => undefined);
      pushNotice('info', `已上传并提交 ${done.length} 个文件`);
      if (done.length === 1) void openFile(done[0]);
      onClose();
    } catch (e) {
      const msg = (e as Error).message;
      setError(done.length ? `已上传 ${done.length} 个；${files[done.length].name} 失败：${msg}` : msg);
      setFiles((fs) => fs.slice(done.length));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onClose={busy ? undefined : onClose} fullWidth maxWidth="sm">
      <DialogTitle>上传文件</DialogTitle>
      <DialogContent sx={{ display: 'flex', flexDirection: 'column', gap: 2, pt: '8px !important' }}>
        <Typography variant="body2" color="text.secondary">
          文件保存到本 Issue 的文档目录并随文档提交，AI 起草和实现时都能读取。可选择已有目录，或输入新目录（如 assets/评分）。
        </Typography>
        <Autocomplete
          freeSolo
          options={dirOptions}
          value={subdir}
          inputValue={subdir}
          onInputChange={(_, v) => setSubdir(v)}
          renderInput={(params) => (
            <TextField
              {...params}
              label="目录"
              placeholder="留空表示文档目录根"
              error={normalized === null}
              helperText={
                normalized === null
                  ? '目录不能包含 ..、. 或以 . 开头的名称'
                  : `保存到 ${uploadTarget(root, normalized, '') || root}/`
              }
              inputProps={{ ...params.inputProps, 'aria-label': '目录' }}
            />
          )}
        />
        <Box>
          <Button variant="outlined" component="label" disabled={busy}>
            选择文件
            <input
              hidden
              multiple
              type="file"
              aria-label="选择文件"
              onChange={(e) => {
                setFiles(Array.from(e.target.files ?? []));
                e.target.value = '';
              }}
            />
          </Button>
          {files.map((f) => (
            <Typography key={f.name} variant="body2" sx={{ mt: 0.5 }}>
              {f.name}（{(f.size / 1024).toFixed(1)} KB）
            </Typography>
          ))}
        </Box>
        <FormControlLabel
          control={<Checkbox checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)} size="small" />}
          label="同名文件已存在时覆盖"
        />
        {tooBig.length > 0 && <Alert severity="error">单个文件不能超过 10MB：{tooBig.map((f) => f.name).join('、')}</Alert>}
        {docClash && <Alert severity="error">intent.md / spec.md / plan.md 请在编辑器中修改，不能上传覆盖</Alert>}
        {error && <Alert severity="error">{error}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={busy}>
          取消
        </Button>
        <Button variant="contained" onClick={() => void upload()} disabled={!canUpload}>
          {busy ? '上传中…' : '上传'}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
