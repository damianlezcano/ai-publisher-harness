# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M8 — Attachments / Advanced Resource UX — **DESIGN_APPROVED / IMPLEMENTATION_PENDING**
- Current phase: DESIGN_APPROVED_IMPLEMENTATION_PENDING (no hay implementación)
- Current main commit: ver `git log -1` (commit de cierre de diseño M8)
- M1-M7: completados y cerrados
- ADR-0010: Accepted (untrusted generated-content preview isolation)
- ADR-0011: Accepted (prompt attachment contract)
- verify: PASS — "M7 contract passed" (baseline previo a M8)
- git diff --check: limpio
- Worktrees: solo `main`
- Siguiente acción: implementación de M8 por sesión fresca `opencode-go/deepseek-v4-flash`

## Alcance M8 aprobado

- **Clipboard image paste**: evento DOM `paste` (sin privilegio de clipboard global);
  backend revalida contenido (magic-byte), cap 25 MB, dedup SHA-256, creación
  atómica de material. Original nunca se modifica.
- **Multi-file import**: un batch command (`materials_add_from_paths`), resultado
  determinista por archivo (`added` / `duplicate` / `unsupported` / `failed`),
  fallo parcial permitido, originales nunca modificados, dedup por SHA-256.
- **Prompt attachments**: frontend envía `MaterialId` (nunca paths); backend valida
  que cada material pertenece al proyecto y provisiona copias/referencias
  autorizadas para `AgentEngine` antes de `open_session`. `AgentEngine` y
  `AgentPrompt` estables salvo el cambio mínimo (`AgentRequest.attachments`).
- **Preview tiers**: imágenes/texto/Markdown se previsualizan in-app de forma
  segura; PDF/office se delegan al system handler; generated web content es
  **UNTRUSTED**.
- **Generated-web security boundary** (ADR-0010): webview `preview` sin
  capacidades (nunca hereda Tauri privileges ni IPC privilegiado), servidor
  loopback con token aislado (crate `project-preview`), CSP, fallback explícito a
  system browser si el aislamiento seguro no puede probarse. NUNCA debilitar
  seguridad para embeder el preview.
- **Core**: único cambio aditivo `remove_material` (project-core + project-fs);
  no se reabren invariantes M1-M7.

## Task graph (M8_DESIGN §22-26)

| # | Task | Level | Depends | Ownership |
| --- | --- | --- | --- | --- |
| 0 | Design + ADR approval | HIGH_ARCHITECTURE | — | DONE (V4 Pro + Human) |
| 1 | `remove_material` core+fs (+ tests) | MEDIUM | 0 | project-core, project-fs |
| 2 | `project-app` import/preview/attachment facade + DTOs + errors | MEDIUM_HIGH | 1 | crates/project-app/** |
| 3 | `AgentRequest.attachments` + AgentService provisioning + prompt augmentation | HIGH_CODING | 0 | project-agent/** |
| 4 | `project-preview` crate (loopback token server + containment + teardown) + security suite | HIGH_CODING — **HIGH_RISK / SECURITY REVIEW** | 0 | project-preview/** |
| 5 | Tauri commands + capabilities (`preview.json` empty) + preview window | MEDIUM | 2,3,4 | app/src-tauri/** |
| 6 | Frontend paste/chips/cards/preview UI + a11y + component tests | MEDIUM | 5 | app/src |
| 7 | Named suites + verify gate + smoke script + docs/VERIFY | MEDIUM/HIGH | 5,6 | tests, scripts, docs/VERIFY |
| 8 | Gate/docs/ADR + verify + checkpoint | HIGH_ARCHITECTURE | 7 | docs, verify |

- **Task 4 (web preview) = HIGH_RISK / SECURITY REVIEW**, reviewer independiente
  (OpenCode Go DeepSeek V4 Flash); evalúa el fallback §11 y reporta
  approve / fallback / request-changes.
- Tasks 1, 3 y 4 son independientes una vez aceptados los ADR.
- **Recommended first task**: Task 1 (`remove_material` core+fs) — desbloquea la
  task 2; tasks 3 y 4 pueden arrancar en paralelo.

## Model policy (implementación)

- IMPLEMENTATION_ORCHESTRATOR: `opencode-go/deepseek-v4-flash`.
- LOW: Cursor Composer 2.5 → fallback `opencode-go/mimo-v2.5`.
- MEDIUM: `opencode-go/deepseek-v4-flash` → Composer 2.5 → `opencode-go/qwen3.8-max`.
- MEDIUM_HIGH: Cursor Grok 4.6 medium → DeepSeek V4 Flash → `opencode-go/kimi-k2.7-code`.
- HIGH_CODING: Cursor Grok 4.6 medium → `opencode-go/kimi-k2.7-code`.
- HIGH_ARCHITECTURE: fresh `opencode-go/deepseek-v4-pro` only.

## Documents required by fresh implementation session

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M8_DESIGN.md (diseño aprobado, 30 secciones) — PRIMARIO
- docs/decisions/0010-*, 0011-* (Accepted)
- docs/M6_DESIGN.md, docs/M7_DESIGN.md (boundaries, commands, provider)
- docs/VERIFY.md (§M7 actual; §M8 se añade en la tarea 7), docs/WORKTREES.md,
  docs/MULTI_AGENT_WORKFLOW.md, docs/TESTING.md
- docs/CURRENT_CHECKPOINT.md (este documento)
