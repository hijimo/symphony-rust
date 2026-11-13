# Symphony worker skills

Skills used by Symphony's workflow templates (`symphony-commit`,
`symphony-pull`, `symphony-push`, `symphony-land`). They are installed into the
`CODEX_HOME/skills` directory used by Symphony workers (default `~/.codex/skills`)
by `deploy/scripts/install-codex-skills.sh`, which `dev.sh` runs on start.

They do not depend on the workspace living inside this repository
(docs/方案/self-verification-and-agents-md.md, D10). The repository's own
`.codex/skills` stay for developing Symphony itself.
