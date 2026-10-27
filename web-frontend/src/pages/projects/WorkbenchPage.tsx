import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Alert, Box, Button, CircularProgress, Snackbar, Typography } from '@mui/material';
import ArrowBackIcon from '@mui/icons-material/ArrowBack';
import { Group, Panel, Separator } from 'react-resizable-panels';
import FileTree from '../../components/workbench/FileTree';
import EditorPane from '../../components/workbench/EditorPane';
import AiPanel from '../../components/workbench/AiPanel';
import FlowGatePanel from '../../components/workbench/FlowGatePanel';
import { useWorkbenchStore, type SelectionRef } from '../../store/workbenchStore';

const handle = { width: 4, background: '#e7e7f3', cursor: 'col-resize' } as const;

export default function WorkbenchPage() {
  const { id, iid } = useParams<{ id: string; iid: string }>();
  const projectId = Number(id);
  const issueIid = Number(iid);
  const navigate = useNavigate();
  const { init, dispose, loading, error, flow, notices, dismissNotice } = useWorkbenchStore();
  const [selection, setSelection] = useState<SelectionRef | null>(null);

  useEffect(() => {
    if (projectId && issueIid) void init(projectId, issueIid);
    return () => dispose();
  }, [projectId, issueIid, init, dispose]);

  const notice = notices[notices.length - 1];

  return (
    <Box sx={{ height: 'calc(100vh - 72px)', display: 'flex', flexDirection: 'column' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1 }}>
        <Button size="small" startIcon={<ArrowBackIcon />} onClick={() => navigate(`/projects/${projectId}/kanban`)}>
          看板
        </Button>
        <Typography variant="h6" sx={{ fontSize: 16 }}>需求文档工作台</Typography>
        {loading && <CircularProgress size={16} />}
      </Box>
      {error && <Alert severity="error">{error}</Alert>}
      {!error && flow && (
        <Box sx={{ flex: 1, minHeight: 0, border: '1px solid #e7e7f3', borderRadius: 1, overflow: 'hidden' }}>
          <Group orientation="horizontal">
            <Panel defaultSize="20%" minSize="12%">
              <FileTree />
            </Panel>
            <Separator style={handle} />
            <Panel defaultSize="52%" minSize="30%">
              <EditorPane onAskAi={setSelection} />
            </Panel>
            <Separator style={handle} />
            <Panel defaultSize="28%" minSize="18%">
              <Group orientation="vertical">
                <Panel defaultSize="38%" minSize="20%">
                  <FlowGatePanel />
                </Panel>
                <Separator style={{ height: 4, background: '#e7e7f3', cursor: 'row-resize' }} />
                <Panel defaultSize="62%" minSize="30%">
                  <AiPanel selection={selection} onClearSelection={() => setSelection(null)} />
                </Panel>
              </Group>
            </Panel>
          </Group>
        </Box>
      )}
      <Snackbar open={Boolean(notice)} autoHideDuration={5000} onClose={() => notice && dismissNotice(notice.id)}>
        {notice ? (
          <Alert severity={notice.level as 'info' | 'warning' | 'error'} onClose={() => dismissNotice(notice.id)}>
            {notice.message}
          </Alert>
        ) : (
          <span />
        )}
      </Snackbar>
    </Box>
  );
}
