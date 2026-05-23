import { Box, Avatar, Chip, Typography, Link } from '@mui/material';
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline';
import HighlightOffIcon from '@mui/icons-material/HighlightOff';
import HourglassEmptyIcon from '@mui/icons-material/HourglassEmpty';
import type { KanbanMergeRequest, CiStatus, ReviewStatus } from '../../types/kanban';

const CARD_BG = '#f3f3fe'; // surface-container-low

function getCiStatusDot(status: CiStatus | null): { color: string; label: string } {
  switch (status) {
    case 'success':
      return { color: '#2e7d32', label: 'CI 通过' };
    case 'failed':
      return { color: '#ba1a1a', label: 'CI 失败' };
    case 'running':
      return { color: '#ed6c02', label: 'CI 运行中' };
    case 'pending':
      return { color: '#ed6c02', label: 'CI 等待中' };
    case 'canceled':
      return { color: '#737686', label: 'CI 已取消' };
    default:
      return { color: '#c3c6d7', label: '无 CI' };
  }
}

function getReviewIcon(status: ReviewStatus | null) {
  switch (status) {
    case 'approved':
      return <CheckCircleOutlineIcon sx={{ fontSize: 16, color: '#2e7d32' }} />;
    case 'changes_requested':
      return <HighlightOffIcon sx={{ fontSize: 16, color: '#ba1a1a' }} />;
    case 'pending':
      return <HourglassEmptyIcon sx={{ fontSize: 16, color: '#ed6c02' }} />;
    default:
      return null;
  }
}

function getReviewLabel(status: ReviewStatus | null): string {
  switch (status) {
    case 'approved':
      return '已批准';
    case 'changes_requested':
      return '需修改';
    case 'pending':
      return '待审核';
    default:
      return '';
  }
}

interface PrCardProps {
  mr: KanbanMergeRequest;
}

export default function PrCard({ mr }: PrCardProps) {
  const ciDot = getCiStatusDot(mr.ci_status);
  const reviewIcon = getReviewIcon(mr.review_status);

  return (
    <Box
      sx={{
        bgcolor: CARD_BG,
        borderRadius: '8px',
        p: 2,
        display: 'flex',
        flexDirection: 'column',
        gap: 1,
        transition: 'background-color 150ms',
        '&:hover': {
          bgcolor: '#ededf9',
        },
      }}
    >
      {/* Title */}
      <Link
        href={mr.web_url}
        target="_blank"
        rel="noopener noreferrer"
        underline="hover"
        sx={{
          color: '#191b23',
          fontSize: '14px',
          fontWeight: 500,
          lineHeight: '20px',
          display: '-webkit-box',
          WebkitLineClamp: 2,
          WebkitBoxOrient: 'vertical',
          overflow: 'hidden',
        }}
      >
        <Typography
          component="span"
          sx={{ color: '#737686', fontWeight: 400, fontSize: '12px', mr: 0.5 }}
        >
          !{mr.iid}
        </Typography>
        {mr.title}
      </Link>

      {/* Branch info */}
      <Typography
        variant="body2"
        sx={{
          color: '#737686',
          fontSize: '11px',
          fontFamily: 'monospace',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {mr.source_branch} → {mr.target_branch}
      </Typography>

      {/* Status row: CI + Review */}
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
        {/* CI status dot */}
        <Box
          sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}
          title={ciDot.label}
        >
          <Box
            sx={{
              width: 8,
              height: 8,
              borderRadius: '50%',
              bgcolor: ciDot.color,
              flexShrink: 0,
            }}
          />
          <Typography sx={{ fontSize: '11px', color: '#434655' }}>
            {ciDot.label}
          </Typography>
        </Box>

        {/* Review status */}
        {reviewIcon && (
          <Box
            sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}
            title={getReviewLabel(mr.review_status)}
          >
            {reviewIcon}
            <Typography sx={{ fontSize: '11px', color: '#434655' }}>
              {getReviewLabel(mr.review_status)}
            </Typography>
          </Box>
        )}
      </Box>

      {/* Related issues */}
      {mr.related_issue_iids.length > 0 && (
        <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 0.5 }}>
          {mr.related_issue_iids.map((iid) => (
            <Chip
              key={iid}
              label={`#${iid}`}
              size="small"
              sx={{
                height: 18,
                fontSize: '11px',
                fontWeight: 500,
                bgcolor: '#dbe1ff',
                color: '#003ea8',
                borderRadius: '4px',
                '& .MuiChip-label': { px: 0.75 },
              }}
            />
          ))}
        </Box>
      )}

      {/* Footer: author */}
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 0.5,
          mt: 'auto',
        }}
      >
        <Avatar
          src={mr.author.avatar_url || undefined}
          alt={mr.author.display_name || mr.author.username}
          sx={{ width: 20, height: 20, fontSize: '10px' }}
        >
          {(mr.author.display_name || mr.author.username).charAt(0).toUpperCase()}
        </Avatar>
        <Typography
          variant="body2"
          sx={{ color: '#434655', fontSize: '12px' }}
        >
          {mr.author.display_name || mr.author.username}
        </Typography>
      </Box>
    </Box>
  );
}
