import { useState, useEffect } from 'react';
import {
  Box,
  Typography,
  Button,
  ToggleButtonGroup,
  ToggleButton,
  CircularProgress,
} from '@mui/material';
import { RestartAlt } from '@mui/icons-material';
import type { WorkflowTemplateMode } from '../types';

interface WorkflowEditorProps {
  templateMode: WorkflowTemplateMode;
  content: string;
  updatedAt: string | null;
  saving: boolean;
  onSave: (mode: WorkflowTemplateMode, content: string) => void;
  onReset: () => void;
  resetting: boolean;
}

export default function WorkflowEditor({
  templateMode,
  content,
  updatedAt,
  saving,
  onSave,
  onReset,
  resetting,
}: WorkflowEditorProps) {
  const [mode, setMode] = useState<WorkflowTemplateMode>(templateMode);
  const [editContent, setEditContent] = useState(content);

  useEffect(() => {
    setMode(templateMode);
    setEditContent(content);
  }, [templateMode, content]);

  const hasChanges = mode !== templateMode || (mode === 'custom' && editContent !== content);

  const handleSave = () => {
    onSave(mode, editContent);
  };

  return (
    <Box>
      <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 2 }}>
        <Box>
          <Typography variant="subtitle2" color="text.primary" sx={{ mb: 0.5 }}>
            模板模式
          </Typography>
          <ToggleButtonGroup
            value={mode}
            exclusive
            onChange={(_, val) => { if (val) setMode(val); }}
            size="small"
          >
            <ToggleButton value="default" aria-label="默认模板">
              默认模板
            </ToggleButton>
            <ToggleButton value="custom" aria-label="自定义">
              自定义
            </ToggleButton>
          </ToggleButtonGroup>
        </Box>
        {updatedAt && (
          <Typography variant="body2" color="text.secondary">
            最后更新: {new Date(updatedAt).toLocaleString('zh-CN')}
          </Typography>
        )}
      </Box>

      {mode === 'custom' ? (
        <Box
          component="textarea"
          value={editContent}
          onChange={(e: React.ChangeEvent<HTMLTextAreaElement>) => setEditContent(e.target.value)}
          aria-label="WORKFLOW.md 内容编辑器"
          sx={{
            width: '100%',
            minHeight: 360,
            p: 2,
            fontFamily: '"JetBrains Mono", "Fira Code", monospace',
            fontSize: '13px',
            lineHeight: '20px',
            bgcolor: '#f3f3fe',
            border: '1px solid #c3c6d7',
            borderRadius: '4px',
            resize: 'vertical',
            outline: 'none',
            '&:focus': {
              borderColor: '#0053db',
              boxShadow: '0 0 0 2px rgba(0, 83, 219, 0.1)',
            },
          }}
        />
      ) : (
        <Box
          sx={{
            width: '100%',
            minHeight: 360,
            p: 2,
            fontFamily: '"JetBrains Mono", "Fira Code", monospace',
            fontSize: '13px',
            lineHeight: '20px',
            bgcolor: '#f8f8fc',
            border: '1px solid #e0e0e8',
            borderRadius: '4px',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            color: '#434655',
            overflow: 'auto',
            maxHeight: 500,
          }}
        >
          {content || '（使用平台默认模板）'}
        </Box>
      )}

      <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', mt: 2 }}>
        <Button
          variant="outlined"
          color="inherit"
          startIcon={resetting ? <CircularProgress size={16} color="inherit" /> : <RestartAlt />}
          onClick={onReset}
          disabled={resetting || templateMode === 'default'}
          size="small"
        >
          重置为默认模板
        </Button>
        <Button
          variant="contained"
          onClick={handleSave}
          disabled={!hasChanges || saving}
          startIcon={saving ? <CircularProgress size={16} color="inherit" /> : undefined}
        >
          保存
        </Button>
      </Box>
    </Box>
  );
}
