import { useState } from 'react';
import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  TextField,
  Alert,
} from '@mui/material';
import { useConcurrencyStore } from '../../store/concurrencyStore';

interface Props {
  open: boolean;
  onClose: () => void;
  currentMax: number;
}

export default function ConcurrencyConfigDialog({
  open,
  onClose,
  currentMax,
}: Props) {
  const [value, setValue] = useState(String(currentMax));
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const { updateConfig } = useConcurrencyStore();

  const handleSave = async () => {
    const num = parseInt(value, 10);
    if (isNaN(num) || num < 1 || num > 100) {
      setError('请输入 1-100 之间的数字');
      return;
    }

    setSaving(true);
    setError('');
    try {
      await updateConfig({ globalMax: num, expectedPrevious: currentMax });
      onClose();
    } catch (err: unknown) {
      if (err && typeof err === 'object' && 'response' in err) {
        const resp = (err as { response?: { data?: { retCode?: string } } })
          .response;
        if (resp?.data?.retCode === 'BIZ_003') {
          setError('配置已被其他管理员修改，请刷新后重试');
        } else {
          setError('保存失败');
        }
      } else {
        setError('保存失败');
      }
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>并行控制配置</DialogTitle>
      <DialogContent>
        {error && (
          <Alert severity="error" sx={{ mb: 2 }}>
            {error}
          </Alert>
        )}
        <TextField
          label="全局最大并行数"
          type="number"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          fullWidth
          variant="filled"
          sx={{ mt: 1 }}
          slotProps={{ htmlInput: { min: 1, max: 100 } }}
          aria-label="全局最大并行数"
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} disabled={saving}>
          取消
        </Button>
        <Button onClick={handleSave} variant="contained" disabled={saving}>
          保存
        </Button>
      </DialogActions>
    </Dialog>
  );
}
