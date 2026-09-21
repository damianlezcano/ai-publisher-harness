## Contratos clave actuales (para los workers)

- **Message/Project:** `Project.messages: Vec<Message>` schema v3
  (`crates/project-core/src/lib.rs:342-357, 400-420`); `Message { id, role,
  text, status, createdAt, materialIds, creationIds }`. Validación: user msg
  solo `material_ids`, assistant msg solo `creation_ids`.
- **Creation:** `Creation { id, displayName, kind: Web|Document|Image|File,
  visibility, relativePath, contentType?, byteSize, revision,
  parentCreationId?, createdAt }` (`lib.rs:378-399`). UI capabilities
  (open/preview/publish) se derivan en facade, no se almacenan.
- **DTOs:** `ProjectSummary {id,name,createdAt,updatedAt,shared}`,
  `ProjectView {id,name,materials,creations,messages,publication}`,
  `MessageView`, `CreationView {id,displayName,kind,visibility,byteSize,
  createdAt,revision}` (`crates/project-app/src/dtos.rs`, `app/src/types.ts`).
- **Frontend estado:** `App.tsx` (conversations/selectedId/conversation/
  agentPhase/agentMessage/settingsOpen), `WorkspaceView` (pendingUser/
  sendError/drag-drop/import), `ChatPanel` (timeline derivada),
  `ComposerBar` (prompt/attachmentIds/model selector), `ConversationsSidebar`
  (lista, rename inline, NO delete). `PublishPanel` = ShareControl (single
  Compartir en bottom bar).
- **Agent:** `AgentService::run` (`service.rs:49-103`) ensure_ready →
  open_session(workspace) → provision_attachments → send → registrar artifacts
  (skip `materials/`). `agent://task` events desde `commands.rs:274-322`.
  Backend status en `OpenCodeBackend.status()` → `BackendStatus` enum
  (`crates/project-opencode/src/status.rs:1-7`): Stopped|Starting|Ready|Failed;
  expuesto a UI solo vía `agent_status`/`app_status` (sin uso frontend).
- **Attachments:** `resolve_attachments` autoriza contra materiales del
  proyecto; copia a `workspace/materials/<n>-<name>`; augment_prompt lista.
  Read path seguro (`project-fs` validate_read_path). Cleanup: sin cleanup
  explícito de `workspace/materials/` tras run (aceptado).
- **Publicación:** `publish/unpublish/publication_status`, túnel Cloudflare,
  QR frontend. TEMPORAL; honestidad de enlace. Reusar tal cual.
- **Pins sidecar:** opencode 1.18.25, cloudflared 2026.8.3 (M10, sin cambios).
- **Copy catálogo:** `app/src/messages.ts` = catálogo ejecutable (ADR-0012);
  tests verdes deben seguir.
