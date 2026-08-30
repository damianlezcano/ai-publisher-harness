# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: M7 — AI Provider Onboarding — **CLOSED / implementado**
- Current phase: IMPLEMENTED_CLOSED
- Current main commit: ver `git log -1` tras el commit de cierre de M7
- M1-M7: completados y cerrados
- verify: PASS — "M7 contract passed"
- git diff --check: limpio
- Worktrees: solo `main`; worktrees históricos de tareas M7 limpiados
- Siguiente milestone: M8 — Attachments / advanced resource UX (no iniciado)

## ADRs

- ADR-0001..0009: Accepted (ninguno reabierto).
- ADR-0008 (Accepted): provider onboarding vía API de integraciones de OpenCode — implementado.
- ADR-0009 (Accepted): selección simplificada de modelo + política free-model — implementado.

## Decisiones de arquitectura (aprobadas e implementadas)

- La app NO es un segundo provider framework. OpenCode sigue siendo la capa de
  provider integrations / auth mechanisms / auth flows / model discovery /
  connection state. Nuestra app es un **conductor fino y seguro**:
  Frontend → Application facade → `ProviderConnectorPort` → OpenCode API.
- **Credential ownership**: pertenece a OpenCode dentro de su config/data
  aislada. M7 NO tiene CredentialStore propio. La app: recibe el secreto una vez
  (frontend), lo envía por loopback al endpoint de integración de OpenCode,
  nunca lo persiste en project files, nunca lo retorna al frontend, nunca lo
  loggea, nunca lo incluye en prompts ni en diagnostic bundles. El frontend sólo
  recibe estados (`configured=true`, `connected=true/false`), nunca el secreto.
- **ChatGPT/accuracy**: ChatGPT Plus/Pro puede usar OAuth de OpenCode si está
  disponible. No confundir suscripción ChatGPT con OpenAI API billing/key.
  Gemini consumer subscription NO es una API key. DeepSeek: seguir el auth
  mechanism real de OpenCode. UI precisa pero simple.
- **Free-model policy**: modelos zero-credential/cost:0 de OpenCode como opción
  inicial de bajo roce. No prometer permanencia, no cambiar silenciosamente a
  modelos pagos ni de provider; si desaparecen, informar claramente.
- **Shared OpenCodeBackend**: extraído a `project-opencode`. Un único
  backend `opencode serve` compartido por AgentEngine + ProviderConnector. NO
  duplicar procesos. Cualquier mutación de credencial reinicia el backend;
  la selección de modelo aplica por prompt sin reiniciar.

## OpenCode research (instalado/testeado)

- Versión instalada: `1.18.25` (rango soportado M5: `>=1.18 <2`).
- Credenciales: `<data>/opencode/auth.json` (0600), write/delete-only por API.
- `opencode auth` == `opencode providers` (CLI interactivo; la app usa el HTTP API).
- Endpoints usados:
  - `GET /api/integration` → `IntegrationInfo{id,name,methods,connections}`.
  - `POST /api/integration/{id}/connect/key` body `{key,label?}` → 204.
  - `POST /api/integration/{id}/connect/oauth` body `{methodID,inputs,label?}` → `IntegrationAttempt{attemptID,url,instructions,mode,time}`.
  - `GET /api/integration/attempt/{id}` → `pending|complete|failed|expired`.
  - `POST /api/integration/attempt/{id}/complete` body `{code?}` → 204.
  - `DELETE /api/integration/attempt/{id}` (cancel), `DELETE /api/credential/{id}` (remove).
  - `GET /api/model` → `ModelV2Info{id,providerID,family,name,cost,status,enabled,limit}`.
  - `GET /config/providers` → providers + `default` map.
  - `POST /api/session/{id}/model` → switch model.
  - NO existe `GET /api/credential/{id}` (sin read-back del secreto).

## Provider auth findings

- `openai`: `key`, `env`, `oauth` (chatgpt-browser, chatgpt-headless).
- `opencode` (OpenCode Zen): `key` (service account), `env`, `oauth` (device).
- `google`: `key`+`env` (NO oauth) — API key only.
- `deepseek`: `key`+`env` — API key only.
- `anthropic`: `key`+`env`.
- 212 integrations totales; `env` no se ofrece en UX M7.
- Modelos gratis: provider `opencode` (`*-free`, `apiKey:"public"`, `cost:0`).

## Task graph (M7_DESIGN §27-30) — completado

| # | Task | Level | Estado |
| --- | --- | --- | --- |
| 1 | Extraer `project-opencode` (`OpenCodeBackend`) + migrar `OpenCodeAgentEngine`; M1-M6 verdes | HIGH_CODING | DONE |
| 2 | `project-provider`: port + models + errors + `SecretString` + `FakeProviderConnector` | MEDIUM | DONE |
| 3 | `OpenCodeProviderConnector` adapter + extender `fake_opencode_server` | HIGH_CODING | DONE |
| 4 | `ProviderService`: selección/settings + test-connection + restart-on-mutation | HIGH_CODING | DONE |
| 5 | `project-app`: backend compartido, facade provider/model + DTOs + error map | MEDIUM_HIGH | DONE |
| 6 | Tauri commands + capabilities + state | MEDIUM | DONE |
| 7 | Frontend "Conectá tu IA" + selector de modelo + tests | MEDIUM | DONE |
| 8 | Security + lifecycle tests + verify + smoke | MEDIUM/HIGH | DONE |
| 9 | Gate/docs/ADR + verify | HIGH_ARCHITECTURE | DONE (cierre) |

- Tareas 1-8 aterrizaron en `main`; la tarea 9 cierra el milestone en el repo.
- M7 NO implementó alcance M8 (attachments / advanced resource UX: clipboard
  image paste, rich previews, embedded web preview). Ese boundary queda intacto.

## Model policy (implementación)

- IMPLEMENTATION_ORCHESTRATOR: `opencode-go/deepseek-v4-flash`.
- LOW: Cursor Composer 2.5 → fallback `opencode-go/mimo-v2.5`.
- MEDIUM: `opencode-go/deepseek-v4-flash` → Composer → `opencode-go/qwen3.8-flash`.
- MEDIUM_HIGH: Cursor Grok 4.6 medium → DeepSeek V4 Flash → Qwen3.8 Max.
- HIGH_CODING: Cursor Grok 4.6 medium → Kimi K2.7 Code.
- HIGH_ARCHITECTURE: fresh DeepSeek V4 Pro session only.
- V4 Pro queda reservado para HIGH_ARCHITECTURE escalation únicamente.

## Documents required by fresh implementation session

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/M7_DESIGN.md (diseño aprobado e implementado, 35 secciones)
- docs/decisions/0008-*, 0009-* (Accepted)
- docs/M5_DESIGN.md, docs/M6_DESIGN.md (boundaries, adapter/backend)
- docs/VERIFY.md (§M7), docs/WORKTREES.md, docs/MULTI_AGENT_WORKFLOW.md
- docs/CURRENT_CHECKPOINT.md (este documento)
