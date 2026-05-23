import { Box, Avatar, Chip, Typography, Link } from '@mui/material';
import type { KanbanIssue } from '../../types/kanban';

/** Surface colors from Architectural Logic design system */
const CARD_BG = '#f3f3fe'; // surface-container-low
const LABEL_COLORS: Record<string, { bg: string; color: string }> = {
  bug: { bg: '#ffdad6', color: '#93000a' },
  feature: { bg: '#dbe1ff', color: '#00174b' },
  enhancement: { bg: '#dbe1ff', color: '#00174b' },
  frontend: { bg: '#e7e7f3', color: '#31447b' },
  backend: { bg: '#e7e7f3', color: '#394c83' },
  'high-priority': { bg: '#ffdbd0', color: '#390c00' },
};

function getLabelStyle(label: string): { bg: string; color: string } {
  const lower = label.toLowerCase();
  return LABEL_COLORS[lower] || { bg: '#e7e7f3', color: '#434655' };
}

function formatRelativeTime(dateStr: string): string {
  const now = Date.now();
  const date = new Date(dateStr).getTime();
  const diff = now - date;
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days} 天前`;
  const months = Math.floor(days / 30);
  return `${months} 个月前`;
}

interface IssueCardProps {
  issue: KanbanIssue;
}

export default function IssueCard({ issue }: IssueCardProps) {
  // Filter out symphony-claimed label from display
  const displayLabels = issue.labels.filter(
    (l) => l !== 'symphony-claimed',
  );

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
          bgcolor: '#ededf9', // surface-container
        },
      }}
    >
      {/* Title */}
      <Link
        href={issue.web_url}
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
          #{issue.iid}
        </Typography>
        {issue.title}
      </Link>

      {/* Labels */}
      {displayLabels.length > 0 && (
        <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 0.5 }}>
          {displayLabels.slice(0, 4).map((label) => {
            const style = getLabelStyle(label);
            return (
              <Chip
                key={label}
                label={label}
                size="small"
                sx={{
                  height: 20,
                  fontSize: '11px',
                  fontWeight: 500,
                  bgcolor: style.bg,
                  color: style.color,
                  borderRadius: '4px',
                  '& .MuiChip-label': { px: 1 },
                }}
              />
            );
          })}
          {displayLabels.length > 4 && (
            <Chip
              label={`+${displayLabels.length - 4}`}
              size="small"
              sx={{
                height: 20,
                fontSize: '11px',
                fontWeight: 500,
                bgcolor: '#e7e7f3',
                color: '#434655',
                borderRadius: '4px',
                '& .MuiChip-label': { px: 1 },
              }}
            />
          )}
        </Box>
      )}

      {/* Footer: author + time */}
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          mt: 'auto',
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
          <Avatar
            src={issue.author.avatar_url || undefined}
            alt={issue.author.display_name || issue.author.username}
            sx={{ width: 20, height: 20, fontSize: '10px' }}
          >
            {(issue.author.display_name || issue.author.username).charAt(0).toUpperCase()}
          </Avatar>
          <Typography
            variant="body2"
            sx={{ color: '#434655', fontSize: '12px' }}
          >
            {issue.author.display_name || issue.author.username}
          </Typography>
        </Box>
        <Typography
          variant="body2"
          sx={{ color: '#737686', fontSize: '11px' }}
        >
          {formatRelativeTime(issue.created_at)}
        </Typography>
      </Box>
    </Box>
  );
}
