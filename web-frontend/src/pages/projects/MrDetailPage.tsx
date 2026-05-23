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
import { getMergeRequest } from '../../api/issues';
import type { MergeRequestDetail } from '../../types/issue';

function getCiStatusColor(status: string | null): { bg: string; color: string } {
  switch (status) {
    case 'success':
      return { bg: '#dbe1ff', color: '#003ea8' };
    case 'failed':
      return { bg: '#ffdad6', color: '#93000a' };
    case 'running':
    case 'pending':
      return { bg: '#ededf9', color: '#434655' };
    case 'canceled':
      return { bg: '#e1e2ed', color: '#434655' };
    default:
      return { bg: '#e1e2ed', color: '#434655' };
  }
}

function getReviewStatusLabel(status: string | null): string {
  switch (status) {
    case 'approved':
      return '已批准';
    case 'changes_requested':
      return '需修改';
    case 'pending':
      return '待审核';
    default:
      return '无';
  }
}

export default function MrDetailPage() {
  const navigate = useNavigate();
  const { id, iid } = useParams<{ id: string; iid: string }>();
  const projectId = Number(id);
  const mrIid = Number(iid);

  const [mr, setMr] = useState<MergeRequestDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    async function fetchMr() {
      setLoading(true);
      try {
        const data = await getMergeRequest(projectId, mrIid);
        setMr(data);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : '加载 MR 详情失败';
        setError(message);
      } finally {
        setLoading(false);
      }
    }
    fetchMr();
  }, [projectId, mrIid]);

  if (loading) {
    return (
      <Box sx={{ display: 'flex', justifyContent: 'center', py: 8 }}>
        <CircularProgress />
      </Box>
    );
  }

  if (error || !mr) {
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
        <Typography color="error">{error || 'MR/PR 不存在'}</Typography>
      </Box>
    );
  }

  const ciColors = getCiStatusColor(mr.ci_status);

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
          !{mr.iid} {mr.title}
        </Typography>
        <Chip
          label={mr.state === 'merged' ? '已合并' : mr.state === 'opened' ? '开启' : '已关闭'}
          size="small"
          sx={{
            backgroundColor:
              mr.state === 'merged' ? '#dbe1ff' : mr.state === 'opened' ? '#e7e7f3' : '#ffdad6',
            color:
              mr.state === 'merged' ? '#003ea8' : mr.state === 'opened' ? '#434655' : '#93000a',
            fontWeight: 500,
          }}
        />
      </Box>

      <Box sx={{ display: 'flex', gap: 3, flexWrap: 'wrap' }}>
        {/* Main Content */}
        <Card sx={{ flex: 2, minWidth: 400, border: '1px solid #c3c6d7' }}>
          <CardContent sx={{ p: 3 }}>
            {/* Branch info */}
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 2 }}>
              <Chip
                label={mr.source_branch}
                size="small"
                sx={{ backgroundColor: '#ededf9', fontSize: '12px', fontFamily: 'monospace' }}
              />
              <Typography variant="body2" color="text.secondary">
                →
              </Typography>
              <Chip
                label={mr.target_branch}
                size="small"
                sx={{ backgroundColor: '#ededf9', fontSize: '12px', fontFamily: 'monospace' }}
              />
            </Box>

            {/* Description */}
            {mr.description ? (
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
                {mr.description}
              </Box>
            ) : (
              <Typography variant="body1" color="text.secondary">
                暂无描述
              </Typography>
            )}

            {/* Code Stats */}
            <Box
              sx={{
                display: 'flex',
                gap: 3,
                mt: 3,
                pt: 2,
                borderTop: '1px solid #c3c6d7',
              }}
            >
              <Box>
                <Typography variant="body2" color="text.secondary">
                  变更文件
                </Typography>
                <Typography variant="subtitle1">{mr.changed_files}</Typography>
              </Box>
              <Box>
                <Typography variant="body2" color="text.secondary">
                  新增
                </Typography>
                <Typography variant="subtitle1" sx={{ color: '#003ea8' }}>
                  +{mr.additions}
                </Typography>
              </Box>
              <Box>
                <Typography variant="body2" color="text.secondary">
                  删除
                </Typography>
                <Typography variant="subtitle1" sx={{ color: '#ba1a1a' }}>
                  -{mr.deletions}
                </Typography>
              </Box>
            </Box>
          </CardContent>
        </Card>

        {/* Sidebar */}
        <Box sx={{ flex: 1, minWidth: 240, display: 'flex', flexDirection: 'column', gap: 2 }}>
          {/* Status Card */}
          <Card sx={{ border: '1px solid #c3c6d7' }}>
            <CardContent sx={{ p: 2 }}>
              {/* CI Status */}
              <Box sx={{ mb: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                  CI 状态
                </Typography>
                {mr.ci_status ? (
                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                    <Chip
                      label={mr.ci_status}
                      size="small"
                      sx={{
                        backgroundColor: ciColors.bg,
                        color: ciColors.color,
                        fontSize: '12px',
                      }}
                    />
                    {mr.ci_web_url && (
                      <Link
                        href={mr.ci_web_url}
                        target="_blank"
                        rel="noopener noreferrer"
                        sx={{ fontSize: '12px' }}
                      >
                        查看
                      </Link>
                    )}
                  </Box>
                ) : (
                  <Typography variant="body2" color="text.secondary">
                    无流水线
                  </Typography>
                )}
              </Box>

              {/* Review Status */}
              <Box sx={{ mb: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                  Review 状态
                </Typography>
                <Typography variant="body2">
                  {getReviewStatusLabel(mr.review_status)}
                </Typography>
                {mr.reviewers.length > 0 && (
                  <Box sx={{ mt: 0.5 }}>
                    {mr.reviewers.map((reviewer) => (
                      <Box
                        key={reviewer.user.username}
                        sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}
                      >
                        <Avatar
                          src={reviewer.user.avatar_url || undefined}
                          sx={{ width: 18, height: 18, fontSize: '10px' }}
                        >
                          {reviewer.user.username[0].toUpperCase()}
                        </Avatar>
                        <Typography variant="body2" sx={{ flex: 1 }}>
                          {reviewer.user.display_name || reviewer.user.username}
                        </Typography>
                        <Chip
                          label={reviewer.state === 'approved' ? '✓' : reviewer.state === 'changes_requested' ? '✗' : '…'}
                          size="small"
                          sx={{ height: 16, fontSize: '10px', minWidth: 20 }}
                        />
                      </Box>
                    ))}
                  </Box>
                )}
              </Box>

              {/* Merge Status */}
              <Box sx={{ mb: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                  合并状态
                </Typography>
                <Typography variant="body2">
                  {mr.merge_status === 'can_be_merged'
                    ? '可合并'
                    : mr.merge_status === 'cannot_be_merged'
                      ? '存在冲突'
                      : mr.merge_status === 'checking'
                        ? '检查中'
                        : '未检查'}
                </Typography>
              </Box>

              {/* Author */}
              <Box sx={{ mb: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 0.5, display: 'block' }}>
                  作者
                </Typography>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                  <Avatar
                    src={mr.author.avatar_url || undefined}
                    sx={{ width: 20, height: 20, fontSize: '11px' }}
                  >
                    {mr.author.username[0].toUpperCase()}
                  </Avatar>
                  <Typography variant="body2">
                    {mr.author.display_name || mr.author.username}
                  </Typography>
                </Box>
              </Box>

              {/* External Link */}
              <Link
                href={mr.web_url}
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

          {/* Related Issues */}
          {mr.related_issues.length > 0 && (
            <Card sx={{ border: '1px solid #c3c6d7' }}>
              <CardContent sx={{ p: 2 }}>
                <Typography variant="overline" color="text.secondary" sx={{ mb: 1, display: 'block' }}>
                  关联 Issues
                </Typography>
                {mr.related_issues.map((issue) => (
                  <Box
                    key={issue.iid}
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
                    onClick={() => navigate(`/projects/${projectId}/issues/${issue.iid}`)}
                  >
                    <Chip
                      label={issue.state === 'opened' ? '开启' : '关闭'}
                      size="small"
                      sx={{
                        fontSize: '10px',
                        height: 18,
                        backgroundColor: issue.state === 'opened' ? '#dbe1ff' : '#e1e2ed',
                        color: issue.state === 'opened' ? '#003ea8' : '#434655',
                      }}
                    />
                    <Typography variant="body2" sx={{ flex: 1 }} noWrap>
                      #{issue.iid} {issue.title}
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
