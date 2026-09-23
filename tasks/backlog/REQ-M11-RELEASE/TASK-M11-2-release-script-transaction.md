# TASK-M11-2: Transacción y CLI de `scripts/release`

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `NO_HUMAN`
- Author / reviewer: assigned on activation; independent reviewer required

## Scope

- Objective: implementar/testear default patch, patch/minor/major/exacta, preflight, gates, commit/tag anotado y diagnósticos no destructivos.
- Allowed paths: `scripts/release`, tests, docs de uso y adaptador Git estrictamente necesario.
- Prohibited paths: Actions, publicación real, secretos, replace destructivo y producto.
- Non-goals: tag/commit/push/release reales durante desarrollo.

## Acceptance and verification

- Fixtures prueban todos los bumps, SemVer inválido, downgrade, tree/branch/main inválidos, existencia remota/local y cero efectos remotos ante fallo.
- `--replace` sólo acepta versión exacta y delega operación remota a TASK-M11-6.
- Shellcheck/formato/tests focalizados y `CI=true ./scripts/verify` pasan.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
