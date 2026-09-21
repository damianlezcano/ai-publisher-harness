# Auditoría de arquitectura (solo lectura)

**STATUS: HISTORICAL SNAPSHOT — 2026-09-18 (pre Phase 4/5/6).**

This document captured the tree as of that date. It is **not** the live
architecture. Do not treat later sections (especially J, K, L, M) as current
contracts. The live model is `docs/ARCHITECTURE.md` (Phases 1–6) plus the
code/tests.

Superseded claims in this snapshot include:

- OrdinaryChat with `knowledge.sqlite` still running hybrid retrieval.
- Thematic / exhaustive LLM synthesis writing to the conversational OpenCode session.
- Session selection described only as `retrieval_mode == "normal"`.
- `should_classify` omitting `prior_referent_kind`.
- Provider-call row A' mixing OrdinaryChat with hybrid retrieval.

**Phase 4+ (live):** serialized Knowledge evidence (`normal` / `exhaustive` /
`thematic`) uses ephemeral `open_fresh_session` plus bounded EducAI visible
history. OrdinaryChat remains `open_session` with **zero** Knowledge
processing. Classifier / K6 / PerItem remain scratch. See
`docs/ARCHITECTURE.md` § OpenCode session frontiers and § Phase 6.

Conversación, OpenCode, Knowledge, classifier, scratch sessions y estrategias de resolución.

Reconstruido desde código y tests del working tree local **el 2026-09-18**. No se modificó el producto al producir este informe. No hubo llamadas al proveedor ni AppImage.

Fecha de auditoría: 2026-09-18.

**Phase 4 (2026-09-18):** session policy was corrected after this snapshot.
Knowledge synthesis with serialized evidence (`normal` / `exhaustive` /
`thematic`) uses ephemeral `open_fresh_session` plus bounded EducAI visible
history. OrdinaryChat remains `open_session`. Classifier / K6 / PerItem remain
scratch. See `docs/ARCHITECTURE.md` § OpenCode session frontiers.

---

## A. Executive summary

EducAI no tiene un objeto llamado `Strategy`. El turno entra por `send_message` / Tauri `agent_send`, se **persiste** el mensaje de usuario, se resuelve un `BoundRoute` (`resolve_intent_with`) y `dispatch_route` elige un **motor**: chat de `project-agent`, respuesta local, agregado per-item, o K6.

El classifier semántico **no** corre en todo prompt: `should_classify` es `has_persisted_knowledge || current_turn_attachment_count > 0`. Sin índice Knowledge y sin adjuntos, “hola” va a chat normal **sin** scratch ni `prompt_async` de clasificación.

Con Knowledge persistido, **sí** hay classifier remoto (scratch) **incluso para “Hola”**. Eso está testado como comportamiento elegido (`knowledge_plus_hollow_chat_still_invokes_the_classifier_once`).

Classifier, summarizer/K6/per-item y chat **no comparten `session_id` de OpenCode**. Comparten el mismo proceso `opencode serve` (`OpenCodeBackend`), no el mismo transcript.

La respuesta final de RAG “normal” **tampoco** reutiliza la sesión de conversación: `retrieval_mode == "normal"` abre `open_fresh_session` y no pisa el cache de chat.

**ROOT CAUSE OF EMPTY SCRATCH ASSISTANT: NOT YET PROVEN**

---

## B. Diagrama real del flujo general

```
UI (api.ts invoke agent_send)
  → Tauri commands.rs::agent_send
      → AppState::send_message_persist
          → resolve_agent_inputs
              → resolve_route
                  → knowledge_routing_context  (flags: sqlite existe, counts, adjuntos)
                  → resolve_intent_with
                      1. resolve_followup?  (si no hay adjuntos de este turno) → BoundRoute, SIN classifier
                      2. detect_creation_intent + creation_turn? → BoundRoute, SIN classifier
                      3. SemanticIntentClassifier.classify(ClassifierInput)
                           si !should_classify → DeterministicAdapter LOCAL (bypass)
                           si should_classify → OpenCodeIntentClassifier (scratch) o fallback
                      4. clamp_summary_depth (detector local compact vs K6)
                  → apply_route (prepare_knowledge / creation / no-op K6-per-item)
              → append_user_message (historial EducAI, no OpenCode)
      → dispatch_message_run → dispatch_route(intent)
          WholeCorpusSummary / PerSourceSummary → send_summary_run (K6, scratch por nodo)
          PerItemBatchAggregate / BatchSummary (fresco) → per-item aggregado (scratch por batch)
          follow-up per-item → send_message_run → complete_per_item_summary_turn
          resto → send_message_run
              si local_answer → complete_local_knowledge_turn (0 inferencia de respuesta)
              si no → AgentService::run → OpenCodeAgentEngine
                  si knowledge.retrieval_mode == "normal" → open_fresh_session
                  si no → open_session (cache 1 por project_id)
                  prompt_async + poll finish==stop
              → append_assistant_message
```

Punto de entrada de tests: `AppState::send_message` = persist + dispatch en el mismo hilo.

Modelo: el de la conversación (`inputs.model` / `selected_model_ref`); classifier y summarizer lo **pinnean** en `prompt_async.model` si está presente.

---

## C. Flujo real SIN Knowledge

Definición en código de “sin Knowledge relevante para clasificar”:

```rust
// crates/project-app/src/classifier/mod.rs
pub fn should_classify(input: &ClassifierInput) -> bool {
    input.has_persisted_knowledge || input.current_turn_attachment_count > 0
}
```

`has_persisted_knowledge` = existe `projects/{id}/knowledge/knowledge.sqlite` (`knowledge_routing_context`).

### Caso “hola”, sin adjuntos, **sin** sqlite Knowledge

1. `should_classify` = false → **no** se llama al delegate OpenCode (`semantic_composite_bypasses_without_a_knowledge_cue`).
2. `deterministic_classify`: sin Knowledge persistido, si no hay thematic/summary gate → `Intent::OrdinaryChat`.
3. `prepare_knowledge_context`: si no hay sqlite, return `(None, None, None)` inmediato.
4. `dispatch_route` → `send_message_run` → `AgentService::run`.
5. `knowledge` es `None` → `fresh_session = false` → `open_session` (sesión de proyecto, reutilizada).
6. Una `prompt_async` de chat. Sin scratch de classifier.

**SIN KNOWLEDGE ACTUAL (sin índice y sin adjuntos):**

```
"hola"
 → persist user message
 → classifier LOCAL (bypass)  [0 inferencias]
 → OrdinaryChat
 → project-agent open_session (reuso)
 → 1× prompt_async  [chat]
 → persist assistant
```

Respuestas inequívocas para **ese** caso:

| # | Pregunta | Respuesta |
|---|----------|-----------|
| 1 | ¿Entra directo a project-agent? | Sí, tras routing local. |
| 2 | ¿Classifier antes? | Sí, el composite; **no** el remoto. |
| 3 | ¿Local / remoto / scratch / project? | Local `DeterministicAdapter`. |
| 4 | ¿Sesión OpenCode extra? | No de classifier. Chat usa/crea la de proyecto. |
| 5 | ¿prompt_async de classifier? | No. |
| 6 | ¿Inferencia remota antes del chat? | No. |
| 7 | ¿Inferencias remotas? | **1** (chat), si hay backend. |
| 8 | ¿Evitar classifier remoto? | `!has_persisted_knowledge && attachment_count==0`, o follow-up/creation pre-gates. |
| 9 | ¿Fast path trivial? | No hay fast path de “hola” sin LLM. El bypass solo evita **clasificar**. El chat igual llama al modelo. |

### Matiz crítico

Si **sí** hay Knowledge persistido, “hola” **no** es este camino. Ver sección K y el test `knowledge_plus_hollow_chat_still_invokes_the_classifier_once`.

---

## D. Flujo real CON Knowledge

Ejemplo: *“¿Qué se habló sobre Kubernetes en las reuniones?”* con corpus READY.

1. Sqlite existe → `should_classify` true.
2. Follow-up/creation no aplican (turno fresco, no es creación).
3. `OpenCodeIntentClassifier::run_session`: `POST /session?directory=<data>/opencode-scratch` + `scratch_tool_free_permission()`, una `prompt_async` con JSON de intents, poll, abort. Payload: prompt + flags estructurales; **sin** cuerpos de documentos (test `classifier_session_is_separate_and_receives_no_document_bodies`).
4. Modelo/provider: los de la conversación (`with_model`), no un modelo de classifier distinto.
5. Output: `ClassifierDecision` `{intent, modifiers, confidence}` validado; `reason_code` Rust.
6. Routing: el intent manda. Con adapter determinístico, “se habló de X” + alcance de corpus cae en `RetrievalIntent::CorpusExhaustive` (`retrieval_intent.rs`). Un classifier semántico **puede** devolver otro intent; el clamp de profundidad solo afecta compact vs K6, no exhaustive vs RAG.
7. `apply_route` → `prepare_exhaustive_knowledge_context`: scan local de todos los chunks READY (embeddings locales posibles).
   - Hits léxicos: evidencia acotada + **sí** va a `send_message_run` (LLM).
   - Cero hits + cobertura completa: `exhaustive_local_answer` → **sin** LLM de respuesta.
8. Si hay LLM: `retrieval_mode` exhaustive ≠ `"normal"` → **`open_session` reutilizada**, no fresh. Evidencia se serializa en el prompt (`serialize_knowledge_context`).
9. Respuesta: assistant EducAI persistido. Transcript OpenCode de classifier **abortado e independiente**.

**`classifier session_id == final answer session_id`?**

**No.** Evidencia: classifier siempre `POST /session` nuevo + abort; chat usa cache de `OpenCodeAgentEngine` o `open_fresh_session`. Test: `created_session_ids().len() == 1` para el FakeServer del **classifier** mientras el answer va por `FakeAgentEngine` / otro camino (`classifier_session_is_separate_and_receives_no_document_bodies`).

Para RAG `NormalSemantic`, el answer ni siquiera es la sesión de conversación cacheada: `fresh_session` si `retrieval_mode == "normal"` (`project-agent/src/service.rs`). Test: `fresh_sessions_never_reuse_the_project_conversation_cache`.

---

## E. Inventario de sesiones OpenCode

Un solo sidecar `opencode serve` (`OpenCodeBackend` clonado a agent, classifier, summarizer, provider). Varios **tipos de sesión HTTP**.

| Rol | Dónde se crea | Quién | Para qué | Lifecycle | Persistida en EducAI? | Reuso | Abort | directory | agent JSON | permissions | model | Transcript vs chat |
|-----|---------------|-------|----------|-----------|----------------------|-------|-------|-----------|------------|-------------|-------|-------------------|
| Conversación / project-agent | `OpenCodeAgentEngine::open_session` | `AgentService::run` si no fresh | Chat + creación con tools | Cache `HashMap<project_id, session_id>` hasta restart del backend | No (historial EducAI es `append_*_message`) | Sí, 1/proyecto | `cancel` de turno | `project/workspace` | no se setea | `external_directory` deny (tools del agent `build` siguen) | pin conversación | Continuidad OpenCode del proyecto |
| Knowledge QA “normal” | `open_fresh_session` | `AgentService` si `retrieval_mode=="normal"` | RAG top-k sin heredar historial de chat OpenCode | Turno; **no** reemplaza el cache de chat | No | No | fin de poll / cancel | mismo `workspace` | no | igual deny external_directory | pin conversación | Independiente del chat cacheado |
| Classifier scratch | `OpenCodeIntentClassifier::run_session` | `semantic_classifier` por turno | Clasificar intent | Crear → 1 prompt → poll → **abort siempre** | No | Nunca | Sí | `data_dir/opencode-scratch` | no | tool-safe scratch (`**` execution deny + `external_directory`; no global `*`) | mismo modelo conversación | Independiente |
| Summarizer / K6 / per-item scratch | `OpenCodeRemoteSummarizer::summarize` | `send_summary_run`, `per_item_summarizer` | Un nodo o un batch de síntesis | Una sesión **por** `summarize()` → abort | Nodos K6 en sqlite Knowledge, no como chat | Nunca entre nodos (test: distinct session ids) | Sí | `opencode-scratch` | no | mismo helper tool-safe scratch | pin conversación | Independiente; no turno de chat |
| Provider connection test | `OpenCodeProviderConnector::test_connection` | UI de modelo | Probar provider | Scratch dir temporal + sesión + abort | No | No | implícito al dropear dir | scratch de provider | no | `{}` vacío en create | el modelo bajo test | Fuera del turno |

No hay “clones” de session_id. No hay `parentID` compartido entre classifier y chat.

---

## F. Session continuity / session_id

| Concepto | Realidad |
|----------|----------|
| Conversación EducAI (UI) | `project_id` + mensajes persistidos. Una interacción de producto. |
| OpenCode chat cache | Un `session_id` por proyecto, reusado en chat/creación **salvo** RAG `normal`. |
| Classifier | Nuevo `session_id` cada clasificación; abort. |
| K6/per-item | Nuevo `session_id` cada inferencia de síntesis. |
| “Volver a la sesión principal” | **Ninguna** Strategy de resumen vuelve a `prompt_async` en la sesión de chat. RAG exhaustive/thematic/creation **sí** usan `open_session` (chat cache). RAG `normal` **no**. |
| Transcript compartido | No entre scratch y chat. Thematic/exhaustive **inyectan evidencia en el prompt** de la sesión de chat reutilizada, así que **sí** pueden contaminar el historial OpenCode de esa sesión. |
| parentID | Solo correlación **dentro** de una scratch (user de ese prompt). |

Separación:

- **Misma interacción conceptual:** sí (un turno EducAI, un `turn_id` de mensaje usuario).
- **Misma `session_id` OpenCode:** **no** para classifier vs respuesta; **no** para K6 vs chat; **a veces** para exhaustive/thematic vs chat previo.

---

## G. Scratch architecture

Implementación: no hay factory única. Dos sitios de producción de turno:

- `classifier/opencode.rs` `run_session`
- `summarize.rs` `OpenCodeRemoteSummarizer::summarize`

Contrato común: `with_directory_query("/session", scratch_dir)`, `scratch_tool_free_permission()`, `prompt_async`, poll `GET /message`, `detect_terminal_assistant` + `ScratchCompletionTracker`, `POST abort`.

**Por qué existen (evidencia del repo):**

1. **No ser la sesión de chat / no turno visible** — `summarize.rs`; `KNOWLEDGE_ARCHITECTURE.md` §31; classifier: “never the project's chat session”, “never visible as a normal user chat turn”.
2. **Stateless / no heredar transcript** — docs del classifier.
3. **Tool-safe** — explicit coding-tool execution denies (pattern `**`, so OpenCode 1.18.25 does not hide tools from the model) plus `external_directory` deny; tests `scratch_session_matches_ordinary_chat_request_contract`. A global `*` deny is **not** used: on OpenCode 1.18.25 that ruleset produces an assistant completed with no text. ADR-0006 describes deny of `external_directory` for the **agent**.
4. **Lifecycle acotado + abort** — tests de abort en timeout/error/éxito.
5. **K6: un nodo ≠ un mensaje de chat** — “never creates a user-visible chat turn”.

¿Obligatoriedad técnica de OpenCode? El repo **no** demuestra que OpenCode exija scratch. Demuestra una **decisión de producto/implementación** (aislamiento + tools). Reemplazable en principio por otro aislamiento; no hay prueba de imposibilidad.

**RATIONALE FOR SCRATCH SESSIONS:** aislamiento de transcript, no contaminar chat, tool-free, lifecycle — **documentado y testeado**.

Que sea la única forma posible en OpenCode: **NOT PROVEN FROM REPOSITORY**.

---

## H. Classifier architecture

- Trait: `IntentClassifier::classify`.
- Composite producción: `SemanticIntentClassifier` (gate + umbral 0.5 + fallback).
- Delegate: `OpenCodeIntentClassifier` si `classifier_backend` Some; tests pueden inyectar.
- Fallback: `DeterministicAdapter` / `deterministic_classify`.
- Precedencia **antes** del classifier: follow-up, luego creation lexical.
- **Cuándo remoto:** `should_classify` y backend presente.
- **Cuándo no:** sin Knowledge y sin adjuntos; o pre-gates; o `semantic_classifier` None → `resolve_intent` solo determinístico.
- Input: `ClassifierInput` (prompt + counts/flags). Output: `ClassifierDecision` → `BoundRoute`.
- Efecto: `dispatch_route` + `prepare_knowledge_context` **consumen el intent**; no re-leen el prompt (salvo clamp de summary depth y detectors **dentro** del seam de intent).

---

## I. Strategy mapping

No hay clases `*Strategy`. Mapeo:

| Conceptual | Nombre real `Intent` | Módulo | Trigger | Local | Inferencia respuesta | Sesión respuesta |
|------------|----------------------|--------|---------|-------|----------------------|------------------|
| SinKnowledge | `OrdinaryChat` | `dispatch` → `send_message_run` | bypass o classifier | no RAG | 1 chat | `open_session` |
| SemanticRag | `NormalSemantic` | `prepare_knowledge_context` K3/K4 | classifier / deterministic retrieval | hybrid_search + assemble | 1 chat | **`open_fresh_session`** |
| Exhaustive | `CorpusExhaustive` | `prepare_exhaustive_*` | presence cues / classifier | scan completo; negativo local | 0 o 1 chat | `open_session` si LLM |
| PerItemBatch | `PerItemBatchAggregate` / `BatchSummary` | `per_item.rs` + `complete_per_item_*` | summary compact / follow-up | reps locales/reuso K6 doc | `ceil(n/100)` scratches (cap 100 items, 72kB) | scratch, no chat |
| K6 | `WholeCorpusSummary` / `PerSourceSummary` | `send_summary_run` + `project-knowledge` plan | summary/análisis deep | plan/cache nodos | N docs + batches + global (reuso Ready) | scratch por nodo |
| Inventory | `KnowledgeInventory` | `prepare_inventory_*` | inventory cues / classifier | metadata sqlite | **0** | ninguna |
| Thematic | `CorpusThematic` | `prepare_thematic_*` | thematic cues / classifier | temas locales; evidencia | 1 chat (síntesis) | `open_session` (`retrieval_mode=thematic`) |
| SelectedBatchAggregate | `BatchSummary` | mismo per-item | resumen + adjuntos turno | igual per-item | batches | scratch |
| SelectedPerSource | `PerSourceSummary` | K6 selected set | análisis/deep per archivo | K6 scoped | K6 nodes | scratch |
| Creation | `Creation` | agent + knowledge document-wide | creation gate / classifier | a veces aclaración local | 1 agent | `open_session` |

Thematic con `ready < 2` **cae a K3/K4** (`prepare_knowledge_context`).

---

## J. Remote inference matrix

“Inferencia” = `prompt_async` de modelo. Create-session/abort/GET message = control plane.

Asume producción: `classifier_backend` y `summarizer_backend` Some, modelo seleccionado. Classifier cuenta si `should_classify`.

| Caso | Classifier scratch | Retrieval local | Summarizer/K6 scratch | Chat/agent | Total típico |
|------|--------------------|-----------------|----------------------|------------|--------------|
| A “hola” **sin** Knowledge | 0 | 0 | 0 | 1 | **1** |
| A' “hola” **con** Knowledge persistido | **1** | hybrid (embeddings locales, no LLM) | 0 | 1 (y `fresh_session` si acaba `OrdinaryChat`/`NormalSemantic` con `retrieval_mode=normal`) | **2** |
| B “¿Qué es Kubernetes?” sin Knowledge | 0 | 0 | 0 | 1 | **1** |
| B' igual **con** Knowledge | 1 | hybrid | 0 | 1 fresh si NormalSemantic | **2** |
| C “¿Qué se habló sobre Kubernetes?” con corpus | 1 | exhaustive local | 0 | 0 si negativo completo; 1 si hay hits | **1 o 2** |
| D “¿En qué archivos aparece 'depend on'?” | 1 | exhaustive | 0 | 0 o 1 igual | **1 o 2** |
| E resumí cada archivo, **1** archivo | 1 (si Knowledge/adjuntos) | reps locales | **1** batch | 0 | **2** (o 1 si no clasifica) |
| F igual, N archivos | 1 | reps | `ceil(N/100)` acotado por bytes | 0 | **1 + batches** |
| G análisis profundo por documento | 1 | plan K6 | **hasta** `#docs + #batches + 1 global` (menos nodos Ready reusados); +1 retry `InvalidOutput` por nodo | 0 chat | **1 + K6 remotes** |

Embeddings ONNX = inferencia **local**, no provider OpenCode.

---

## K. Knowledge detection semantics

El código **distingue** “existe índice” vs “este turno es operación Knowledge”, **pero el trigger del classifier remoto no**.

| Señal | Qué mide | Dónde |
|-------|----------|--------|
| `knowledge.sqlite` | Knowledge **existe** | `has_persisted_knowledge` |
| Adjuntos del turno | Material **esta** vez | `current_turn_attachment_count` |
| `should_classify` | ¿Correr LLM classifier? | OR de las dos anteriores |
| `Intent::*` | ¿Usar motor Knowledge? | classifier + pre-gates |
| `prepare_knowledge_context` | Si sqlite existe y el intent no es K6/per-item, **igual** prepara (RAG default para OrdinaryChat) | `apply_route` `_` |

“Este turno necesita Knowledge” **no** es un predicado único. Hay:

1. Gate estructural (existe corpus o adjuntos) → classifier remoto.
2. Intent → qué motor.
3. Sqlite presente + OrdinaryChat → **igual** corre hybrid_search (“hola” sobre el corpus).

Eso **no** es “el turno necesita Knowledge”; es “hay corpus, entonces se consulta”.

---

## L. Comparación contra el modelo conceptual

**Principio A — Sin Knowledge: directo a LLM, sin classifier Knowledge innecesario**

**CUMPLE PARCIALMENTE.**

Sin sqlite y sin adjuntos: cumple (bypass).

Con sqlite, “hola”/“qué es un pod” **sí** disparan classifier scratch (test hueco). El modelo conceptual habla de “el turno no necesita Knowledge”; el código usa “Knowledge existe”.

**Principio B — Con Knowledge: clasificar → Strategy → local → LLM si hace falta**

**CUMPLE PARCIALMENTE.**

La forma es esa (`BoundRoute` → prepare → motor). No hay objeto Strategy. Classifier y preparación son pasos separados. RAG normal **no** “vuelve” a la sesión de conversación OpenCode.

**Principio C — Clasificación y respuesta, misma conversación; ¿misma session_id?**

**CUMPLE PARCIALMENTE** (conversación EducAI) / **NO CUMPLE** (session_id OpenCode).

Mismo `project_id`/`turn_id`. `session_id` distintos, demostrado.

**Principio D — Strategy local sin segunda inferencia**

**CUMPLE.** Inventory; exhaustive negativo completo; aclaración creation; no-selection per-source. (El classifier remoto, si el gate está on, **ya** fue una inferencia previa.)

**Principio E — RAG no obligatorio para toda consulta Knowledge**

**CUMPLE.** Inventory, exhaustive local, thematic prep, per-item, K6 son otros caminos. OrdinaryChat con sqlite **sí** entra a hybrid por default.

**Principio F — Resumen breve vs análisis profundo**

**CUMPLE.** `PerItemBatchAggregate` vs `PerSourceSummary`/`WholeCorpusSummary`; `clamp_summary_depth` impide que el classifier suba compact a K6.

---

## M. Divergencias observadas (hechos)

1. El gate del classifier es **existencia de Knowledge**, no **necesidad del turno**.
2. Classifier scratch ≠ sesión de respuesta ≠ (a menudo) sesión de chat.
3. RAG `normal` usa **otra** sesión agent (`open_fresh_session`) para no pisar el cache de chat.
4. K6 y per-item **nunca** contestan por la sesión de conversación.
5. Thematic/exhaustive **sí** pueden escribir en la sesión de chat reutilizada.
6. OrdinaryChat con sqlite todavía hace retrieval K3/K4.
7. Un proceso OpenCode, muchas sesiones; “misma conversación” ≠ mismo `session_id`.

---

## N. Unknowns

- Si en Fedora/AppImage el usuario siempre tiene sqlite tras el primer import (entonces “hola” siempre clasifica).
- Qué intent devuelve el LLM real para “hola” con corpus (OrdinaryChat vs NormalSemantic); el test inyecta OrdinaryChat.
- Si OpenCode mezclaría tools/transcript si se clasificara en la sesión de chat (no hay prueba empírica en repo).
- Causa del assistant scratch vacío.

---

## O. Root cause scratch vacío

**ROOT CAUSE OF EMPTY SCRATCH ASSISTANT: NOT YET PROVEN**

---

## P. Conclusión

1. **¿“hola” puede ejecutar classifier remoto antes del chat?**
   **Sin Knowledge persistido y sin adjuntos: no.**
   **Con Knowledge persistido: sí, una scratch, por diseño actual.**

2. **¿Classifier y respuesta final usan la misma `session_id`?**
   **No.**

3. **¿Summarizer y classifier comparten `session_id`?**
   **No.** Cada `summarize()` y cada `classify()` crean y abortan la suya.

4. **¿Scratch es arquitectónicamente obligatorio según el repo?**
   **No está demostrado como único mecanismo posible en OpenCode.** Sí está implementado y documentado para aislamiento, tool-free y no emitir turnos de chat internos.

5. **¿Coincide con el modelo conceptual?**
   **Parcialmente.** La forma “intent → motor local o LLM” existe. Falla o se desvía en: classifier si el corpus existe; session_ids distintos; RAG normal en sesión fresh; OrdinaryChat que igual retrieva.

6. **Divergencias principales:** gate `should_classify`; multiplex de sesiones OpenCode; `open_fresh_session` para RAG normal; síntesis K6/per-item fuera del chat.

READ-ONLY ARCHITECTURE AUDIT COMPLETE
