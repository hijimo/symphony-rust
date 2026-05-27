import { useEffect, useRef, useCallback } from 'react';
import { Box, Typography, Alert, Button, Chip, Skeleton } from '@mui/material';
import { Refresh } from '@mui/icons-material';
import { useOverviewKanbanStore } from '../store/overviewKanbanStore';
import ProjectKanbanSection from '../components/overview/ProjectKanbanSection';
import OverviewEmptyState from '../components/overview/OverviewEmptyState';

const AUTO_REFRESH_BASE_MS = 30_000;
const JITTER_MS = 3_000;

export default function OverviewKanbanPage() {
  const {
    projectMetas,
    projectIssues,
    projectPrs,
    issuesLoading,
    prsLoading,
    issuesError,
    prsError,
    totalRunningProjects,
    hasMore,
    fetchIssues,
    fetchPrs,
    reset,
  } = useOverviewKanbanStore();

  const abortRef = useRef<AbortController | null>(null);
  const intervalRef = useRef<number | null>(null);

  const doRefresh = useCallback(() => {
    const { issuesLoading, prsLoading } = useOverviewKanbanStore.getState();
    if (issuesLoading || prsLoading) return;

    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;

    fetchIssues(controller.signal);
    fetchPrs(controller.signal);
  }, [fetchIssues, fetchPrs]);

  // Initial fetch + auto-refresh
  useEffect(() => {
    doRefresh();

    const jitter = Math.random() * JITTER_MS * 2 - JITTER_MS;
    intervalRef.current = window.setInterval(doRefresh, AUTO_REFRESH_BASE_MS + jitter);

    return () => {
      if (intervalRef.current) window.clearInterval(intervalRef.current);
      abortRef.current?.abort();
      reset();
    };
  }, [doRefresh, reset]);

  // Page Visibility API
  useEffect(() => {
    const handleVisibility = () => {
      if (document.visibilityState === 'visible') {
        doRefresh();
      }
    };
    document.addEventListener('visibilitychange', handleVisibility);
    return () => document.removeEventListener('visibilitychange', handleVisibility);
  }, [doRefresh]);

  const handleManualRefresh = () => {
    doRefresh();
  };

  const isInitialLoad = issuesLoading && projectMetas.length === 0;
  const showEmpty = !issuesLoading && !issuesError && projectMetas.length === 0;

  return (
    <Box>
      {/* Header */}
      <Box
        sx={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          mb: 3,
        }}
      >
        <Box>
          <Typography variant="h5" sx={{ fontWeight: 600, color: '#191b23' }}>
            总览
          </Typography>
          <Typography variant="body2" sx={{ color: '#737686', mt: 0.25 }}>
            运行中项目
            {totalRunningProjects > 0 && (
              <Chip
                label={totalRunningProjects}
                size="small"
                sx={{
                  ml: 1,
                  height: 18,
                  fontSize: '11px',
                  fontWeight: 600,
                  bgcolor: '#e7e7f3',
                  color: '#434655',
                  borderRadius: '4px',
                }}
              />
            )}
          </Typography>
        </Box>
        <Button
          size="small"
          startIcon={<Refresh />}
          onClick={handleManualRefresh}
          disabled={issuesLoading && prsLoading}
          sx={{
            textTransform: 'none',
            fontSize: '13px',
            color: '#434655',
            fontWeight: 500,
            '&:hover': { bgcolor: '#e7e7f3' },
          }}
        >
          刷新
        </Button>
      </Box>

      {/* Issues error */}
      {issuesError && (
        <Alert
          severity="error"
          sx={{ mb: 2, borderRadius: '8px' }}
          action={
            <Button color="inherit" size="small" onClick={handleManualRefresh}>
              重试
            </Button>
          }
        >
          {issuesError}
        </Alert>
      )}

      {/* PRs error (inline, non-blocking) */}
      {prsError && !issuesError && (
        <Alert
          severity="warning"
          sx={{ mb: 2, borderRadius: '8px', fontSize: '13px' }}
          action={
            <Button color="inherit" size="small" onClick={handleManualRefresh}>
              重试
            </Button>
          }
        >
          PR/MR 数据加载失败
        </Alert>
      )}

      {/* Loading skeleton */}
      {isInitialLoad && (
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {[1, 2, 3].map((i) => (
            <Skeleton
              key={i}
              variant="rounded"
              height={200}
              sx={{ borderRadius: '8px' }}
            />
          ))}
        </Box>
      )}

      {/* Empty state */}
      {showEmpty && <OverviewEmptyState />}

      {/* Project sections */}
      {projectMetas.length > 0 && (
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {projectMetas.slice(0, 8).map((meta) => (
            <ProjectKanbanSection
              key={meta.project_id}
              meta={meta}
              issuesData={projectIssues.get(meta.project_id)}
              prsData={projectPrs.get(meta.project_id)}
              prsLoading={prsLoading}
            />
          ))}

          {/* "More projects" footer */}
          {(hasMore || projectMetas.length > 8) && (
            <Box
              sx={{
                border: '1px dashed #c3c6d7',
                borderRadius: '8px',
                p: 2,
                textAlign: 'center',
              }}
            >
              <Typography variant="body2" sx={{ color: '#737686', fontSize: '13px' }}>
                还有 {totalRunningProjects - Math.min(projectMetas.length, 8)} 个运行中项目
              </Typography>
            </Box>
          )}
        </Box>
      )}
    </Box>
  );
}
