# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M9 — Education UX Polish
- Current phase: **DESIGN_APPROVED_IMPLEMENTATION_PENDING**
- Current main commit: `ff4cd70` (cierre M8)
- M1-M8: cerrados
- ADR-0012: **Accepted** (catálogo de mensajes único, i18n-ready sin framework)
- Terminología canónica: **`Compartir`** (`Publicar` NO es acción de UI;
  identificadores internos `Publication*`/`publish`/`unpublish` NO se renombran)
- M9 boundary: **frontend-only** (`app/src` + docs + tests). Cero cambios de
  project-core/fs, AgentEngine, PublicationManager; cero Tauri commands/
  capabilities nuevos; cero cambios de invariantes de seguridad. Si la
  implementación requiere alguno → `ARCHITECTURE_ESCALATION_REQUIRED`.
- verify (baseline M8): PASS — "M8 contract passed" (gate M9 se agrega en T10)
- M10: **no iniciado**

## Implementación

- Orchestrator: `opencode-go/deepseek-v4-flash` (sesión fresca). DeepSeek V4 Pro
  queda reservado a fresh HIGH_ARCHITECTURE escalation.
- La implementación M9 **NO continúa en esta sesión**.
- Siguiente tarea de implementación: **T1** (catálogo de mensajes + refactor de
  terminología).
- Budget de sesión Flash: ideal 0-60K, aceptable 60-100K, evaluar rotación a
  ~100K, rotar a >=150K (nunca dejar crecer una sesión a 300K+).

## Task graph M9 (T1-T10)

| # | Task | Level | Depends | Author | Reviewer |
| --- | --- | --- | --- | --- | --- |
| 1 | Message catalog + terminology refactor | MEDIUM | — | Cursor Composer 2.5 | DeepSeek V4 Flash |
| 2 | Visual system + responsive CSS tokens | MEDIUM | — | Cursor Composer 2.5 | DeepSeek V4 Flash |
| 3 | Shared UI primitives + a11y hooks + guidance | MEDIUM | 1,2 | DeepSeek V4 Flash | Qwen3.8 Max |
| 4 | Projects view UX | MEDIUM | 3 | Cursor Composer 2.5 | DeepSeek V4 Flash |
| 5 | Chat/composer UX | MEDIUM | 3 | Cursor Composer 2.5 | DeepSeek V4 Flash |
| 6 | Materials UX | MEDIUM | 3 | DeepSeek V4 Flash | Cursor Composer 2.5 |
| 7 | Creations UX | MEDIUM | 3 | Cursor Composer 2.5 | DeepSeek V4 Flash |
| 8 | Sharing UX + QR + temporary-link messaging | MEDIUM | 3 | DeepSeek V4 Flash | Cursor Composer 2.5 |
| 9 | Cross-cutting a11y + keyboard + errors | MEDIUM_HIGH | 4-8 | DeepSeek V4 Flash | Qwen3.8 Max |
| 10 | Gate + docs + verify + checkpoint | MEDIUM | 9 | DeepSeek V4 Flash (lead) | Qwen3.8 Max |

T1 y T2 son independientes; T3 construye sobre ambos; T4-T8 en paralelo (panels
separados, no editar `styles.css`/`messages.ts` tras T1/T2); T9 integra; T10 gatea.

## Documentos para la próxima sesión fresca

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M9_DESIGN.md (Approved)
- docs/decisions/0012-* (Accepted)
- docs/UX.md (terminología canónica = Compartir)
- docs/VERIFY.md, docs/WORKTREES.md, docs/MULTI_AGENT_WORKFLOW.md, docs/TESTING.md
- docs/CURRENT_CHECKPOINT.md (este documento)
