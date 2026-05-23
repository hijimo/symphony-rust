import { useLocation, useNavigate } from 'react-router-dom';
import {
  Drawer,
  List,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  Toolbar,
  Divider,
  Typography,
  Box,
  Tooltip,
  useMediaQuery,
  useTheme,
} from '@mui/material';
import PeopleOutlineIcon from '@mui/icons-material/PeopleOutline';
import TuneIcon from '@mui/icons-material/Tune';
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined';
import FolderOutlinedIcon from '@mui/icons-material/FolderOutlined';
import SpeedIcon from '@mui/icons-material/Speed';
import NotificationsOutlinedIcon from '@mui/icons-material/NotificationsOutlined';
import { useAuthStore } from '../store/auth';

const DRAWER_WIDTH = 256;
const DRAWER_COLLAPSED_WIDTH = 64;

/** Light cool-toned sidebar — surface-container-low from Architectural Logic */
const SIDEBAR_BG = '#f3f3fe';
/** Active item background — surface-container-high */
const SIDEBAR_ACTIVE_BG = '#e7e7f3';
/** Hover background — surface-container-high */
const SIDEBAR_HOVER_BG = '#e7e7f3';
/** Active indicator & icon color — primary */
const SIDEBAR_PRIMARY = '#003ea8';
/** Inactive text & icon color — on-surface-variant */
const SIDEBAR_TEXT_INACTIVE = '#434655';
/** Active text color — on-surface */
const SIDEBAR_TEXT_ACTIVE = '#191b23';
/** Group title color — outline */
const SIDEBAR_GROUP_TITLE = '#737686';
/** Divider color — outline-variant */
const SIDEBAR_DIVIDER = '#c3c6d7';

interface MenuItem {
  path: string;
  label: string;
  icon: React.ReactNode;
}

interface MenuGroup {
  group: string;
  roles: string[];
  items: MenuItem[];
}

const menuGroups: MenuGroup[] = [
  {
    group: '项目',
    roles: ['admin', 'user'],
    items: [
      { path: '/projects', label: '项目列表', icon: <FolderOutlinedIcon /> },
    ],
  },
  {
    group: '管理',
    roles: ['admin'],
    items: [
      { path: '/admin/users', label: '用户管理', icon: <PeopleOutlineIcon /> },
      {
        path: '/admin/concurrency',
        label: '并行控制',
        icon: <SpeedIcon />,
      },
      {
        path: '/admin/alerts',
        label: '告警管理',
        icon: <NotificationsOutlinedIcon />,
      },
      { path: '/admin/config', label: '系统配置', icon: <TuneIcon /> },
    ],
  },
  {
    group: '个人',
    roles: ['admin', 'user'],
    items: [
      { path: '/settings', label: '个人设置', icon: <SettingsOutlinedIcon /> },
    ],
  },
];

interface SidebarProps {
  mobileOpen: boolean;
  onMobileClose: () => void;
}

export default function Sidebar({ mobileOpen, onMobileClose }: SidebarProps) {
  const theme = useTheme();
  const isMobile = useMediaQuery(theme.breakpoints.down('md'));
  const isTablet = useMediaQuery(theme.breakpoints.between('md', 'lg'));
  const location = useLocation();
  const navigate = useNavigate();
  const { user } = useAuthStore();

  const collapsed = isTablet && !isMobile;
  const drawerWidth = collapsed ? DRAWER_COLLAPSED_WIDTH : DRAWER_WIDTH;

  const isActive = (path: string) => location.pathname.startsWith(path);

  const handleNavigate = (path: string) => {
    navigate(path);
    if (isMobile) onMobileClose();
  };

  const drawerContent = (
    <Box sx={{ overflow: 'auto', height: '100%', bgcolor: SIDEBAR_BG }}>
      <Toolbar sx={{ height: 48, minHeight: '48px !important' }} />
      {menuGroups
        .filter((g) => user && g.roles.includes(user.role))
        .map((group, gi) => (
          <Box key={group.group}>
            {gi > 0 && (
              <Divider sx={{ my: 1, borderColor: SIDEBAR_DIVIDER }} />
            )}
            {!collapsed && (
              <Typography
                sx={{
                  px: 2,
                  pt: 2,
                  pb: 0.5,
                  display: 'block',
                  color: SIDEBAR_GROUP_TITLE,
                  fontSize: '12px',
                  fontWeight: 500,
                  textTransform: 'uppercase',
                  letterSpacing: '0.02em',
                  lineHeight: '16px',
                }}
              >
                {group.group}
              </Typography>
            )}
            <List disablePadding>
              {group.items.map((item) => {
                const active = isActive(item.path);
                const button = (
                  <ListItemButton
                    key={item.path}
                    onClick={() => handleNavigate(item.path)}
                    aria-current={active ? 'page' : undefined}
                    sx={{
                      height: 40,
                      px: 2,
                      borderRadius: 0,
                      bgcolor: active ? SIDEBAR_ACTIVE_BG : 'transparent',
                      '&:hover': { bgcolor: SIDEBAR_HOVER_BG },
                      justifyContent: collapsed ? 'center' : 'flex-start',
                      borderLeft: active
                        ? `4px solid ${SIDEBAR_PRIMARY}`
                        : '4px solid transparent',
                      transition: theme.transitions.create(
                        ['background-color', 'border-color'],
                        { duration: 150 }
                      ),
                    }}
                  >
                    <ListItemIcon
                      sx={{
                        minWidth: collapsed ? 0 : 40,
                        color: active
                          ? SIDEBAR_PRIMARY
                          : SIDEBAR_TEXT_INACTIVE,
                      }}
                    >
                      {item.icon}
                    </ListItemIcon>
                    {!collapsed && (
                      <ListItemText
                        primary={item.label}
                        primaryTypographyProps={{
                          fontSize: '14px',
                          fontWeight: active ? 500 : 400,
                          color: active
                            ? SIDEBAR_TEXT_ACTIVE
                            : SIDEBAR_TEXT_INACTIVE,
                        }}
                      />
                    )}
                  </ListItemButton>
                );

                return collapsed ? (
                  <Tooltip key={item.path} title={item.label} placement="right">
                    {button}
                  </Tooltip>
                ) : (
                  button
                );
              })}
            </List>
          </Box>
        ))}
    </Box>
  );

  if (isMobile) {
    return (
      <Drawer
        variant="temporary"
        open={mobileOpen}
        onClose={onMobileClose}
        ModalProps={{ keepMounted: true }}
        sx={{
          '& .MuiDrawer-paper': {
            width: DRAWER_WIDTH,
            boxSizing: 'border-box',
            bgcolor: SIDEBAR_BG,
            borderRight: 'none',
          },
        }}
        aria-label="主导航"
      >
        {drawerContent}
      </Drawer>
    );
  }

  return (
    <Drawer
      variant="permanent"
      sx={{
        width: drawerWidth,
        flexShrink: 0,
        '& .MuiDrawer-paper': {
          width: drawerWidth,
          boxSizing: 'border-box',
          bgcolor: SIDEBAR_BG,
          borderRight: 'none',
          transition: theme.transitions.create('width', {
            easing: theme.transitions.easing.sharp,
            duration: 225,
          }),
          overflowX: 'hidden',
        },
      }}
      aria-label="主导航"
    >
      {drawerContent}
    </Drawer>
  );
}

export { DRAWER_WIDTH, DRAWER_COLLAPSED_WIDTH };
