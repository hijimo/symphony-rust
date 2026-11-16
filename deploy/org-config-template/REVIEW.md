# Review instructions

Organization review policy. A repository's own `REVIEW.md` takes precedence.

## Important (P0/P1)

- Missing authentication or authorization on a new or changed endpoint.
- Personal data (passwords, tokens, ID numbers, phone numbers) written to logs or responses.
- Credentials committed in code, configuration or test data.
- Behaviour that contradicts the approved spec or plan.

## Nit (P2/P3)

- Missing tests for a new branch of logic.
- Unclear names, dead code, inconsistent error messages.

Report at most five nits. Do not report formatting that the linters already enforce.
