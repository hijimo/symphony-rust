import { useState } from 'react';
import { Box, TextField, Tab, Tabs, Typography } from '@mui/material';

interface MarkdownEditorProps {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  minRows?: number;
}

export default function MarkdownEditor({
  value,
  onChange,
  disabled,
  placeholder = '输入 Markdown 内容...',
  minRows = 12,
}: MarkdownEditorProps) {
  const [tab, setTab] = useState<'edit' | 'preview'>('edit');

  return (
    <Box>
      <Tabs
        value={tab}
        onChange={(_, v) => setTab(v)}
        sx={{
          minHeight: 32,
          mb: 1,
          '& .MuiTab-root': {
            minHeight: 32,
            py: 0.5,
            px: 1.5,
            fontSize: '12px',
            fontWeight: 500,
          },
        }}
      >
        <Tab label="编辑" value="edit" />
        <Tab label="预览" value="preview" />
      </Tabs>

      {tab === 'edit' ? (
        <TextField
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={disabled}
          placeholder={placeholder}
          fullWidth
          multiline
          minRows={minRows}
          maxRows={24}
          inputProps={{ maxLength: 65536 }}
          sx={{
            '& .MuiFilledInput-root': {
              fontFamily: '"JetBrains Mono", "Fira Code", monospace',
              fontSize: '13px',
              lineHeight: '20px',
            },
          }}
        />
      ) : (
        <Box
          sx={{
            backgroundColor: '#f3f3fe',
            borderRadius: '4px',
            p: 2,
            minHeight: minRows * 20 + 32,
            maxHeight: 480,
            overflow: 'auto',
          }}
        >
          {value ? (
            <Box
              component="pre"
              sx={{
                fontFamily: '"Inter", sans-serif',
                fontSize: '14px',
                lineHeight: '22px',
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
                margin: 0,
                color: '#191b23',
              }}
            >
              {value}
            </Box>
          ) : (
            <Typography variant="body2" color="text.secondary">
              暂无内容
            </Typography>
          )}
        </Box>
      )}
    </Box>
  );
}
