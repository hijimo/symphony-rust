import { useEffect, useState } from 'react';
import {
  Box,
  Typography,
  Card,
  CardContent,
  Button,
  Chip,
  Avatar,
  CircularProgress,
  Link,
} from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import OpenInNewIcon from '@mui/icons-material/OpenInNew';
import { useNavigate, useParams } from 'react-router-dom';
import { getIssue } from '../../api/issues';
import type { IssueDetail } from '../../types/issue';

export default function IssueDetailPage() {
  const navigate = useNavigate();
  const { id, iid } = useParams<{ id: string; iid: string }>();
  const projectId = Number(id);
  const issueIid = Number(iid);

  const [issue, setIssue] = useState<IssueDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    async function fetchIssue() {
      setLoading(true);
      try {
        const data = await getIssue(projectId, issueIid);
        setIssue(data);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : '加载 Issue 失败';
        setError(message);
      } finally {
        setLoading(false);
      }
    }
    fetchIssue();
  }, [projectId, issueIid]);

  if (loading) {
    return (
      <Box sx={{ display: 'flex', justifyContent: 'center', py: 8 }}>
        <CircularProgress />
      </Box>
    );
  }

  if (error || !issue) {
    return (
      <Box>
        <Button
          variant="text"
          startIcon={<ArrowBackIcon />}
          onClick={() => navigate(-1)}
          sx={{ color: '#434655', mb: 2 }}
        >
          返回
        </Button>
        <Typography color="error">{error || 'Issue 不存在'}</Typography>
      </Box>
    );
  }

  return (
    <Box>
      {/* Header */}
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, mb: 3 }}>
        <Button
          variant="text"
          startIcon={<ArrowBackIcon />}
          onClick={() => navigate(-1)}
          sx={{ color: '#434655', minWidth: 'auto', px: 1 }}
        >
          返回
        </Button>
        <Typography variant="h5" color="text.primary" sx={{ flex: 1 }}>
          #{issue.iid} {issue.title}
        </Typography>
        <Chip
          label={issue.state === 'opened' ? '开启' : '已关闭'}
          size="small"
          sx={{
            backgroundColor: issue.state === 'opened' ? '#dbe1ff' : '#e1e2ed',
            color: issue.state === 'opened' ? '#003ea8' : '#434655',
            fontWeight: 500,
          }}
        />
      </Box>

      <Box sx={{ display: 'flex', gap: 3, flexWrap: 'wrap' }}>
        {/* Main Content */}
        <Card sx={{ flex: 2, minWidth: 400, border: '1px solid #c3c6d7' }}>
          <CardContent sx={{ p: 3 }}>
            {issue.description ? (
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
                {issue.description}
              </Box>
            ) : (
              <Typography variant="body1" color="text.secondary">
                暂无描述
              </Typography>
            )}
          </CardContent>
        </Card>

        {/* Sidebar */}
        <Box sx={{ flex: 1, minWidth: 240, display: 'flex', flexDirection: 'column', gap: 2 }}>
          {/* Meta Info */}
          <Card sx={{ border: '1px solid #c3c6d7' }}>
            <CardContent sx={{ p: 2 }}>
              {/* Labels */}
              {issue.labels.length > 0 && (
                <Box sx={{ mb: 2 }}>
                  <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                    标签
                  </Typography>
                  <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap' }}>
                    {issue.labels.map((label) => (
                      <Chip
                        key={label}
                        label={label}
                        size="small"
                        sx={{
                          backgroundColor: '#ededf9',
                          color: '#191b23',
                          borderRadius: '4px',
                          fontSize: '12px',
                        }}
                      />
                    ))}
                  </Box>
                </Box>
              )}

              {/* Assignees */}
              {issue.assignees.length > 0 && (
                <Box sx={{ mb: 2 }}>
                  <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                    指派人
                  </Typography>
                  {issue.assignees.map((user) => (
                    <Box key={user.username} sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}>
                      <Avatar
                        src={user.avatar_url || undefined}
                        sx={{ width: 20, height: 20, fontSize: '11px' }}
                      >
                        {user.username[0].toUpperCase()}
                      </Avatar>
                      <Typography variant="body2">
                        {user.display_name || user.username}
                      </Typography>
                    </Box>
                  ))}
                </Box>
              )}

              {/* Milestone */}
              {issue.milestone && (
                <Box sx={{ mb: 2 }}>
                  <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                    里程碑
                  </Typography>
                  <Typography variant="body2">{issue.milestone}</Typography>
                </Box>
              )}

              {/* Author */}
              <Box sx={{ mb: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                  创建者
                </Typography>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                  <Avatar
                    src={issue.author.avatar_url || undefined}
                    sx={{ width: 20, height: 20, fontSize: '11px' }}
                  >
                    {issue.author.username[0].toUpperCase()}
                  </Avatar>
                  <Typography variant="body2">
                    {issue.author.display_name || issue.author.username}
                  </Typography>
                </Box>
              </Box>

              {/* External Link */}
              <Link
                href={issue.web_url}
                target="_blank"
                rel="noopener noreferrer"
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 0.5,
                  fontSize: '13px',
                  color: '#0053db',
                  textDecoration: 'none',
                  '&:hover': { textDecoration: 'underline' },
                }}
              >
                在平台中查看
                <OpenInNewIcon sx={{ fontSize: 14 }} />
              </Link>
            </CardContent>
          </Card>

          {/* Related MRs */}
          {issue.related_mrs.length > 0 && (
            <Card sx={{ border: '1px solid #c3c6d7' }}>
              <CardContent sx={{ p: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 1, display: 'block' }}>
                  关联 MR/PR
                </Typography>
                {issue.related_mrs.map((mr) => (
                  <Box
                    key={mr.iid}
                    sx={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 1,
                      mb: 1,
                      cursor: 'pointer',
                      '&:hover': { backgroundColor: '#f3f3fe' },
                      borderRadius: '4px',
                      p: 0.5,
                    }}
                    onClick={() => navigate(`/projects/${projectId}/mrs/${mr.iid}`)}
                  >
                    <Chip
                      label={mr.state}
                      size="small"
                      sx={{
                        fontSize: '10px',
                        height: 18,
                        backgroundColor:
                          mr.state === 'merged'
                            ? '#dbe1ff'
                            : mr.state === 'opened'
                              ? '#e7e7f3'
                              : '#ffdad6',
                        color:
                          mr.state === 'merged'
                            ? '#003ea8'
                            : mr.state === 'opened'
                              ? '#434655'
                              : '#93000a',
                      }}
                    />
                    <Typography variant="body2" sx={{ flex: 1 }} noWrap>
                      !{mr.iid} {mr.title}
                    </Typography>
                  </Box>
                ))}
              </CardContent>
            </Card>
          )}
        </Box>
      </Box>
    </Box>
  );
}
