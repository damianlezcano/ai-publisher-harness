# UX Principles

## Default language
Use non-technical concepts.

## Canonical user-facing vocabulary (M9)

This is the source of truth for rendered copy; `app/src/messages.ts` is its
executable reflection (ADR-0012). Spanish (`es-AR`).

| Concept | Canonical label(s) |
| --- | --- |
| App | EducAI |
| Project | Proyecto / Mis proyectos / Nuevo proyecto |
| Material | Material / Materiales |
| Creation | Creación / Creaciones |
| AI assistant | Asistente (panel) |
| Share action | Compartir |
| Sharing in progress | Compartiendo… |
| Shared state | Compartido |
| Not shared state | No compartido |
| Stop sharing | Dejar de compartir |
| Public link | Enlace para compartir |
| QR | Código QR / Mostrar QR |
| Preview | Vista previa / Abrir vista previa |
| Create | Crear / Creando… |
| Model | Modelo |
| Free model | Gratis |
| Paid model | De pago |

`Compartir` is the canonical user-facing share verb; `Publicar` is **not** used
as a primary UI action. Internal domain identifiers (`Publication*`, `publish`,
`unpublish`) are not renamed.

Avoid exposing in default UX:
- OpenCode
- Cloudflare
- Quick Tunnel
- API
- port
- tunnel
- runtime
- server
- DNS
- hosting
- localhost
- provider/model IDs
- filesystem paths
- internal metadata (IDs, revisions, hashes)

## Temporary-link honesty (sharing)

Every shared project shows, in user language:

> "Este enlace funciona mientras el recurso esté compartido. Si cerrás la
> aplicación, dejás de compartir o se corta la conexión, el enlace deja de
> funcionar."

We never claim permanent hosting.

## Sharing flow

```
No compartido ──[Compartir]──▶ Compartiendo… ──▶ Compartido
                                                    ├─ Copiar enlace
                                                    ├─ Abrir enlace
                                                    ├─ Mostrar QR
                                                    └─ Dejar de compartir
```

- Sharing is temporary and per project.
- Stopping one project never affects others.
- Closing/restarting the app may stop a live link.

## Empty states and first-run

Every empty state tells the user the next useful action. First-run is a short
dismissible guide (crear proyecto → agregar material → pedir a la IA → mirar la
creación → compartir), not a tutorial.

## Error recovery

Errors show a title, a plain-language message, and a next action where one exists
(Reintentar / Conectar IA / Abrir con la aplicación). Never a code, path, or
stack trace.

## Keyboard shortcuts

- `Ctrl/Cmd+Enter` — send prompt.
- `Esc` — close the focused dialog or cancel the create/rename form.
- `Ctrl/Cmd+N` — new project.

## Main screens
### Projects
List local projects and clearly show shared vs not-shared state. First-run guide
on empty.

### Project
Asistente + Materiales + Creaciones + Vista previa + Compartir/Dejar de compartir.

### Shared state
Show the share link, copy action, QR, and Dejar de compartir, with the
temporary-link note.

## Closing app
If one or more projects are shared, warn that the links will stop working.
