import { useState } from 'react';
import {
  Box,
  Typography,
  Card,
  CardContent,
  Button,
  CircularProgress,
  Snackbar,
  Alert,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import { useNavigate, useParams } from 'react-router-dom';
import IssueForm from '../../components/issues/IssueForm';
import { useIssueStore } from '../../store/issueStore';
import { createIssue } from '../../api/issues';

export default function CreateIssuePage() {
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const projectId = Number(id);

  const {
    aiStatus,
    generatedContent,
    aiError,
    startGenerate,
    stopGenerate,
    resetAI,
  } = useIssueStore();

  const [title, setTitle] = useState('');
  const [prompt, setPrompt] = useState('');
  const [description, setDescription] = useState('');
  const [labels, setLabels] = useState<string[]>([]);
  const [assignee, setAssignee] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [snackError, setSnackError] = useState('');

  const handleGenerate = () => {
    if (!projectId || prompt.trim().length < 5) return;
    startGenerate(projectId, {
      prompt: prompt.trim(),
      title: title.trim() || undefined,
    });
  };

  const handleApplyGenerated = () => {
    setDescription(generatedContent);
  };

  const handleSubmit = async () => {
    if (!title.trim()) {
      setSnackError('请输入 Issue 标题');
      return;
    }

    setSubmitting(true);
    try {
      const issue = await createIssue(projectId, {
        title: title.trim(),
        description: description.trim() || undefined,
        labels: labels.length > 0 ? labels : undefined,
        assignee: assignee.trim() || undefined,
      });
      resetAI();
      navigate(`/projects/${projectId}/issues/${issue.iid}`, { replace: true });
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : '创建 Issue 失败';
      setSnackError(message);
    } finally {
      setSubmitting(false);
    }
  };

  const handleCancel = () => {
    resetAI();
    navigate(-1);
  };

  return (
    <Box>
      {/* Header */}
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, mb: 3 }}>
        <Button
          variant="text"
          startIcon={<ArrowBackIcon />}
          onClick={handleCancel}
          sx={{ color: '#434655', minWidth: 'auto', px: 1 }}
        >
          返回
        </Button>
        <Typography variant="h5" color="text.primary">
          创建 Issue
        </Typography>
      </Box>

      <Card sx={{ maxWidth: 800, border: '1px solid #c3c6d7' }}>
        <CardContent sx={{ p: 3 }}>
          <IssueForm
            title={title}
            onTitleChange={setTitle}
            prompt={prompt}
            onPromptChange={setPrompt}
            description={description}
            onDescriptionChange={setDescription}
            labels={labels}
            onLabelsChange={setLabels}
            assignee={assignee}
            onAssigneeChange={setAssignee}
            aiStatus={aiStatus}
            generatedContent={generatedContent}
            aiError={aiError}
            onGenerate={handleGenerate}
            onStopGenerate={stopGenerate}
            onApplyGenerated={handleApplyGenerated}
            disabled={submitting}
          />

          {/* Submit Buttons */}
          <Box sx={{ display: 'flex', justifyContent: 'flex-end', mt: 4, gap: 1.5 }}>
            <Button
              variant="outlined"
              onClick={handleCancel}
              disabled={submitting}
              sx={{ borderColor: '#c3c6d7', color: '#434655' }}
            >
              取消
            </Button>
            <Button
              variant="contained"
              onClick={handleSubmit}
              disabled={submitting || !title.trim()}
              startIcon={
                submitting ? <CircularProgress size={16} color="inherit" /> : undefined
              }
            >
              创建 Issue
            </Button>
          </Box>
        </CardContent>
      </Card>

      {/* Error Snackbar */}
      <Snackbar
        open={!!snackError}
        autoHideDuration={6000}
        onClose={() => setSnackError('')}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      >
        <Alert
          severity="error"
          onClose={() => setSnackError('')}
          variant="filled"
          sx={{ borderRadius: '4px' }}
        >
          {snackError}
        </Alert>
      </Snackbar>
    </Box>
  );
}
