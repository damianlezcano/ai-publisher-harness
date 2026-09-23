# TASK-M11-4: Build nativo Windows de release

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `NO_HUMAN`
- Author / reviewer: assigned on activation; independent reviewer required

## Scope

- Objective: job Windows x64/MSVC que reutilice `packaging/windows/build.ps1` y preserve REQ-WIN-001.
- Allowed paths: workflow, tests/inspección Windows y docs de build estrictamente necesarias.
- Prohibited paths: cross-compilation, omitir ONNX/providers, rediseñar REQ-WIN-001, publicar o tocar producto sin evidencia.

## Acceptance and verification

- Comprueba NSIS recién creado, OpenCode, cloudflared, ONNX Runtime/provider, hashes/tamaños y contratos Knowledge/distribución.
- Entrega exactamente el NSIS normalizado, bytes/hash/provenance; cualquier discrepancia bloquea.
- Pruebas Windows apropiadas y contracts complementarios pasan.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
