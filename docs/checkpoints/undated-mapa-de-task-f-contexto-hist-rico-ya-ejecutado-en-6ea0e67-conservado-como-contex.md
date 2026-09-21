## Mapa de Task F (contexto histórico — YA EJECUTADO en `6ea0e67`, conservado como contexto)

Backend delete ya existe end-to-end pero NO des-publica (bug hallazgo 16):
(MAPEO ORIGINAL — Task F ya resuelta arriba; se conserva para auditoría del
estado previo.)

- `project_delete` Tauri: `app/src-tauri/src/commands.rs:80-89`.
- `AppState::delete_project`: `crates/project-app/src/app.rs:310-317` — solo
  `self.projects.lock().delete_project(&pid)`, NO llama `self.unpublish` →
  entrada stale en `PublicationManager.published` (proyecto "shared" hasta
  restart). **Fix Task F: delete debe unpublish antes/consistente.**
- `ProjectService::delete_project`: `crates/project-core/src/lib.rs:807-811`:
  `get` → `repository.delete` (metadata) → `content.remove_project_tree`.
  `FilesystemProjectRepository::delete` borra el dir del proyecto
  (`project-fs/src/lib.rs:495-505`); `remove_project_tree` borra el árbol si
  existe (`project-fs/src/lib.rs:754-760`). Owner del árbol = proyecto (única
  duración; sin recursos compartidos entre proyectos: materials/creations son
  por-proyecto, `inputs/<id>` y `outputs/<id>` bajo `projects/<pid>/`).
- `AppState::unpublish`: `app.rs:1183-1189` → `PublicationManager::unpublish`
  (`crates/project-publication/src/manager.rs:306-340`, AlreadyLocal si no
  publicada, idempotente). Tauri `unpublish`: `commands.rs:349-353`.
- List ordenado por `updated_at` desc (`project-fs/src/lib.rs:438-442`);
  `rename_project` actualiza `updated_at` (`project-core/lib.rs:798-806`) →
  renombrar mueve al tope. Requisito F "order semantics unchanged": preservar
  esta regla (updated_at desc), no reintroducir otra.
- Rename UI ya existe: inline ✎ en `ConversationsSidebar.tsx:24-52,172-180`
  (usa `api.projectRename`, `api.ts:29-30`). Delete NO está cableado en la UI.

Frontend conversación:

- `app/src/App.tsx`: `conversations/selectedId/conversation` state;
  `refreshConversations` (28-33), `openConversation` (40-54), efecto inicial
  auto-crea default si lista vacía (56-86). Selection post-delete debe vivir
  aquí (refrescar lista, elegir activa, limpiar si última).
- `ConversationsSidebar.tsx` (189 líneas): props `conversations/selectedId/
  onSelect/onRefresh`; rename inline + ✎; NO delete. Agregar menú ⋮ contextual
  (Renombrar / Eliminar conversación) + ConfirmDialog. Copy catálogo en
  `app/src/messages.ts` (conversations.* / common.*); NO ProjectId/paths/términos
  técnicos en UX.
- `ConfirmDialog.tsx` (type-name-to-confirm, 15-48) existe y está testado
  (`ConfirmDialog.test.tsx`) pero NO cableado en conversaciones. UX F pide
  confirmación humana simple: "¿Eliminar esta conversación?" / "Se eliminarán
  los mensajes y los recursos asociados a esta conversación." / [Cancelar]
  [Eliminar]; visualmente destructivo; sin delete silencioso.
- `api.projectDelete`: `app/src/api.ts:31`. `AppError`/`errorMessage`:
  `api.ts:103-115`.
- Tests frontend: patrón `App.test.tsx` (mock invoke/listen), vitest + testing
  library; 193 tests verdes en main. Backend tests: `crates/project-app/tests/
  app_facade.rs` (delete ya en `project_lifecycle`), `project-fs/tests/
  project_lifecycle.rs` (delete: 1138, 1157, 1173), `project-publication/tests/`
  (unpublish idempotente).

Decisión ownership recursos: en la arquitectura actual NO hay recursos
compartidos entre conversaciones (cada proyecto es dueño exclusivo de su árbol);
el único estado cruzado durable es `PublicationManager.published` (manejar con
unpublish). Por lo tanto NO se requiere ARCHITECTURE_ESCALATION por ownership:
delete del proyecto borra su árbol completo (mensajes+materials+creations) y
debe unpublish primero para no dejar entrada stale. Si el autor encontrara una
referencia cruzada real no contemplada, debe parar y escalar, no adivinar.
