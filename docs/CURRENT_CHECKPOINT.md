# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M9 — Education UX Polish
- Current phase: **IMPLEMENTATION_IN_PROGRESS**
- Current main commit: `75f0747` (merge T8)
- M1-M8: cerrados. M9 design approved. ADR-0012: Accepted.
- Terminología canónica: **`Compartir`**. IDs internos sin renombrar.
- M9 boundary: frontend-only. Cero cambios backend/Tauri/capabilities.
- verify (baseline M8): PASS. Gate M9 se agrega en T10.
- M10: **no iniciado**

## Implementación M9 — estado por tarea

| # | Task | Estado | Autor | Revisor |
| --- | --- | --- | --- | --- |
| 1 | Message catalog + terminology | **INTEGRADO** `9384e1d` | Composer 2.5 | DeepSeek V4 Flash (PASS) |
| 2 | Visual system + responsive | **INTEGRADO** `09bf097` | Composer 2.5 | DeepSeek V4 Flash (PASS) |
| 3 | Shared primitives + guidance | **INTEGRADO** `fbcd8a7`+`da4842a` | DeepSeek V4 Flash | Qwen3.8 Max (APPROVE tras fix IMPORTANT×2) |
| 4 | Projects UX | **INTEGRADO** `2b4c5f5` | Composer 2.5 | DeepSeek V4 Flash (PASS) |
| 5 | Chat/composer UX | **INTEGRADO** `79a7afc` | Composer 2.5 | DeepSeek V4 Flash (PASS) |
| 6 | Materials UX | **INTEGRADO** `477f0e4` | DeepSeek V4 Flash | Composer 2.5 (APPROVE) |
| 7 | Creations UX | **INTEGRADO** `b58bf16` | Composer 2.5 | DeepSeek V4 Flash (PASS) |
| 8 | Sharing UX + QR | **INTEGRADO** `e663891` | DeepSeek V4 Flash | Composer 2.5 (APPROVE; +1 key common.confirm) |
| 9 | Cross-cutting a11y + keyboard + errors | EN CURSO | DeepSeek V4 Flash | Qwen3.8 Max |
| 10 | Gate + docs + verify + checkpoint | PENDIENTE | lead | Qwen3.8 Max |

Frontend tras T1-T8: **134 tests / 19 files, todos verdes** en main.

## T9 pendiente (trabajo EN CURSO en ../ai-publisher-m9-a11y-pass, branch m9/a11y-pass)

- Migrar ConfirmDialog y ProviderPanel al Dialog compartido (T8 ya migró QrDialog + stop-confirm).
- Live region única (ToastRegion + useToast) en App; toast "Tu recurso está listo" (agent.ready) al completar.
- ProviderStatusBanner en App: query modelGetSelected + providerList; free / requires-choice / needs-reconnect; Conectar IA abre ProviderPanel.
- ChatPanel: pasar aiUsable + onOpenProvider + onProviderError (needs-reconnect).
- CreationsPanel: pasar shared={project.publication.state === "published"} desde WorkspaceView.
- Errores de apertura/carga con guidance (ErrorNotice) sin raw.
- Tests App/ProviderPanel/ConfirmDialog actualizados.

## Modelos usados (MODEL_REQUESTED == MODEL_ACTUAL, verificado)

- T1/T2/T4/T5/T7 + T6rev/T8rev: `composer-2.5` (confirmado por launcher).
- T3, T6, T8, T9 (autor): `opencode-go/deepseek-v4-flash`.
- T3rev: `opencode-go/qwen3.8-max`.
- Nota: en 3 lanzamientos la verificación de UI del launcher timeouteó por wraplínea del estado; se confirmó el modelo por inspección directa del panel. T6 autor falló una vez (illegal instruction opencode) y se relanzó secuencialmente OK.

## Worktrees vivos (limpiar en cierre)

- ../ai-publisher-m9-messages, -visual-system (fusionadas)
- ../ai-publisher-m9-shared-primitives (+ review) (fusionadas)
- ../ai-publisher-m9-projects-ux, -chat-ux, -materials-ux (+review), -creations-ux, -sharing-ux (+review) (fusionadas)
- ../ai-publisher-m9-a11y-pass (EN CURSO)

## Documentos para la próxima sesión fresca

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M9_DESIGN.md (Approved), docs/UX.md (Compartir), docs/VERIFY.md
- docs/decisions/0012-* (Accepted), docs/CURRENT_CHECKPOINT.md (este documento)

## Presupuesto de sesión

- Rotar a ~100K, nunca a 300K+. Próximo hit: tras T9 y tras T10.