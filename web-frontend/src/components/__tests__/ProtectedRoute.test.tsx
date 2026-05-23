import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { describe, it, expect, beforeEach } from 'vitest';
import { ThemeProvider } from '@mui/material/styles';
import theme from '../../theme';
import ProtectedRoute from '../ProtectedRoute';
import { useAuthStore } from '../../store/auth';

function renderWithRoute(
  initialEntry: string,
  requireAdmin = false,
) {
  return render(
    <ThemeProvider theme={theme}>
      <MemoryRouter initialEntries={[initialEntry]}>
        <Routes>
          <Route
            path="/protected"
            element={
              <ProtectedRoute requireAdmin={requireAdmin}>
                <div>Protected Content</div>
              </ProtectedRoute>
            }
          />
          <Route path="/login" element={<div>Login Page</div>} />
          <Route path="/settings" element={<div>Settings Page</div>} />
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe('ProtectedRoute', () => {
  beforeEach(() => {
    useAuthStore.setState({
      token: null,
      user: null,
      isAuthenticated: false,
    });
  });

  it('redirects to /login when not authenticated', () => {
    renderWithRoute('/protected');
    expect(screen.getByText('Login Page')).toBeInTheDocument();
    expect(screen.queryByText('Protected Content')).not.toBeInTheDocument();
  });

  it('renders children when authenticated', () => {
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' },
      isAuthenticated: true,
    });
    renderWithRoute('/protected');
    expect(screen.getByText('Protected Content')).toBeInTheDocument();
  });

  it('redirects non-admin from admin routes', () => {
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 2, username: 'john', displayName: 'John', role: 'user' },
      isAuthenticated: true,
    });
    renderWithRoute('/protected', true);
    expect(screen.getByText('Settings Page')).toBeInTheDocument();
    expect(screen.queryByText('Protected Content')).not.toBeInTheDocument();
  });

  it('allows admin to access admin routes', () => {
    useAuthStore.setState({
      token: 'mock-token',
      user: { id: 1, username: 'admin', displayName: 'Admin', role: 'admin' },
      isAuthenticated: true,
    });
    renderWithRoute('/protected', true);
    expect(screen.getByText('Protected Content')).toBeInTheDocument();
  });
});
