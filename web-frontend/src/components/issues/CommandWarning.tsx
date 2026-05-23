import { Box, Typography } from '@mui/material';
import WarningAmberIcon from '@mui/icons-material/WarningAmber';

// Validation command whitelist prefixes
const ALLOWED_PREFIXES = [
  'cargo test',
  'cargo build',
  'cargo clippy',
  'npm test',
  'npm run',
  'npx',
  'yarn test',
  'yarn run',
  'pnpm test',
  'pnpm run',
  'go test',
  'python -m pytest',
  'pytest',
  'make',
  'curl',
  'grep',
  'cat',
  'ls',
];

function extractCommands(content: string): string[] {
  const commands: string[] = [];
  // Match backtick-wrapped commands in the content
  const backtickRegex = /`([^`]+)`/g;
  let match;
  while ((match = backtickRegex.exec(content)) !== null) {
    commands.push(match[1].trim());
  }
  return commands;
}

function isCommandSafe(command: string): boolean {
  return ALLOWED_PREFIXES.some((prefix) => command.startsWith(prefix));
}

interface CommandWarningProps {
  content: string;
}

export default function CommandWarning({ content }: CommandWarningProps) {
  // Only check the Validation section
  const validationIdx = content.indexOf('## Validation');
  if (validationIdx === -1) return null;

  const validationSection = content.slice(validationIdx);
  const commands = extractCommands(validationSection);
  const unsafeCommands = commands.filter((cmd) => !isCommandSafe(cmd));

  if (unsafeCommands.length === 0) return null;

  return (
    <Box
      sx={{
        backgroundColor: '#ffdbd0',
        borderRadius: '8px',
        p: 2,
        display: 'flex',
        gap: 1.5,
        alignItems: 'flex-start',
      }}
    >
      <WarningAmberIcon sx={{ color: '#832600', fontSize: 20, mt: 0.25 }} />
      <Box>
        <Typography
          variant="body1"
          sx={{ fontWeight: 500, color: '#390c00', mb: 0.5 }}
        >
          检测到非白名单命令
        </Typography>
        <Typography variant="body2" sx={{ color: '#832600', mb: 1 }}>
          以下命令不在安全白名单中，请确认是否安全：
        </Typography>
        {unsafeCommands.map((cmd, i) => (
          <Box
            key={i}
            component="code"
            sx={{
              display: 'block',
              fontFamily: '"JetBrains Mono", monospace',
              fontSize: '12px',
              backgroundColor: 'rgba(131, 38, 0, 0.08)',
              borderRadius: '4px',
              px: 1,
              py: 0.5,
              mb: 0.5,
              color: '#832600',
            }}
          >
            {cmd}
          </Box>
        ))}
      </Box>
    </Box>
  );
}
