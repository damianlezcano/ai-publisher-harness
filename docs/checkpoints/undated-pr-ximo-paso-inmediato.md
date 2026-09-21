## Próximo paso (inmediato)

> **Nota (cambio «Sí» integrado en `main`):** el pase grande de corrección de
> aceptación humana (7 ítems, COMPLETO en `ebeac0e`) **preservó** la nueva
> confirmación de eliminación de conversación con «Sí» (ítem 8 del pase previo), y
> el **próximo AppImage fresco** debe construirse desde un `main` que ya la incluya
> y también las correcciones Creation/Share/Chat (el actual `930ee074…` es de
> `773278d` y NO trae este pass).

**PASS CREATION/SHARE/CHAT INTEGRADO Y VERIFICADO (`ebeac0e`).** Repo en
`TÉCNICAMENTE LISTO PARA RE-ACEPTACIÓN HUMANA` en cuanto se construya el AppImage
fresco. El ÚNICO gate siguiente es: (1) **FRESH REAL APPIMAGE BUILD + VERIFICACIÓN
TÉCNICA** desde main `ebeac0e`, y (2) que el **product owner re-corra el escenario
real §17/§15** sobre ESE AppImage nuevo. Solo el humano puede marcar HUMAN
ACCEPTED. M11 NO iniciar.

1. Construir AppImage NUEVO desde main `ebeac0e` con `scripts/smoke-package
   appimage` (fetch-sidecars → `cargo tauri build --bundles appimage`), sidecars
   pineados SIN cambiar (opencode 1.18.25, cloudflared 2026.8.3), `./scripts/verify`
   EXIT=0 contra el artefacto fresco, lanzamiento real en Fedora/Wayland con PATH
   sin sidecars, y luego entregar al product owner.
2. El product owner re-corre el escenario real §15 sobre el AppImage NUEVO:
   conversación nueva + adjunto de rosco + prompt real → el asistente responde y
   genera la creación; card de creación [Abrir][Compartir] (título humano, no
   "index"; sin cards duplicadas en turnos siguientes); Abrir funciona; el agente
   usa el archivo; Compartir produce URL pública usable con EL JUEGO (no "Material
   del proyecto"); sin burbuja vacía "Asistente"; sin toast duplicado; modelo en
   Configuración; menú "…" → Eliminar conversación con confirmación «Sí»;
   renombrar/eliminar conversación, reinicio y delete persistido. Solo el humano
   acepta el AppImage final. NO afirmar aceptación humana desde OpenCode.
3. NO iniciar M11. Este pass queda en TÉCNICAMENTE LISTO esperando el AppImage
   nuevo y la re-aceptación humana.
2. **Seguimiento recomendado NO bloqueante (de las reviews de G):**
   - (UX NIT-1 / qwen LOW) `PublishPanel.tsx`: la URL pública es `<p>` dentro de
     `role="menu"`; envolver en `role="group"` (o mover los `<p>` al contenedor
     del popover) para no saltar el texto en lectores de pantalla.
   - (qwen LOW) `ComposerBar.tsx` `modelOptionLabel`: al caer a etiqueta genérica
     ("De pago"/"Gratis") cuando `name===modelId`, agregar el nombre del
     proveedor para evitar opciones indistinguibles.
   - (qwen LOW) `useShareControl.ts`/`WorkspaceView.tsx`: `onShare` en una
     tarjeta de creación abre el menú del ShareControl del composer (mismo hook,
     distinta ubicación); enfocar/anunciar el menú revelado.
   - (qwen/UX NIT) `messages.timeline.resourceLabel`, CSS `.message-resource`,
     `humanSize` (export sin uso) quedaron muertos; cleanup de catálogo/CSS.
   - (re-review code/a11y NIT) `registrar.rs`: el error del copy sidecar
     best-effort se descarta con `let _ =` sin log; agregar debug/warn.
   - (re-review code/a11y NIT) Skip lists (`build`, `dist`, `target`,
     `materials`) a cualquier profundidad podrían excluir una carpeta de
     actividad con ese nombre; improbable en este dominio, aceptado.
   - (re-review code/a11y LOW) `app.rs prepare_share_visibility`: Compartir
     explícito de una card no-web no degrada un Web público existente (la raíz de
     la URL puede no ser el artifact de la card); M1 le quitó su peor
     manifestación; revisitar solo si el producto quiere democión de cualquier
     Web público cuando el target no es Web.
   - (F review) chequeo de existencia de proyecto autoritativo DENTRO del lock
     del agente en `AgentService::run` + test single-instance delete↔agent.
3. NO iniciar M11. El pass de corrección queda en TÉCNICAMENTE READY FOR HUMAN
   REVIEW esperando aceptación humana.
