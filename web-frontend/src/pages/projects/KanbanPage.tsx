import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import {
  Box,
  Typography,
  Button,
  IconButton,
  Tooltip,
  Alert,
  Chip,
} from '@mui/material';
import { Refresh, Add } from '@mui/icons-material';
import KanbanBoard from '../../components/kanban/KanbanBoard';
import KanbanSkeleton from '../../components/kanban/KanbanSkeleton';
import AuthorFilter from '../../components/kanban/AuthorFilter';
import { useKanbanStore } from '../../store/kanbanStore';
import type { PlatformUser } from '../../types/kanban';

export default function KanbanPage() {
  const { id } = useParams<{ id: string }>();
  const projectId = Number(id);
  const navigate = useNavigate();

  const { kanbanData, loading, error, filters, fetchKanban, refresh, setFilters, clearError } =
    useKanbanStore();

  const [searchInput, setSearchInput] = useState('');
  const [labelsInput, setLabelsInput] = useState('');
  const [assigneeFilter, setAssigneeFilter] = useState('');
  const initialFetchDone = useRef(false);

  // Fetch on mount
  useEffect(() => {
    if (projectId && !initialFetchDone.current) {
      initialFetchDone.current = true;
      fetchKanban(projectId);
    }
  }, [projectId, fetchKanban]);

  // Debounced search
  useEffect(() => {
    if (!initialFetchDone.current) return;
    const timer = setTimeout(() => {
      setFilters({ search: searchInput || undefined });
    }, 400);
    return () => clearTimeout(timer);
  }, [searchInput, setFilters]);

  // Debounced labels
  useEffect(() => {
    if (!initialFetchDone.current) return;
    const timer = setTimeout(() => {
      setFilters({ labels: labelsInput || undefined });
    }, 400);
    return () => clearTimeout(timer);
  }, [labelsInput, setFilters]);

  // Assignee filter (immediate)
  useEffect(() => {
    if (!initialFetchDone.current) return;
    setFilters({ assignee: assigneeFilter || undefined });
  }, [assigneeFilter, setFilters]);

  // Re-fetch when filters change (skip initial)
  const filtersKey = JSON.stringify(filters);
  const prevFiltersKey = useRef(filtersKey);
  useEffect(() => {
    if (prevFiltersKey.current === filtersKey) return;
    prevFiltersKey.current = filtersKey;
    if (projectId) {
      fetchKanban(projectId);
    }
  }, [filtersKey, projectId, fetchKanban]);

  const handleRefresh = useCallback(() => {
    if (projectId) {
      refresh(projectId);
    }
  }, [projectId, refresh]);

  // Collect unique authors from all columns for the filter dropdown
  const allAuthors = useMemo((): PlatformUser[] => {
    if (!kanbanData) return [];
    const map = new Map<string, PlatformUser>();
    const addUser = (u: PlatformUser) => {
      if (!map.has(u.username)) map.set(u.username, u);
    };
    kanbanData.todo.issues.forEach((i) => {
      addUser(i.author);
      i.assignees.forEach(addUser);
    });
    kanbanData.in_progress.issues.forEach((i) => {
      addUser(i.author);
      i.assignees.forEach(addUser);
    });
    kanbanData.pr.merge_requests.forEach((mr) => addUser(mr.author));
    return Array.from(map.values());
  }, [kanbanData]);

  return (
    <Box>
      {/* Header */}
      <Box
        sx={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          mb: 2,
        }}
      >
        <Typography variant="h5" color="text.primary">
          看板
        </Typography>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <Button
            size="small"
            variant="contained"
            startIcon={<Add />}
            onClick={() => navigate(`/projects/${id}/issues/create`)}
            sx={{
              textTransform: 'none',
              borderRadius: '4px',
              fontSize: '13px',
              fontWeight: 500,
              background: 'linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%)',
              '&:hover': {
                background: 'linear-gradient(135deg, #4f46e5 0%, #7c3aed 100%)',
              },
            }}
          >
            创建 Issue
          </Button>
          {/* Cached indicator */}
          {kanbanData?.cached && (
            <Chip
              label="缓存"
              size="small"
              sx={{
                height: 22,
                fontSize: '11px',
                fontWeight: 500,
                bgcolor: '#e7e7f3',
                color: '#737686',
                borderRadius: '4px',
              }}
            />
          )}
          <Tooltip title="刷新 (绕过缓存)">
            <IconButton onClick={handleRefresh} disabled={loading} aria-label="刷新看板">
              <Refresh />
            </IconButton>
          </Tooltip>
        </Box>
      </Box>

      {/* Filter bar */}
      <Box sx={{ mb: 2 }}>
        <AuthorFilter
          authors={allAuthors}
          value={assigneeFilter}
          onChange={setAssigneeFilter}
          searchValue={searchInput}
          onSearchChange={setSearchInput}
          labelsValue={labelsInput}
          onLabelsChange={setLabelsInput}
        />
      </Box>

      {/* Error state */}
      {error && (
        <Alert
          severity="error"
          sx={{ mb: 2, borderRadius: '8px' }}
          action={
            <Button
              color="inherit"
              size="small"
              onClick={() => {
                clearError();
                fetchKanban(projectId);
              }}
            >
              重试
            </Button>
          }
        >
          {error}
        </Alert>
      )}

      {/* Content */}
      {loading && !kanbanData ? (
        <KanbanSkeleton />
      ) : kanbanData ? (
        <KanbanBoard data={kanbanData} />
      ) : null}
    </Box>
  );
}
