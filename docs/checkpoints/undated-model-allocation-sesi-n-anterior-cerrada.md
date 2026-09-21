## Model allocation (sesión anterior cerrada)

- **Creation/Share/UX human-acceptance pass (COMPLETE, integrado `ebeac0e`): orquestador
  `opencode-go/deepseek-v4-flash` (esta sesión, budget CONTINUE al cierre). Autor
  `cursor-grok-4.6-high` vía Cursor (`corr/creation-share-ux-pass`, commits `3ba7c5a` +
  fix `857d98c`). Revisor UX independiente `cursor-grok-4.6-high` FRESH (sesión previa,
  APPROVE). Revisor código/a11y `opencode-go/qwen3.8-flash` FRESH (`creation-share-code-review`,
  pane `w1F:p1G`, REQUEST_CHANGES con M1 MAJOR + m1-m7 MINOR + LOW/NIT). Fix acotado por
  autor Cursor Grok 4.6 High FRESH (`creation-share-fix`, pane `w1F:p1H`, commit `857d98c`).
  Re-review código/a11y `opencode-go/qwen3.8-flash` FRESH (`creation-share-rereview`, pane
  `w1F:p1J`, APPROVE). Re-review UX acotado `cursor-grok-4.6-high` FRESH
  (`creation-share-ux-rereview`, pane `w1F:p1K`, APPROVE). Merge `ebeac0e` + evidencia
  `05a2c2a`, `./scripts/verify` EXIT=0 en main (FE 217/217). Panes reviewers cerrados tras
  APPROVE; fixer/author a cerrar en cierre de sesión; worktree autor + branch
  `corr/creation-share-ux-pass` a limpiar. Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0
  sesiones.**
- **Post-T7 human blocker pass (este): orquestador `opencode-go/deepseek-v4-flash`
  (rota en ROTATE_SESSION_REQUIRED 117K tras merge/cleanup). Autor
  `cursor-grok-4.6-high` vía Cursor (`postt7-author`, pane `w1F:p1A`,
  `corr/post-t7-blockers`, commits `b106d07b` + fixes `21e9e5b`). Revisor UX
  independiente `cursor-grok-4.6-high` FRESH (`postt7-ux-review`, pane `w1F:p1B`,
  `corr/post-t7-ux-review`, APPROVE + re-APPROVE tras MAJOR). Revisor código/a11y
  `opencode-go/qwen3.8-flash` FRESH (`postt7-a11y-review`, pane `w1F:p1C`,
  `corr/post-t7-a11y-review`, REQUEST_CHANGES → APPROVE; MAJOR resuelto en
  `21e9e5b`). Merge `d6f97ab`, `./scripts/verify` EXIT=0 en main. Panes author y
  ambos reviewers a cerrar en cierre de sesión; worktrees/branches a limpiar.
  Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.**
- **Task G: autor `cursor-grok-4.6-high` (HIGH_VISUAL vía Cursor,
  `task-g-author`, pane `w1M:p1`, commit `e345520` en
  `corr/g-product-ux-pass`), revisor UX independiente `cursor-grok-4.6-high`
  FRESH (`task-g-ux-review`, pane `w1N:p1`, `corr/g-product-ux-review`,
  APPROVE con nits NIT-1..5), revisor código/a11y `opencode-go/qwen3.8-flash`
  FRESH (`task-g-a11y-review`, pane `w1P:p1`,
  `corr/g-product-ux-a11y-review`, APPROVE con LOW/NIT). Sin ciclos
  REQUEST_CHANGES. Merge `2451c50`, `./scripts/verify` PASS (EXIT=0). Panes
  cerrados tras APPROVE/integración; worktrees/branches de G a limpiar en
  cierre.**

- Orquestador previo: `opencode-go/deepseek-v4-flash` (cerrada en bootstrap
  HARD). **Este pass:** deepseek-v4-flash (A/B/C/D integradas; rota en
  CHECKPOINT_WARNING tras Task D).
- **Qwen3.8 Max: 0 sesiones. DeepSeek V4 Pro: 0 sesiones.** (seguir así;
  Qwen3.8 Max solo con ESCALATION_REASON explícito).
- Task A: autor `opencode-go/kimi-k2.7-code`, revisor `opencode-go/qwen3.8-flash`
  (APPROVE tras REQUEST_CHANGES). Ambos panes cerrados. Grok NO usado (G/F-UI
  pendientes).
- Task B: autor `opencode-go/kimi-k2.7-code`, revisor `opencode-go/qwen3.8-flash`
  (APPROVE, nits no bloqueantes). Commit `a16a07c`. Ambos panes cerrados.
- Task C: autor `opencode-go/kimi-k2.7-code` (`fd1d928`), revisor
  `opencode-go/qwen3.8-flash` (REQUEST_CHANGES → APPROVE; MAJOR-1 resuelto con
  regresión; NIT-1 poll cadence + MINOR-1 transient resend anotados no
  bloqueantes). Ambos panes cerrados. Backend sin cambios.
- **Task D: autor Composer 2.5 (`task-d-author`, commit `18ac233`), revisor
  `opencode-go/qwen3.8-flash` (`task-d-review`, APPROVE sin hallazgos). Ambos
  panes cerrados.** Grok NO usado (G/F-UI pendientes).
- **Task E: fix LOW commit `60dc786` en `corr/e-duplicate-render` (implementado
  por el orquestador deepseek-v4-flash — desviación de proceso documentada: la
  raíz ya estaba confirmada en el checkpoint y el cambio es acotado; el circuito
  autor→revisor se respetó para la revisión), revisor `opencode-go/qwen3.8-flash`
  FRESH (`task-e-review`, pane `w1F:p18`, APPROVE tras verificar contrato backend
  `send_message_run`, tests 193/193, tsc/eslint/prettier). NIT no bloqueante:
  CSS `.chat-status.ok` (`app/src/styles.css:407`) quedó muerto (solo referencias
  en selectores negativos de test). Merge `cea141e`, branch borrado, worktree y
  pane de review cerrados.** Grok NO usado (G/F-UI pendientes).
- **Task F: autor `opencode-go/kimi-k2.7-code` (`task-f-author`, pane `w1J:p1`,
  commits `26411f1` + fixes `14365c0`), revisor `opencode-go/qwen3.8-flash`
  (`task-f-review`, pane `w1K:p1`, FRESH — REQUEST_CHANGES → APPROVE tras
  verificar MAJOR-1 resuelto con lock-ordering sólido, fail-closed testeado, y
  gates verdes 199 FE + cargo + fmt/clippy). Ambas panes cerrados. Merge
  `6ea0e67`, branches `corr/f-conversation-delete`(-review) borrados, worktrees
  removidos. Hallazgos residuales NO bloqueantes anotados por el revisor:
  MINOR TOCTOU (si delete completa en la ventana entre el pre-check de
  `run_agent_with_inputs` y que `AgentService::run` adquiera el lock del agente,
  puede quedar un scratch dir `projects/<id>/workspace` sin datos de usuario;
  fix recomendado: chequeo de existencia autoritativo DENTRO del lock del agente
  en `run`, antes de `create_dir_all`), NIT comment del pre-check (usa lock
  distinto al del agente), NIT test de serialización usa dos AppState
  independientes (pasa vía cleanup, no vía el mecanismo "waits"; añadir twin
  single-instance), NIT disable UI solo cubre la conversación seleccionada.
  Grok NO usado (G pendiente).**
