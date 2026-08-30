# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M7 — AI Provider Onboarding
- Current phase: DESIGN_PENDING
- Current main commit: `dcbfe4a` (`docs: mark M6 implemented and closed`)
- M1-M6: completados y cerrados
- verify: PASS — `M0 harness contract passed` + `M6 contract passed`
- git diff --check: limpio
- Worktrees: solo el checkout de integración `main`; sin worktrees/panes de M6 abiertos; sin stashes

## M7 objective

AI provider onboarding no técnico: conexiones/credenciales de proveedor,
mecanismos de auth (API key, OAuth/device flow), detección de proveedor,
listado de modelos, selección simplificada, "probar conexión", almacenamiento
seguro de credenciales, errores de proveedor, defaults/modelos gratuitos.

## M7 known constraints

- No implementar integraciones directas OpenAI/Gemini/DeepSeek/OpenRouter en el MVP.
- Credenciales/config de proveedores delegadas a OpenCode y almacenadas localmente cuando sea posible.
- OpenCode invisible para el usuario no técnico.
- verify debe seguir offline, sin credenciales y sin red (`docs/VERIFY.md`).
- La UI no introduce terminología técnica (port, tunnel, API key, OpenCode, ...).
- M6 excluyó explícitamente provider onboarding (`docs/M6_DESIGN.md` §2 y §39).
- El dominio no debe conocer detalles de proveedor (boundary M5).

## Security invariant (credentials)

`docs/SECURITY.md` #8: Credentials must never be written into project files,
logs, URLs or exported bundles. (Regla 12: el HTML/JS externo es contenido no
confiable en el contexto de preview.)

## M5 AgentEngine / OpenCode boundaries (relevantes a M7)

- `project-agent` es el único crate que conoce OpenCode, y solo dentro del adapter (ADR-0006).
- Un único `opencode serve` headless sobre loopback `127.0.0.1`; una sesión por proyecto (workdir = `workspace/`).
- XDG config aislada + `--pure`: el `~/.config/opencode` del desarrollador (permisos, plugins, credenciales) nunca afecta el comportamiento del producto.
- Auth del server: `OPENCODE_SERVER_PASSWORD`/`OPENCODE_SERVER_USERNAME` (basic auth loopback, hardening M5).
- Credential/provider/model UX diferida a M7 (consequences de ADR-0006).
- Los outputs se registran como `Creation` con visibilidad privada por defecto; nunca inferida.

## Accepted ADRs relevantes

Todas Accepted: ADR-0001 (Tauri 2 + core runtime), 0002 (metadata + filesystem
layout), 0003 (publisher boundary + http stack), 0004 (publication snapshot/
route/state), 0005 (cloudflare quick tunnel), 0006 (AgentEngine + OpenCode
adapter), 0007 (React 19 + Vite + TS + boundary Tauri).

## Unresolved HIGH_ARCHITECTURE decisions for M7

1. Credential storage strategy
2. OpenCode credential/config isolation boundary
3. Supported provider authentication mechanisms
4. API key vs OAuth/device flows
5. Cómo llegan las credenciales a OpenCode sin entrar en: project files, logs, URLs, prompts, exported bundles
6. Provider detection/listing
7. Model discovery
8. Simplified model selection
9. Free/default model policy
10. Test-connection semantics
11. Credential lifecycle (update/delete)
12. Provider error mapping
13. Integración con el AgentEngine M5 sin filtrar detalles de proveedor al dominio

## Documents a fresh architecture session must read

- CODEX_HANDOFF.md
- docs/CURRENT_CHECKPOINT.md (este documento)
- docs/AGENT_POLICY.md
- docs/ARCHITECTURE.md
- docs/SECURITY.md
- docs/UX.md
- docs/MILESTONES.md
- docs/M6_DESIGN.md (§2 boundary y §39 scope M7/M8)
- docs/M5_DESIGN.md (boundary del adapter y auth)
- docs/decisions/0006-agent-engine-and-opencode-adapter.md
- docs/decisions/0007-frontend-framework-and-tauri-boundary.md

## M7 status

M7 has NOT been designed or implemented. Fase actual: DESIGN_PENDING. No existe
`M7_DESIGN.md` ni ADR de proveedores/credenciales. No implementar hasta que el
diseño HIGH_ARCHITECTURE esté aprobado y persistido.