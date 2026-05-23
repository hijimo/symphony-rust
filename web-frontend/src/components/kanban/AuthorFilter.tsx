import { Box, TextField, InputAdornment, Autocomplete } from '@mui/material';
import { Search } from '@mui/icons-material';
import type { PlatformUser } from '../../types/kanban';

interface AuthorFilterProps {
  /** All unique authors from kanban data */
  authors: PlatformUser[];
  /** Currently selected author username */
  value: string;
  onChange: (username: string) => void;
  /** Search text filter */
  searchValue: string;
  onSearchChange: (value: string) => void;
  /** Labels filter text */
  labelsValue: string;
  onLabelsChange: (value: string) => void;
}

export default function AuthorFilter({
  authors,
  value,
  onChange,
  searchValue,
  onSearchChange,
  labelsValue,
  onLabelsChange,
}: AuthorFilterProps) {
  return (
    <Box
      sx={{
        display: 'flex',
        gap: 1.5,
        alignItems: 'center',
        flexWrap: 'wrap',
      }}
    >
      {/* Search */}
      <TextField
        size="small"
        placeholder="搜索 Issue 标题..."
        value={searchValue}
        onChange={(e) => onSearchChange(e.target.value)}
        sx={{ width: { xs: '100%', sm: 220 } }}
        variant="filled"
        aria-label="搜索 Issue"
        slotProps={{
          input: {
            startAdornment: (
              <InputAdornment position="start">
                <Search sx={{ color: '#737686', fontSize: 20 }} />
              </InputAdornment>
            ),
          },
        }}
      />

      {/* Assignee filter */}
      <Autocomplete
        size="small"
        options={authors}
        getOptionLabel={(option) => option.display_name || option.username}
        value={authors.find((a) => a.username === value) || null}
        onChange={(_, newValue) => onChange(newValue?.username || '')}
        renderInput={(params) => (
          <TextField
            {...params}
            variant="filled"
            label="指派人"
            sx={{ minWidth: 160 }}
          />
        )}
        sx={{ width: { xs: '100%', sm: 180 } }}
        clearOnEscape
        aria-label="按指派人过滤"
      />

      {/* Labels filter */}
      <TextField
        size="small"
        placeholder="标签过滤 (逗号分隔)"
        value={labelsValue}
        onChange={(e) => onLabelsChange(e.target.value)}
        sx={{ width: { xs: '100%', sm: 200 } }}
        variant="filled"
        aria-label="按标签过滤"
      />
    </Box>
  );
}
