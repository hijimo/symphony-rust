import { useState } from 'react';
import { TextField, IconButton, InputAdornment } from '@mui/material';
import { Visibility, VisibilityOff } from '@mui/icons-material';
import type { TextFieldProps } from '@mui/material';

type PasswordFieldProps = Omit<TextFieldProps, 'type'> & {
  showToggle?: boolean;
};

export default function PasswordField({ showToggle = true, ...props }: PasswordFieldProps) {
  const [visible, setVisible] = useState(false);

  return (
    <TextField
      {...props}
      type={visible ? 'text' : 'password'}
      autoComplete="new-password"
      slotProps={{
        input: {
          endAdornment: showToggle ? (
            <InputAdornment position="end">
              <IconButton
                aria-label={visible ? '隐藏密码' : '显示密码'}
                onClick={() => setVisible((v) => !v)}
                edge="end"
                size="small"
              >
                {visible ? <VisibilityOff /> : <Visibility />}
              </IconButton>
            </InputAdornment>
          ) : undefined,
        },
      }}
    />
  );
}
