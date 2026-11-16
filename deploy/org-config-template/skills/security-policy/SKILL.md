---
name: security-policy
description:
  Organization security rules for requirements, designs and code; use when a
  change touches authentication, personal data, logging or external interfaces.
---

# Security policy

1. Every new or changed external interface states how callers authenticate and
   which permission they need.
2. Changes to personal data name the data category, where it is stored, who can
   read it and how long it is kept.
3. Security-relevant actions (login, permission changes, data export) write an
   audit log entry with actor, action, target and time. Never log the data itself.
4. If a requirement conflicts with these rules, say so under `## Concerns` with
   an owner instead of silently choosing one side.
