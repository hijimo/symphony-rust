import {
  Box,
  TextField,
  Typography,
  Divider,
} from '@mui/material';
import AiGenerateButton from './AiGenerateButton';
import StreamingDisplay from './StreamingDisplay';
import MarkdownEditor from './MarkdownEditor';
import LabelSelector from './LabelSelector';
import CommandWarning from './CommandWarning';
import type { AIGenerateStatus } from '../../store/issueStore';

interface IssueFormProps {
  title: string;
  onTitleChange: (value: string) => void;
  prompt: string;
  onPromptChange: (value: string) => void;
  description: string;
  onDescriptionChange: (value: string) => void;
  labels: string[];
  onLabelsChange: (labels: string[]) => void;
  assignee: string;
  onAssigneeChange: (value: string) => void;
  // AI generation
  aiStatus: AIGenerateStatus;
  generatedContent: string;
  aiError: string | null;
  onGenerate: () => void;
  onStopGenerate: () => void;
  onApplyGenerated: () => void;
  // Form state
  disabled?: boolean;
}

export default function IssueForm({
  title,
  onTitleChange,
  prompt,
  onPromptChange,
  description,
  onDescriptionChange,
  labels,
  onLabelsChange,
  assignee,
  onAssigneeChange,
  aiStatus,
  generatedContent,
  aiError,
  onGenerate,
  onStopGenerate,
  onApplyGenerated,
  disabled,
}: IssueFormProps) {
  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
      {/* Title */}
      <TextField
        label="Issue 标题"
        value={title}
        onChange={(e) => onTitleChange(e.target.value)}
        disabled={disabled}
        fullWidth
        required
        inputProps={{ maxLength: 200 }}
        helperText={`${title.length}/200`}
        placeholder="简要描述问题或需求"
      />

      {/* AI Generation Section */}
      <Box>
        <Typography variant="subtitle2" color="text.secondary" sx={{ mb: 1.5 }}>
          AI 辅助生成
        </Typography>
        <Box sx={{ display: 'flex', gap: 1.5, alignItems: 'flex-start' }}>
          <TextField
            label="需求描述"
            value={prompt}
            onChange={(e) => onPromptChange(e.target.value)}
            disabled={disabled || aiStatus === 'generating'}
            fullWidth
            multiline
            minRows={2}
            maxRows={4}
            inputProps={{ maxLength: 2000 }}
            placeholder="用一两句话描述你的需求，AI 将生成结构化的 Issue 内容"
            helperText={`${prompt.length}/2000`}
          />
          <Box sx={{ pt: 1, flexShrink: 0 }}>
            <AiGenerateButton
              status={aiStatus}
              disabled={disabled || prompt.trim().length < 5}
              onGenerate={onGenerate}
              onStop={onStopGenerate}
            />
          </Box>
        </Box>

        {/* Streaming Display */}
        {(aiStatus !== 'idle' || generatedContent) && (
          <Box sx={{ mt: 2 }}>
            <StreamingDisplay
              content={generatedContent}
              status={aiStatus}
              error={aiError}
            />
            {/* Command Warning */}
            {generatedContent && (
              <Box sx={{ mt: 1.5 }}>
                <CommandWarning content={generatedContent} />
              </Box>
            )}
            {/* Apply button */}
            {aiStatus === 'done' && generatedContent && (
              <Box sx={{ mt: 1.5, display: 'flex', justifyContent: 'flex-end' }}>
                <Typography
                  component="button"
                  onClick={onApplyGenerated}
                  sx={{
                    cursor: 'pointer',
                    border: 'none',
                    background: 'none',
                    color: '#0053db',
                    fontSize: '14px',
                    fontWeight: 500,
                    textDecoration: 'underline',
                    '&:hover': { color: '#003ea8' },
                  }}
                >
                  应用到描述
                </Typography>
              </Box>
            )}
          </Box>
        )}
      </Box>

      <Divider />

      {/* Description Editor */}
      <Box>
        <Typography variant="subtitle2" color="text.secondary" sx={{ mb: 1.5 }}>
          Issue 描述
        </Typography>
        <MarkdownEditor
          value={description}
          onChange={onDescriptionChange}
          disabled={disabled}
          placeholder="输入 Issue 描述（支持 Markdown 格式）..."
        />
      </Box>

      <Divider />

      {/* Labels & Assignee */}
      <Box sx={{ display: 'flex', gap: 3, flexWrap: 'wrap' }}>
        <Box sx={{ flex: 1, minWidth: 240 }}>
          <LabelSelector
            value={labels}
            onChange={onLabelsChange}
            disabled={disabled}
          />
        </Box>
        <Box sx={{ flex: 1, minWidth: 240 }}>
          <TextField
            label="指派人"
            value={assignee}
            onChange={(e) => onAssigneeChange(e.target.value)}
            disabled={disabled}
            fullWidth
            placeholder="GitLab/GitHub 用户名"
            inputProps={{ maxLength: 100 }}
            helperText="留空则不指派"
          />
        </Box>
      </Box>
    </Box>
  );
}
