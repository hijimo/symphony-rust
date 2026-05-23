import { Box, Typography, LinearProgress, Chip } from '@mui/material';
import type { ProjectConcurrencyInfo } from '../../api/concurrency';

interface Props {
  project: ProjectConcurrencyInfo;
}

export default function ProjectConcurrencyCard({ project }: Props) {
  const maxAgents = project.max_agents ?? 0;
  const utilization =
    maxAgents > 0 ? (project.active_agents / maxAgents) * 100 : 0;

  const statusColor =
    project.service_status === 'running'
      ? 'success'
      : project.service_status === 'error'
        ? 'error'
        : 'default';

  return (
    <Box
      sx={{
        p: 2,
        bgcolor: '#ffffff',
        borderRadius: 2,
        border: '1px solid #e0e0e8',
      }}
      data-testid="project-concurrency-card"
    >
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          mb: 1,
        }}
      >
        <Typography variant="subtitle2" sx={{ fontWeight: 500 }}>
          {project.project_name}
        </Typography>
        <Chip
          label={project.service_status}
          size="small"
          color={statusColor}
          variant="outlined"
        />
      </Box>

      <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, mb: 0.5 }}>
        <Typography variant="body2" color="text.secondary">
          Agent: {project.active_agents}
          {maxAgents > 0 ? ` / ${maxAgents}` : ''}
        </Typography>
        {project.queued_tasks > 0 && (
          <Typography variant="body2" color="text.secondary">
            排队: {project.queued_tasks}
          </Typography>
        )}
      </Box>

      {maxAgents > 0 && (
        <LinearProgress
          variant="determinate"
          value={Math.min(utilization, 100)}
          color={utilization >= 90 ? 'error' : 'primary'}
          sx={{ height: 4, borderRadius: 2 }}
        />
      )}
    </Box>
  );
}
