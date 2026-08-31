# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M9 — Education UX Polish
- Current phase: **IMPLEMENTATION_IN_PROGRESS**
- Current main commit: `f5217f7` (merge T1 + T2)
- M1-M8: cerrados. M9 design approved. ADR-0012: Accepted.
- Terminología canónica: **`Compartir`** (`Publicar` NO es acción de UI; IDs
  internos `Publication*`/`publish`/`unpublish` NO se renombran).
- M9 boundary: **frontend-only** (`app/src` + docs + tests). Cero cambios de
  project-core/fs, AgentEngine, PublicationManager; cero Tauri commands/
  capabilities nuevos. Si la implementación requiere alguno →
  `ARCHITECTURE_ESCALATION_REQUIRED`.
- verify (baseline M8): PASS. Gate M9 se agrega en T10.
- M10: **no iniciado**

## Implementación M9 — estado por tarea

| # | Task | Estado | Commit(s) | Autor | Revisor |
| --- | --- | --- | --- | --- | --- |
| 1 | Message catalog + terminology refactor | **INTEGRADO** | `9384e1d` → merge `54b8e77` | Composer 2.5 | DeepSeek V4 Flash |
| 2 | Visual system + responsive tokens | **INTEGRADO** | `09bf097` → merge `f5217f7` | Composer 2.5 | DeepSeek V4 Flash |
| 3 | Shared UI primitives + a11y + guidance | EN CURSO | — | DeepSeek V4 Flash | Qwen3.8 Max |
| 4-8 | Projects/Chat/Materials/Creations/Sharing UX | PENDIENTE (depende de 3) | — | per design | per design |
| 9 | Cross-cutting a11y + keyboard + errors | PENDIENTE | — | DeepSeek V4 Flash | Qwen3.8 Max |
| 10 | Gate + docs + verify + checkpoint | PENDIENTE | — | lead | Qwen3.8 Max |

Detalles:

- **T1** (`9384e1d`): `app/src/messages.ts` (catálogo único, helpers tipados,
  copy de first-run/empty-states/sharing/QR/provider/progress/error para M9),
  `messages.test.ts` (terminología canónica + términos prohibidos + helpers),
  `labels.ts` re-export desde el catálogo, todos los componentes consumen
  `messages.*`. Test 55/55. No toca styles.css.
- **T2** (`09bf097`): `styles.css` con tokens de color/spacing 4-32/radius/font,
  jerarquía tipográfica, botones primary/secondary/danger/ghost, cards, badges
  (.badge.ok/.neutral/.warning/.danger), dialogs, grid responsive (<960 1col
  chat→materials→creations→sharing; ≥960 2col chat+materials izq /
  creations+sharing der; ≥1280 max-width 1200px), min-width 800px. Bloque
  final `/* -- M9: shared primitives -- */` con classes para T3: `.empty-state`,
  `.toast-container`/`.toast`, `.error-notice`, `.provider-status-banner`,
  `.spinner`, `.first-run-guide`.

## Decisiones de orquestación

- Ownership: T1/T2 integrados; `messages.ts` y `styles.css` congelados para
  T3-T8 (read-only). Necesidad de key/class nueva → reportar al lead.
  Excepción acordada: T3 puede añadir CSS sólo en un bloque final marcado.
- Workers vía `scripts/agent-launch` (Herdr) con MODEL_REQUESTED == MODEL_ACTUAL
  verificado. T1/T2: `composer-2.5` confirmado. T3+: `opencode-go/deepseek-v4-flash`
  y reviewer `opencode-go/qwen3.8-max`.
- Panes: T1 `wZ:pE` (m9t1-messages), T2 `wZ:pF` (m9t2-visual) — terminados.
- Worktrees vivos: `../ai-publisher-m9-messages` (m9/messages), `../ai-publisher-m9-visual-system` (m9/visual-system) — ramas ya fusionadas, pendientes de limpieza en cierre.

## Documentos para la próxima sesión fresca

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M9_DESIGN.md (Approved), docs/UX.md (Compartir), docs/VERIFY.md
- docs/decisions/0012-* (Accepted), docs/CURRENT_CHECKPOINT.md (este documento)

## Presupuesto de sesión

- Budget Flash: rotar a ~100K, nunca dejar crecer a 300K+. Próximo hit de
  checkpoint: tras T3 y tras T4-T8.