import { useState } from 'react';
import { Outlet } from 'react-router-dom';
import { Box, Toolbar } from '@mui/material';
import TopNav from './TopNav';
import Sidebar from './Sidebar';

export default function AppLayout() {
  const [mobileOpen, setMobileOpen] = useState(false);

  return (
    <Box sx={{ display: 'flex', minHeight: '100vh' }}>
      <TopNav onMenuClick={() => setMobileOpen(true)} />
      <Sidebar mobileOpen={mobileOpen} onMobileClose={() => setMobileOpen(false)} />
      <Box
        component="main"
        sx={{
          flexGrow: 1,
          bgcolor: '#faf8ff',
          minHeight: '100vh',
        }}
      >
        <Toolbar sx={{ height: 48, minHeight: '48px !important' }} />
        <Box sx={{ p: '12px' }}>
          <Outlet />
        </Box>
      </Box>
    </Box>
  );
}
