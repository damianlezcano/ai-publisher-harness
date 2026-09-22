# TASK-METRICS-4: REWORK de responsabilidad UI de métricas

- Requirement: `REQ-METRICS-001`
- Base SHA: `bd3aaf6ea022fa0a0a1d06b302b8be2561793400`
- Status: `PASS`
- Human gate: `TARGETED_HUMAN` (posterior a `PASS` independiente)
- Author / reviewer: pendiente de lanzamiento verificado / pendiente de lanzamiento verificado e independiente

## Scope

- Objective: separar sin ambigüedad las dos responsabilidades visibles:
  - el popup de una respuesta muestra sólo métricas del turno propietario;
  - Detalles de la conversación muestra sólo configuración, acumulados de conversación, Knowledge acumulado y archivos/materiales.
- Owned paths: `app/src/components/AssistantMetrics.tsx`,
  `app/src/components/ConversationMetrics.tsx`, `app/src/messages.ts`, y sus
  pruebas directas (`AssistantMetrics.test.tsx`, `ConversationDetails.test.tsx`,
  y `ChatPanel.test.tsx` sólo si es necesario para la interacción).
- Allowed paths: tipos o pruebas estrictamente necesarias para las superficies
  anteriores, y este contrato para el handoff. No hay cambio backend salvo que
  una prueba demuestre que la UI no dispone del dato acumulado requerido;
  detenerse y escalar antes de ampliar el scope.
- Prohibited paths: `REQ-WIN-001`, rutas/sesiones/Knowledge, contratos de
  arquitectura, publicación, credenciales, empaquetado, logs/telemetría interna
  y cualquier UI ajena.

## Contract

- Popup: como máximo modelo+proveedor, duración, entrada, salida, caché,
  costo informado, llamadas y `Knowledge: Usado/No usado`. Los tokens/costo se
  muestran sólo con `source == provider_actual`; ausencias reales muestran
  `No disponible`, nunca `0` inventado. `remoteCalls = 0` se presenta como
  conteo medido, no como éxito. No mostrar modo/candidatos/evidencias/corpus/
  contexto/RAG ni diagnósticos.
- Conversación: eliminar toda métrica del último turno/respuesta. Mostrar sólo
  modelo configurado, uso acumulado (entrada/salida/caché/costo/llamadas),
  Knowledge acumulado (`Usado en N respuestas`, `Materiales utilizados: N`, o
  `No usado en esta conversación`) y archivos/materiales de la conversación.
  No sumar ni mostrar snapshots o diagnósticos de retrieval.
- Preserve AC-012 and AC-013. No exponer contenidos, paths, prompts, secretos
  ni modificar la semántica de llamadas del proveedor.

## Acceptance and verification

- Pruebas cubren popup completo, datos de proveedor ausentes, `remoteCalls=0`,
  Knowledge usado/no usado; y detalles acumulados que no duplican el último
  turno ni exponen métricas RAG internas.
- Run focused frontend tests plus typecheck, lint, format and build; then
  `./scripts/architecture-verify`, `CI=true ./scripts/verify`, and
  `git diff --check`.
- The author must not commit or push. Handoff supplies the exact uncommitted
  diff for independent read-only review; the lead applies only the reviewed
  patch to the integration checkout, also without a commit.

## Handoff

- Implementation result: uncommitted frontend patch integrated; no commit or push.
- Verification evidence: `pnpm test` (23 files / 375 tests), typecheck, lint,
  format check, build, `git diff --check`, `./scripts/architecture-verify`
  (14 contracts), and `CI=true ./scripts/verify` passed on the final diff.
- Reviewer verdict: `PASS` — independent `metrics4-ui-reviewer` confirmed that
  the accumulated Knowledge count uses only successful assistant responses
  conservatively linked by `turnId` to durable user-turn metrics; failed,
  cancelled, absent, and unlinked responses are excluded. It also confirmed
  the popup field limit, unavailable-material handling, conversation scoping,
  accessibility labels, provider semantics, AC-012/AC-013, and no REQ-WIN-001
  scope change.
- Rework history: created from the Human Gate REWORK instruction, 2026-09-22;
  multiple independent-review corrections resolved; `TARGETED_HUMAN` approved
  by the human owner on 2026-09-22.
