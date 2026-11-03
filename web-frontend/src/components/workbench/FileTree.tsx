import { useState } from 'react';
import { Box, IconButton, InputBase, List, ListItemButton, Tooltip, Typography } from '@mui/material';
import ChevronRightIcon from '@mui/icons-material/ChevronRight';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import InsertDriveFileOutlinedIcon from '@mui/icons-material/InsertDriveFileOutlined';
import FolderOutlinedIcon from '@mui/icons-material/FolderOutlined';
import SearchIcon from '@mui/icons-material/Search';
import UploadFileIcon from '@mui/icons-material/UploadFile';
import EditNoteIcon from '@mui/icons-material/EditNote';
import { useWorkbenchStore } from '../../store/workbenchStore';
import * as api from '../../api/changes';
import type { SearchHit } from '../../types/changes';
import UploadDialog from './UploadDialog';

type UploadFn = (dir: string, files?: File[]) => void;

function Node({ path, depth, onOpen, onUpload }: { path: string; depth: number; onOpen?: () => void; onUpload: UploadFn }) {
  const { tree, expanded, toggleDir, openFile, active, aiTouched } = useWorkbenchStore();
  const entries = tree[path] ?? [];
  return (
    <>
      {entries.map((e) => (
        <Box key={e.path}>
          <ListItemButton
            dense
            selected={active === e.path}
            onClick={() => (e.kind === 'dir' ? toggleDir(e.path) : void openFile(e.path).then(() => onOpen?.()))}
            onDragOver={e.kind === 'dir' && e.writable ? (ev) => ev.preventDefault() : undefined}
            onDrop={
              e.kind === 'dir' && e.writable
                ? (ev) => {
                    ev.preventDefault();
                    ev.stopPropagation();
                    const files = Array.from(ev.dataTransfer.files);
                    if (files.length) onUpload(e.path, files);
                  }
                : undefined
            }
            sx={{ pl: 0.5 + depth * 1.5, py: 0.1, minHeight: 24 }}
            data-testid={`tree-${e.path}`}
          >
            {e.kind === 'dir' ? (
              expanded[e.path] ? <ExpandMoreIcon sx={{ fontSize: 16 }} /> : <ChevronRightIcon sx={{ fontSize: 16 }} />
            ) : (
              <Box sx={{ width: 16 }} />
            )}
            {e.kind === 'dir' ? (
              <FolderOutlinedIcon sx={{ fontSize: 16, mr: 0.5, color: '#737686' }} />
            ) : (
              <InsertDriveFileOutlinedIcon sx={{ fontSize: 15, mr: 0.5, color: '#737686' }} />
            )}
            <Typography noWrap sx={{ fontSize: 13, flex: 1 }}>
              {e.name}
            </Typography>
            {e.writable && e.kind === 'dir' && (
              <Tooltip title="上传文件到此目录">
                <IconButton
                  size="small"
                  aria-label={`上传到 ${e.path}`}
                  onClick={(ev) => {
                    ev.stopPropagation();
                    onUpload(e.path);
                  }}
                  sx={{ p: 0.25 }}
                >
                  <UploadFileIcon sx={{ fontSize: 14, color: '#0053db' }} />
                </IconButton>
              </Tooltip>
            )}
            {e.writable && e.kind === 'file' && (
              <Tooltip title="可编辑">
                <EditNoteIcon sx={{ fontSize: 14, color: '#0053db' }} />
              </Tooltip>
            )}
            {aiTouched[e.path] && (
              <Box
                aria-label="AI 本轮修改"
                sx={{ width: 7, height: 7, borderRadius: '50%', bgcolor: '#ac3500', ml: 0.5 }}
              />
            )}
          </ListItemButton>
          {e.kind === 'dir' && expanded[e.path] && <Node path={e.path} depth={depth + 1} onOpen={onOpen} onUpload={onUpload} />}
        </Box>
      ))}
    </>
  );
}

export default function FileTree({ onOpen }: { onOpen?: () => void } = {}) {
  const { projectId, iid, openFile, pushNotice, flow } = useWorkbenchStore();
  const [upload, setUpload] = useState<{ dir?: string; files?: File[] } | null>(null);
  const onUpload: UploadFn = (dir, files) => setUpload({ dir, files });
  const [q, setQ] = useState('');
  const [mode, setMode] = useState<'file' | 'content'>('file');
  const [hits, setHits] = useState<SearchHit[] | null>(null);

  const runSearch = async () => {
    if (!q.trim()) {
      setHits(null);
      return;
    }
    try {
      setHits(await api.search(projectId, iid, q.trim(), mode));
    } catch (e) {
      pushNotice('error', (e as Error).message);
    }
  };

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', bgcolor: '#fff' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', px: 1, py: 0.5, borderBottom: '1px solid #e7e7f3', gap: 0.5 }}>
        <SearchIcon sx={{ fontSize: 16, color: '#737686' }} />
        <InputBase
          placeholder={mode === 'file' ? '搜索文件名' : '搜索内容'}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && runSearch()}
          sx={{ fontSize: 13, flex: 1 }}
          inputProps={{ 'aria-label': '搜索' }}
        />
        {flow && (
          <Tooltip title="上传文件（如 xlsx 评分标准）到文档目录">
            <IconButton size="small" aria-label="上传文件" onClick={() => setUpload({ dir: flow.writableDir })}>
              <UploadFileIcon sx={{ fontSize: 16 }} />
            </IconButton>
          </Tooltip>
        )}
        <Tooltip title={mode === 'file' ? '切换为内容搜索' : '切换为文件名搜索'}>
          <IconButton size="small" onClick={() => setMode(mode === 'file' ? 'content' : 'file')} sx={{ fontSize: 11 }}>
            {mode === 'file' ? '名' : '文'}
          </IconButton>
        </Tooltip>
      </Box>
      <List dense sx={{ overflow: 'auto', flex: 1, py: 0 }} aria-label="文件目录">
        {hits ? (
          <>
            <ListItemButton dense onClick={() => setHits(null)}>
              <Typography sx={{ fontSize: 12, color: '#0053db' }}>← 返回目录（{hits.length} 条结果）</Typography>
            </ListItemButton>
            {hits.map((h, i) => (
              <ListItemButton key={`${h.path}:${h.line}:${i}`} dense onClick={() => void openFile(h.path).then(() => onOpen?.())}>
                <Box sx={{ minWidth: 0 }}>
                  <Typography noWrap sx={{ fontSize: 12 }}>
                    {h.path}
                    {h.line ? `:${h.line}` : ''}
                  </Typography>
                  {h.text && (
                    <Typography noWrap sx={{ fontSize: 11, color: '#737686', fontFamily: 'monospace' }}>
                      {h.text}
                    </Typography>
                  )}
                </Box>
              </ListItemButton>
            ))}
          </>
        ) : (
          <Node path="" depth={0} onOpen={onOpen} onUpload={onUpload} />
        )}
      </List>
      <UploadDialog
        open={upload !== null}
        initialDir={upload?.dir}
        initialFiles={upload?.files}
        onClose={() => setUpload(null)}
      />
    </Box>
  );
}
