import { Box, Card, CardContent, Typography, Chip, IconButton, Tooltip } from '@mui/material';
import PeopleOutlineIcon from '@mui/icons-material/PeopleOutline';
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined';
import ViewKanbanOutlinedIcon from '@mui/icons-material/ViewKanbanOutlined';
import { useNavigate } from 'react-router-dom';
import ServiceStatusBadge from './ServiceStatusBadge';
import ServiceControlButton from './ServiceControlButton';
import type { Project } from '../../types';

interface ProjectCardProps {
  project: Project;
  onStart: (id: number) => Promise<void>;
  onStop: (id: number) => Promise<void>;
}

export default function ProjectCard({ project, onStart, onStop }: ProjectCardProps) {
  const navigate = useNavigate();

  const canControl = project.my_role === 'owner' || project.my_role === 'admin';

  return (
    <Card
      sx={{
        cursor: 'pointer',
        border: '1px solid #c3c6d7',
        transition: 'border-color 150ms ease',
        '&:hover': {
          borderColor: '#003ea8',
        },
      }}
      onClick={() => navigate(`/projects/${project.id}`)}
      role="article"
      aria-label={`项目 ${project.name}`}
    >
      <CardContent sx={{ p: 2.5, '&:last-child': { pb: 2.5 } }}>
        {/* Header: name + platform badge */}
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.5 }}>
          <Chip
            label={project.platform === 'gitlab' ? 'GitLab' : 'GitHub'}
            size="small"
            sx={{
              bgcolor: project.platform === 'gitlab' ? '#fc6d26' : '#24292f',
              color: '#ffffff',
              fontWeight: 500,
              fontSize: '11px',
              height: 20,
              borderRadius: '4px',
            }}
          />
          <Typography
            variant="subtitle1"
            sx={{
              flex: 1,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              color: '#191b23',
            }}
          >
            {project.name}
          </Typography>
        </Box>

        {/* Namespace/repo */}
        <Typography
          variant="body2"
          sx={{
            color: '#434655',
            mb: 1.5,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {project.namespace}/{project.repo_name}
        </Typography>

        {/* Description */}
        {project.description && (
          <Typography
            variant="body2"
            sx={{
              color: '#737686',
              mb: 1.5,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {project.description}
          </Typography>
        )}

        {/* Footer: status + members + controls */}
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            mt: 1,
          }}
        >
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5 }}>
            <ServiceStatusBadge status={project.service_status} />
            <Tooltip title="成员管理">
              <Box
                onClick={(e) => {
                  e.stopPropagation();
                  navigate(`/projects/${project.id}/members`);
                }}
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 0.5,
                  color: '#737686',
                  cursor: 'pointer',
                  borderRadius: '4px',
                  px: 0.5,
                  '&:hover': { color: '#003ea8' },
                }}
                role="link"
                aria-label="成员管理"
              >
                <PeopleOutlineIcon sx={{ fontSize: 16 }} />
                <Typography variant="body2">{project.member_count}</Typography>
              </Box>
            </Tooltip>
          </Box>

          <Box
            onClick={(e) => e.stopPropagation()}
            sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}
          >
            <Tooltip title="看板">
              <IconButton
                size="small"
                onClick={() => navigate(`/projects/${project.id}/kanban`)}
                sx={{ color: '#737686', '&:hover': { color: '#003ea8' } }}
                aria-label="看板"
              >
                <ViewKanbanOutlinedIcon sx={{ fontSize: 18 }} />
              </IconButton>
            </Tooltip>
            <Tooltip title="项目设置">
              <IconButton
                size="small"
                onClick={() => navigate(`/projects/${project.id}/settings`)}
                sx={{ color: '#737686', '&:hover': { color: '#003ea8' } }}
                aria-label="项目设置"
              >
                <SettingsOutlinedIcon sx={{ fontSize: 18 }} />
              </IconButton>
            </Tooltip>
            {canControl && (
              <ServiceControlButton
                status={project.service_status}
                onStart={() => onStart(project.id)}
                onStop={() => onStop(project.id)}
              />
            )}
          </Box>
        </Box>
      </CardContent>
    </Card>
  );
}
