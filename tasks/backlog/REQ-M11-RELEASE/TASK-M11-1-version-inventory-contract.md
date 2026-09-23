# TASK-M11-1: Inventario ejecutable y contrato de versión

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `NO_HUMAN`
- Author / reviewer: assigned on activation; independent reviewer required

## Scope

- Objective: declarar/probar una fuente canónica y todas las derivadas de versión de producto.
- Owned paths: futuros tests/gates de versión y fuentes derivadas inventariadas.
- Prohibited paths: workflow/release remoto, producto, pins de componentes/modelo.
- Non-goals: ejecutar un release.

## Acceptance and verification

- El inventario detecta divergencias y nuevas fuentes; Cargo/Tauri/package/packaging/verify quedan alineados o una excepción está explícita y probada.
- Lockfiles, crates internos `0.0.0` y dependencias no se confunden con versión EducAI.
- Gates focalizados, `./scripts/test-distribution-contracts` y `CI=true ./scripts/verify` pasan.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
