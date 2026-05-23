import { createTheme } from '@mui/material/styles';

const theme = createTheme({
  palette: {
    primary: {
      main: '#003ea8',
      light: '#0053db',
      dark: '#0048c1',
      contrastText: '#ffffff',
    },
    secondary: {
      main: '#495c94',
      light: '#acbffe',
      dark: '#31447b',
      contrastText: '#ffffff',
    },
    error: {
      main: '#ba1a1a',
      light: '#ffdad6',
      dark: '#93000a',
      contrastText: '#ffffff',
    },
    background: {
      default: '#faf8ff',
      paper: '#ffffff',
    },
    text: {
      primary: '#191b23',
      secondary: '#434655',
    },
    divider: '#c3c6d7',
    action: {
      hover: 'rgba(0, 62, 168, 0.04)',
    },
  },
  typography: {
    fontFamily: '"Inter", sans-serif',
    button: {
      textTransform: 'none',
      fontWeight: 500,
      fontSize: '14px',
    },
    h4: {
      fontSize: '24px',
      fontWeight: 600,
      lineHeight: '30px',
      letterSpacing: '-0.02em',
    },
    h5: {
      fontSize: '22px',
      fontWeight: 600,
      lineHeight: '28px',
      letterSpacing: '-0.01em',
    },
    h6: {
      fontSize: '20px',
      fontWeight: 600,
      lineHeight: '26px',
    },
    subtitle1: {
      fontSize: '16px',
      fontWeight: 500,
      lineHeight: '22px',
    },
    subtitle2: {
      fontSize: '14px',
      fontWeight: 500,
      lineHeight: '24px',
    },
    body1: {
      fontSize: '14px',
      fontWeight: 400,
      lineHeight: '18px',
    },
    body2: {
      fontSize: '12px',
      fontWeight: 400,
      lineHeight: '18px',
    },
    caption: {
      fontSize: '11px',
      fontWeight: 500,
      lineHeight: '16px',
      letterSpacing: '0.03em',
    },
    overline: {
      fontSize: '12px',
      fontWeight: 500,
      lineHeight: '16px',
      letterSpacing: '0.02em',
    },
  },
  shape: {
    borderRadius: 4,
  },
  shadows: [
    'none',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
    '0 4px 16px rgba(42, 52, 57, 0.08)',
  ] as any,
  components: {
    MuiButton: {
      styleOverrides: {
        root: {
          borderRadius: '4px',
          padding: '8px 16px',
          fontWeight: 500,
          fontSize: '14px',
        },
        contained: {
          background: 'linear-gradient(135deg, #0053db 0%, #0048c1 100%)',
          color: '#ffffff',
          boxShadow: 'none',
          '&:hover': {
            background: 'linear-gradient(135deg, #0048c1 0%, #003ea8 100%)',
            boxShadow: 'none',
          },
        },
        containedPrimary: {
          background: 'linear-gradient(135deg, #0053db 0%, #0048c1 100%)',
          color: '#ffffff',
          '&:hover': {
            background: 'linear-gradient(135deg, #0048c1 0%, #003ea8 100%)',
          },
        },
        outlined: {
          borderColor: '#c3c6d7',
        },
      },
    },
    MuiTextField: {
      defaultProps: {
        variant: 'filled',
        size: 'medium',
      },
      styleOverrides: {
        root: {
          '& .MuiFilledInput-root': {
            backgroundColor: '#f3f3fe',
            borderRadius: '4px',
            '&:before': {
              borderBottom: 'none',
            },
            '&:after': {
              borderBottomColor: '#0053db',
              borderBottomWidth: '2px',
            },
            '&:hover:not(.Mui-disabled):before': {
              borderBottom: 'none',
            },
            '&.Mui-focused': {
              backgroundColor: '#f3f3fe',
            },
          },
          '& .MuiInputLabel-root': {
            fontSize: '12px',
            fontWeight: 500,
            letterSpacing: '0.02em',
          },
        },
      },
    },
    MuiCard: {
      styleOverrides: {
        root: {
          borderRadius: '8px',
          boxShadow: 'none',
          border: 'none',
        },
      },
    },
    MuiDialog: {
      styleOverrides: {
        paper: {
          borderRadius: '8px',
          boxShadow: '0 4px 16px rgba(42, 52, 57, 0.08)',
        },
      },
    },
    MuiChip: {
      styleOverrides: {
        root: {
          borderRadius: '4px',
          fontWeight: 500,
          fontSize: '12px',
        },
      },
    },
    MuiTableRow: {
      styleOverrides: {
        root: {
          height: '40px',
        },
      },
    },
    MuiPaper: {
      styleOverrides: {
        root: {
          backgroundImage: 'none',
        },
      },
    },
  },
});

export default theme;
