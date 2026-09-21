## Estado previo (CONFIRMACIÓN DE ELIMINACIÓN CON «SÍ» — CAMBIO FRONTEND BOUNDED, INTEGRADO, 2026-09-01)

- **DELETE-CONFIRMATION «SÍ» (frontend, acotado) INTEGRADO.** Cambio puntual sobre el
  diálogo compartido `app/src/components/ConfirmDialog.tsx` para la ELIMINACIÓN DE
  CONVERSACIÓN: ya NO se exige escribir el título exacto de la conversación; ahora se
  confirma con la afirmación **«Sí»**. `normalizeConfirmation` (trim → toLowerCase →
  NFD → strip U+0300–U+036f) acepta `Sí`/`sí`/`SI`/`si` y tolera espacios al inicio/final.
  Cadenas ajenas (`No`, `borrar`, el propio título, `s i`, `siii`, solo-espacios, vacío)
  NO habilitan el botón. **Enter NO puede saltar la confirmación** (input fuera de
  `<form>`, botones `type="button"`, `useFocusTrap` solo mapea Escape/Tab); el botón
  `danger` sigue `disabled={!ready || busy}`; Cancel/Escape/backdrop → `onCancel` nunca
  `onConfirm`.
- **SIN FUGA DE ALCANCE / SIN RELAJAR TAREA F.** La regla `ready` quedó:
  `confirmText !== undefined ? value === confirmText : normalizeConfirmation(value) ===
  normalizeConfirmation(messages.common.confirmYes)`. El flujo de PROYECTOS
  (`ProjectsView.tsx`, pasa `confirmText={deleting.name}`) conserva el matching **exacto**
  original (case/accent/sensitive, sin trim) → byte-idéntico al pre-cambio. El flujo de
  CONVERSACIÓN (`ConversationsSidebar.tsx`, pasa solo `confirmPrompt`, sin `confirmText`)
  usa la rama afirmativa. `commitDelete` (guard in-flight/busy, fail-closed, reset solo
  en éxito) y toda la semántica destructiva/persistencia/unpublish/filesystem de Task F
  quedaron **intactas** (diff solo frontend, 5 archivos, sin Rust/tauri/api).
- **A11Y / COPY.** `confirmPrompt` se asocia al input vía `aria-describedby` (`<p
  id="confirm-prompt">`); foco inicial en el input; `role="dialog" aria-modal` intactos.
  Copy voseo: `confirmYes: "Sí"`, `confirmPrompt: "Para confirmar, escribí Sí."`,
  `confirmNameLabel: "Confirmación"` (label sr-only genérico, aceptado).
- **REVIEWS INDEPENDIENTES (qwen3.8-flash, sesiones FRESH):** primera →
  **REQUEST_CHANGES** (should-fix scope-leak del matching + should-fix `aria-describedby`;
  nits de tests); fix acotado aplicado → re-review **APPROVE**. Nota: la sugerencia
  literal del reviewer (`confirmText === messages.common.confirmYes ? …`) habría roto
  `ConfirmDialog.test.tsx` (que pasa `confirmText` explícito); se resolvió con la regla
  explícito=exacto / ausente=afirmativo.
- **VERDE:** vitest FE **214/214** (21 archivos), `tsc --noEmit` 0, `eslint` 0,
  `prettier --check` 0, **`./scripts/verify` EXIT=0** (cargo check, contracts M10 +
  UX_REDESIGN_01, fetch-sidecars).
- **PENDIENTE (fuera de este cambio acotado):** el AppImage `930ee074…` se construyó
  desde `773278d` y **NO incluye** este cambio; el próximo AppImage fresco +
  re-aceptación humana deben incluirlo. El pase grande de corrección de aceptación
  humana (8 ítems) sigue pendiente y **debe preservar** esta confirmación con «Sí»
  (ítem 8). M11 **NO INICIADO**. El orquestador rota en este checkpoint.
