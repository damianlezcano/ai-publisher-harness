# TASK-M11-3: Build nativo Linux de release

- Requirement: `REQ-M11-RELEASE`
- Base SHA: `09bcb280ba7670b42c86e62111a814a410f38561`
- Status: `PLANNED`
- Human gate: `NO_HUMAN`
- Author / reviewer: assigned on activation; independent reviewer required

## Scope

- Objective: job Actions Linux que reutilice Ubuntu 24.04/AppImage y entregue un artefacto versionado validado.
- Allowed paths: workflow futuro, packaging Linux/tests/docs estrictamente necesarios.
- Prohibited paths: cross-compilation, rediseño AppDir, publicación, pins/producto sin fallo reproducido.

## Acceptance and verification

- Parte del SHA del tag, no output stale; ejecuta `package linux-appimage`, GLIBC 2.39, WebKit, graphics y ONNX gates.
- Inspecciona sidecars/payload, entrega un solo AppImage con bytes/hash al agregador.
- Tests de workflow/packaging aplicables y `CI=true ./scripts/verify` pasan.

## Handoff

- Implementation result:
- Verification evidence:
- Reviewer verdict:
- Rework history:
