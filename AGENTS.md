# AGENTS.md

## Communication

- Always reply in Chinese.

## Browser And Login Verification

- When the user has provided valid account credentials for a local or test environment, use the provided username and password to perform a real login for browser verification.
- Do not bypass login by injecting mock authentication state into `localStorage`, cookies, or frontend stores unless the user explicitly asks for a mocked-auth test or real login is impossible.
- If real login fails, report the concrete error or blocker before falling back to mocked authentication.

## Frontend Node Environment

- The system default Node.js is Node v22, but commands executed with `workdir` set to `web-frontend` can resolve `/usr/local/bin/node` first and accidentally use Node v16.
- When running frontend commands from `web-frontend`, explicitly prefer Node v22 by prefixing the command with:
  `PATH="$HOME/.nvm/versions/node/v22.18.0/bin:$PATH"`.
- Example:
  `PATH="$HOME/.nvm/versions/node/v22.18.0/bin:$PATH" npx vitest run ...`
