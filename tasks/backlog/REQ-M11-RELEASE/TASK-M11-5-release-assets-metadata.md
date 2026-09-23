# TASK-M11-5: Assets, checksums y `release.json`

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `NO_HUMAN`
- Author / reviewer: assigned on activation; independent reviewer required

## Scope

- Objective: generación/validación determinista de assets, SHA-256 y metadata schema-versionada.
- Allowed paths: herramientas/esquema/tests de metadata y documentación de contrato.
- Prohibited paths: secretos, rutas locales, inflar modelo en bundles, publicación o cambiar componentes.
- Non-goals: updater in-app.

## Acceptance and verification

- Tests verifican nombres, bytes/hashes y que checksums cubre ambos binarios inequívocamente.
- JSON contiene versión/tag/commit, plataformas/arquitecturas, componentes/payloads y modelo/revisión sin datos sensibles.
- Cruza compatibilidad ONNX Runtime/modelo y rechaza ausencia, divergencia o asset extra.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
