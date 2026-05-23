import { useState, useEffect } from 'react';
import { Box, TextField, Typography, Chip } from '@mui/material';
import type { ProjectPlatform } from '../../types';

export interface ParsedGitUrl {
  platform: ProjectPlatform | null;
  namespace: string;
  repoName: string;
  host: string;
  isValid: boolean;
}

interface GitUrlInputProps {
  value: string;
  onChange: (value: string) => void;
  error?: string;
  disabled?: boolean;
}

function parseGitUrl(url: string): ParsedGitUrl {
  const empty: ParsedGitUrl = { platform: null, namespace: '', repoName: '', host: '', isValid: false };
  if (!url.trim()) return empty;

  // HTTPS format: https://gitlab.com/group/subgroup/repo.git
  const httpsMatch = url.match(
    /^https?:\/\/([^/]+)\/(.+?)\/([^/]+?)(?:\.git)?$/
  );
  if (httpsMatch) {
    const [, host, namespace, repoName] = httpsMatch;
    const platform = detectPlatform(host);
    return { platform, namespace, repoName, host, isValid: true };
  }

  // SSH format: git@gitlab.com:group/subgroup/repo.git
  const sshMatch = url.match(
    /^git@([^:]+):(.+?)\/([^/]+?)(?:\.git)?$/
  );
  if (sshMatch) {
    const [, host, namespace, repoName] = sshMatch;
    const platform = detectPlatform(host);
    return { platform, namespace, repoName, host, isValid: true };
  }

  return empty;
}

function detectPlatform(host: string): ProjectPlatform | null {
  const lower = host.toLowerCase();
  if (lower.includes('github')) return 'github';
  if (lower.includes('gitlab')) return 'gitlab';
  return null;
}

export default function GitUrlInput({ value, onChange, error, disabled }: GitUrlInputProps) {
  const [parsed, setParsed] = useState<ParsedGitUrl>({
    platform: null,
    namespace: '',
    repoName: '',
    host: '',
    isValid: false,
  });

  useEffect(() => {
    setParsed(parseGitUrl(value));
  }, [value]);

  return (
    <Box>
      <TextField
        fullWidth
        label="Git URL"
        placeholder="https://gitlab.com/group/my-project.git"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        error={!!error}
        helperText={error || '支持 HTTPS 和 SSH 格式'}
        disabled={disabled}
        required
      />

      {parsed.isValid && (
        <Box
          sx={{
            mt: 1.5,
            p: 2,
            bgcolor: '#f3f3fe',
            borderRadius: '8px',
            display: 'flex',
            flexDirection: 'column',
            gap: 1,
          }}
        >
          <Typography variant="body2" sx={{ color: '#434655', fontWeight: 500 }}>
            解析结果
          </Typography>
          <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 1, alignItems: 'center' }}>
            {parsed.platform && (
              <Chip
                label={parsed.platform === 'gitlab' ? 'GitLab' : 'GitHub'}
                size="small"
                sx={{
                  bgcolor: parsed.platform === 'gitlab' ? '#fc6d26' : '#24292f',
                  color: '#ffffff',
                  fontWeight: 500,
                  fontSize: '12px',
                }}
              />
            )}
            <Typography variant="body2" sx={{ color: '#191b23' }}>
              <Box component="span" sx={{ color: '#434655' }}>命名空间：</Box>
              {parsed.namespace}
            </Typography>
            <Typography variant="body2" sx={{ color: '#191b23' }}>
              <Box component="span" sx={{ color: '#434655' }}>仓库名：</Box>
              {parsed.repoName}
            </Typography>
          </Box>
        </Box>
      )}
    </Box>
  );
}

export { parseGitUrl };
