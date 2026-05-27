import { createBrowserRouter, Navigate } from 'react-router-dom';
import AppLayout from './components/AppLayout';
import ProtectedRoute from './components/ProtectedRoute';
import Login from './pages/Login';
import AdminUsers from './pages/AdminUsers';
import AdminConcurrency from './pages/AdminConcurrency';
import AdminAlerts from './pages/AdminAlerts';
import AdminConfig from './pages/AdminConfig';
import Settings from './pages/Settings';
import ProjectListPage from './pages/projects/ProjectListPage';
import CreateProjectPage from './pages/projects/CreateProjectPage';
import ProjectSettingsPage from './pages/projects/ProjectSettingsPage';
import ProjectMembersPage from './pages/projects/ProjectMembersPage';
import KanbanPage from './pages/projects/KanbanPage';
import CreateIssuePage from './pages/projects/CreateIssuePage';
import IssueDetailPage from './pages/projects/IssueDetailPage';
import MrDetailPage from './pages/projects/MrDetailPage';
import OverviewKanbanPage from './pages/OverviewKanbanPage';

const router = createBrowserRouter([
  {
    path: '/login',
    element: <Login />,
  },
  {
    path: '/',
    element: (
      <ProtectedRoute>
        <AppLayout />
      </ProtectedRoute>
    ),
    children: [
      {
        index: true,
        element: <Navigate to="/overview" replace />,
      },
      {
        path: 'overview',
        element: <OverviewKanbanPage />,
      },
      {
        path: 'projects',
        element: <ProjectListPage />,
      },
      {
        path: 'projects/new',
        element: <CreateProjectPage />,
      },
      {
        path: 'projects/:id/settings',
        element: <ProjectSettingsPage />,
      },
      {
        path: 'projects/:id/members',
        element: <ProjectMembersPage />,
      },
      {
        path: 'projects/:id/kanban',
        element: <KanbanPage />,
      },
      {
        path: 'projects/:id/issues/create',
        element: <CreateIssuePage />,
      },
      {
        path: 'projects/:id/issues/:iid',
        element: <IssueDetailPage />,
      },
      {
        path: 'projects/:id/mrs/:iid',
        element: <MrDetailPage />,
      },
      {
        path: 'admin/users',
        element: (
          <ProtectedRoute requireAdmin>
            <AdminUsers />
          </ProtectedRoute>
        ),
      },
      {
        path: 'admin/concurrency',
        element: (
          <ProtectedRoute requireAdmin>
            <AdminConcurrency />
          </ProtectedRoute>
        ),
      },
      {
        path: 'admin/alerts',
        element: (
          <ProtectedRoute requireAdmin>
            <AdminAlerts />
          </ProtectedRoute>
        ),
      },
      {
        path: 'admin/config',
        element: (
          <ProtectedRoute requireAdmin>
            <AdminConfig />
          </ProtectedRoute>
        ),
      },
      {
        path: 'settings',
        element: <Settings />,
      },
    ],
  },
]);

export default router;
