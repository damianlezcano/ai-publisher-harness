# TASK-M11-7: Documentación, drills, review y human gate

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `RELEASE_HUMAN`
- Author / reviewer: assigned on activation; independent reviewer required

## Scope

- Objective: documentación canónica, drills de fallo, evidence y coordinación de review/human gate.
- Allowed paths: docs de release/version/distribución/recovery, evidence y tests no productivos.
- Prohibited paths: activar/publicar por sí solo, producto, updater o declaración de PASS sin evidencia.
- Non-goals: publicar sin instrucción humana separada.

## Acceptance and verification

- Maintainers pueden preparar, diagnosticar, reintentar y reemplazar un release; se distingue cleanup de publicación de rollback de instalación.
- Evidence registra SHA/tag, inputs/runners, hashes, resultados por plataforma y failure semantics sin secretos/datos de usuario.
- Review independiente `PASS` y autorización humana explícita preceden cualquier primer release real.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
