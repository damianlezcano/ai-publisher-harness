# TASK-M11-6: Publicación atómica y reemplazo explícito

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `TARGETED_HUMAN`; `RELEASE_HUMAN` para release real
- Author / reviewer: assigned on activation; independent security reviewer required

## Scope

- Objective: job agregador con permisos mínimos, publicación sólo del conjunto completo y replace fail-closed.
- Allowed paths: workflow, integración GitHub API/CLI segura, fixtures/mocks y recovery docs.
- Prohibited paths: borrar/recrear tags públicos automáticamente, PATs, releases reales de prueba, secretos y publicación parcial.
- Non-goals: rollback de instalación de usuario.

## Acceptance and verification

- Simulaciones Linux/Windows/metadata/upload/publish fallidos no dejan release estable parcial; sólo job final tiene `contents: write`.
- Replace exige versión exacta, inventario tag/release/assets, permisos/trazabilidad; aborta sin borrar estado previo ante tag divergente/protegido/API insegura.
- Reviewer independiente confirma permisos, retry/idempotencia, no secretos y recovery antes del gate humano.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
