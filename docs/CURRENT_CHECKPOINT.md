# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M9 — Education UX Polish
- Current phase: **CLOSED** (M9 implementation complete)
- Current main commit: (ver el final de esta sección; merge final de T9 + gate T10)
- M1-M9: cerrados. M9 design Implemented. ADR-0012: Accepted.
- Terminología canónica: **`Compartir`** (Compartir/Compartiendo…/Compartido/
  Enlace para compartir/Copiar enlace/Abrir enlace/Mostrar QR/Dejar de
  compartir/No compartido). IDs internos `Publication*`/`publish`/`unpublish`
  NO renombrados.
- M9 boundary: frontend-only. Cero cambios de project-core/fs, AgentEngine,
  PublicationManager; cero Tauri commands/capabilities/window nuevos; cero
  invariantes de seguridad tocadas.
- verify (final): **PASS — "M9 contract passed"** (gate discrimina en
  `app/src/messages.ts` + `app/src/guidance.ts`). 1097 tests determinísticos
  (949 Rust + 148 frontend). `git diff --check` limpio.
- M10 (packaging): **no iniciado**

## Resultado por tarea M9 (T1-T10)

| # | Task | Resultado | Commit(s) integrados |
| --- | --- | --- | --- |
| 1 | Message catalog + terminology | INTEGRADO | 9384e1d |
| 2 | Visual system + responsive tokens | INTEGRADO | 09bf097 |
| 3 | Shared primitives + a11y hooks + guidance | INTEGRADO (2 fixes review) | fbcd8a7, da4842a |
| 4 | Projects UX (first-run, empty, Ctrl+N) | INTEGRADO | 2b4c5f5 |
| 5 | Chat/composer UX (multiline, Ctrl+Enter, attach) | INTEGRADO | 79a7afc |
| 6 | Materials UX (summary, empty, busy) | INTEGRADO | 477f0e4 |
| 7 | Creations UX (switch + badge + clarifier) | INTEGRADO | b58bf16 |
| 8 | Sharing UX + QR + temporary-link + stop confirm | INTEGRADO (+1 key common.confirm) | e663891 |
| 9 | Cross-cutting a11y + keyboard + errors | INTEGRADO | f5c8719 |
| 10 | Gate + docs + verify + checkpoint | COMPLETO | este commit |

Canonical UX alcanzado: first-run guide dismissible; empty states con siguiente
acción; composer multilinea con Ctrl/Cmd+Enter, "Adjuntar material" y estado
"Conectá una IA"; resumen de importación por lotes; toggle de visibilidad claro
con badge; flujo Compartir → Compartiendo… → Compartido con mensaje de enlace
temporal honesto; QR grande con título del proyecto; banner de estado de
proveedor (gratis/requires-choice/needs-reconnect); errores guiados (título +
mensaje + acción), nunca raw; confirmaciones destructivas solo donde aplica;
foco/teclado/Esc consistentes vía Dialog compartido; layout responsive desktop
3 anchos; sin IDs/paths/puertos/Cloudflare/OpenCode en la UI.

## Modelos usados (MODEL_REQUESTED == MODEL_ACTUAL)

- Autores: T1/T2/T4/T5/T7 Composer 2.5; T3/T6/T8/T9 DeepSeek V4 Flash.
- Revisores: T1/T2/T4/T5/T7 DeepSeek V4 Flash (PASS); T3/T9 Qwen3.8 Max
  (APPROVE); T6/T8 Composer 2.5 (APPROVE).
- Nota: en algunos lanzamientos el wait-output de UI del launcher timeouteó por
  wraplínea; el modelo se confirmó por inspección directa del panel. Un agente
  opencode (T6) crasheó una vez (illegal instruction) y se relanzó OK.

## Worktrees / panes

- Limpieza pendiente en cierre: worktrees de M9 y panes de Herdr (autores y
  revisores ya terminaron; todos los commits están integrados y verificados).
- Integration checkout (main) es lead-only y queda limpio.

## Documentos para la próxima sesión fresca

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M9_DESIGN.md (Implemented), docs/UX.md (Compartir), docs/VERIFY.md (M9)
- docs/decisions/0012-* (Accepted), docs/CURRENT_CHECKPOINT.md (este documento)
- **M10 = Packaging** (Linux AppImage/RPM; sidecars OpenCode/cloudflared). No iniciar.