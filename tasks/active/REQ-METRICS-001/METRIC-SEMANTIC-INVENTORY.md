# Registro Semántico de Inventario de Métricas

- Requirement: `REQ-METRICS-001`
- Task: `TASK-METRICS-1`
- Base SHA: `f3450e1ac0053dd1bd160542354d825085793c7`
- Status: `IMPLEMENTING`

---

## 1. Superficies de exposición

Las métricas se persisten en `project.json` como `turnMetrics` en cada mensaje de usuario. Se exponen por tres superficies:

| Superficie | Archivo | Método |
|---|---|---|
| Compacta por respuesta | `app/src/components/AssistantMetrics.tsx` | Línea bajo la respuesta del asistente |
| Popover por respuesta | `app/src/components/AssistantMetrics.tsx` | Portal a `document.body`, botón ⓘ |
| Detalle de conversación | `app/src/components/ConversationMetrics.tsx` | Diálogo "Detalles de la conversación" |
| Acumulado conversación | `app/src/components/ConversationMetrics.tsx` | Subsección "Acumulado de la conversación" |

---

## 2. Matriz de campos — Telemetría del proveedor

| Campo JSON | Tipo TS | Tipo Rust | Fuente | Unidad | Alcance | Aplicabilidad | Agregación | Indisponibilidad | Disposición | Prueba exacta |
|---|---|---|---|---|---|---|---|---|---|---|
| `provider` | `string \| null` | `Option<String>` | Proveedor remoto | — | Turno | Todos los turnos con llamada al proveedor | Último no-nulo en acumulado | `null` → "No disponible" | Visible | `ConversationMetrics.test.tsx:279`, `app_facade.rs:151` |
| `model` | `string \| null` | `Option<String>` | Proveedor remoto | — | Turno | Todos los turnos con llamada al proveedor | Último no-nulo en acumulado | `null` → "No disponible" | Visible | `ConversationMetrics.test.tsx:279`, `app_facade.rs:151` |
| `inputTokens` | `number \| null` | `Option<u64>` | Proveedor remoto (`provider_actual` si `source == "provider_actual"`) | Tokens | Turno | Solo cuando `source == "provider_actual"` | Suma en acumulado (solo si `remoteCalls > 0` por turno; `null` si algún turno es `null`) | `null` → "Noponible" | Visible (solo `provider_actual`) | `AssistantMetrics.test.tsx:58`, `runtime_gate.rs:532` |
| `outputTokens` | `number \| null` | `Option<u64>` | Proveedor remoto (`provider_actual` si `source == "provider_actual"`) | Tokens | Turno | Solo cuando `source == "provider_actual"` | Suma en acumulado (mismo criterio que `inputTokens`) | `null` → "No disponible" | Visible (solo `provider_actual`) | `AssistantMetrics.test.tsx:58`, `runtime_gate.rs:532` |
| `cacheReadTokens` | `number \| null` | `Option<u64>` | Proveedor remoto (`provider_actual` si `source == "provider_actual"`) | Tokens | Turno | Solo cuando `source == "provider_actual"` | Suma en acumulado (mismo criterio que `inputTokens`) | `null` → "No disponible" | Visible (solo `provider_actual`) | `runtime_gate.rs:356`, `app_facade.rs:151` |
| `cacheWriteTokens` | `number \| null` | `Option<u64>` | Proveedor remoto (`provider_actual` si `source == "provider_actual"`) | Tokens | Turno | Solo cuando `source == "provider_actual"` | Suma en acumulado (mismo criterio que `inputTokens`) | `null` → "No disponible" | Visible (solo `provider_actual`) | `runtime_gate.rs:356`, `app_facade.rs:151` |
| `totalTokens` | `number \| null` | `Option<u64>` | Proveedor remoto | Tokens | Turno | Solo cuando `source == "provider_actual"` | Suma en acumulado (mismo criterio que `inputTokens`) | `null` → "No disponible" | Visible (solo `provider_actual`) | `runtime_gate.rs:404` |
| `costUsd` | `number \| null` | `Option<f64>` | Proveedor remoto (`provider_actual` si `source == "provider_actual"`) | USD | Turno | Solo cuando `source == "provider_actual"` | Suma en acumulado (solo si `remoteCalls > 0` por turno; `null` si algún turno es `null`) | `null` → "No disponible" | Visible (solo `provider_actual`) | `AssistantMetrics.test.tsx:326`, `ConversationDetails.test.tsx:320` |
| `turnDurationMs` | `number \| null` | `Option<u64>` | Backend (elapsed time del turno lógico) | Milisegundos | Turno | Todos los turnos completados | Suma TOTAL en acumulado (todos los turnos, sin condición `remoteCalls`) | `null` → "No disponible" | Visible | `AssistantMetrics.test.tsx:58`, `ConversationDetails.test.tsx:320` |
| `remoteCalls` | `number \| null` | `Option<usize>` | Backend (conteo de llamadas ejecutadas) | Conteo | Turno | Todos los turnos | Suma TOTAL en acumulado (todos los turnos) | `null` → "No disponible" | Visible | `inventory.rs:106`, `exhaustive_rag.rs:601` |
| `source` | `string \| null` | `Option<String>` | Backend (`"provider_actual"`, `"estimated"`, `"unavailable"`, `"local"`) | — | Turno | Todos los turnos | Derivado: `"provider_actual"` si algún turno lo tiene, sino `"unavailable"` | `null` → "No disponible" | Visible (categórico) | `runtime_gate.rs:532`, `ConversationMetrics.test.tsx:279` |

---

## 3. Matriz de campos — Knowledge (estimaciones locales)

| Campo JSON | Tipo TS | Tipo Rust | Fuente | Unidad | Alcance | Aplicabilidad | Agregación | Indisponibilidad | Disposición | Prueba exacta |
|---|---|---|---|---|---|---|---|---|---|---|
| `materialCount` | `number \| null` | `Option<usize>` | Backend ( Knowledge store) | Conteo | Turno | Turnos con Knowledge disponible | No acumulable (snapshot por turno) | `null` → "No disponible" | Visible | `AssistantMetrics.test.tsx:116` |
| `corpusBytes` | `number \| null` | `Option<u64>` | Backend ( Knowledge store) | Bytes | Turno | Turnos con Knowledge disponible | No acumulable (snapshot por turno) | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `corpusUtf8Chars` | `number \| null` | `Option<usize>` | Backend ( Knowledge store) | Caracteres | Turno | Turnos con Knowledge disponible | No acumulable (snapshot por turno) | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `corpusEstTokens` | `number \| null` | `Option<usize>` | Backend ( Knowledge store) | Tokens estimados | Turno | Turnos con Knowledge disponible | No acumulable (snapshot por turno) | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `retrievalCandidateCount` | `number \| null` | `Option<usize>` | Backend (fase de recuperación) | Conteo | Turno | Solo turnos con recuperación RAG/exhaustive/thematic | No acumulable | `null` → "No disponible" | Visible | `AssistantMetrics.test.tsx:116` |
| `selectedEvidenceCount` | `number \| null` | `Option<usize>` | Backend (fase de selección) | Conteo | Turno | Solo turnos con recuperación RAG/exhaustive/thematic | No acumulable | `null` → "No disponible" | Visible | `AssistantMetrics.test.tsx:116` |
| `selectedEvidenceBytes` | `number \| null` | `Option<usize>` | Backend (fase de selección) | Bytes | Turno | Solo turnos con recuperación RAG/exhaustive/thematic | No acumulable | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `selectedEvidenceUtf8Chars` | `number \| null` | `Option<usize>` | Backend (fase de selección) | Caracteres | Turno | Solo turnos con recuperación RAG/exhaustive/thematic | No acumulable | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `evidenceEstTokens` | `number \| null` | `Option<usize>` | Backend (fase de selección) | Tokens estimados | Turno | Solo turnos con recuperación RAG/exhaustive/thematic | No acumulable | `null` → "No disponible" | Visible | `AssistantMetrics.test.tsx:151` |
| `contextReductionPct` | `number \| null` | `Option<usize>` | Backend (persistido) o calculado: `min(100, max(0, (1 - evidenceEstTokens / corpusEstTokens) * 100))` | Porcentaje | Turno | Solo modos RAG (`normal`, `exhaustive`, `thematic`) | No acumulable | `null` → "No disponible" | Visible (solo modos RAG) | `AssistantMetrics.test.tsx:116`, `ConversationDetails.test.tsx:279` |
| `semanticProviderState` | `string \| null` | `Option<String>` | Backend (estado semántico del proveedor) | — | Turno | Turnos con Knowledge disponible | No acumulable | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `requestPreparationMs` | `number \| null` | `Option<u64>` | Backend (duración de preparación) | Milisegundos | Turno | Turnos con Knowledge disponible | No acumulable | `null` → "No disponible" | Visible | `runtime_gate.rs:404` |
| `retrievalMode` | `string \| null` | `Option<String>` | Backend (`"normal"`, `"exhaustive"`, `"thematic"`, `"local_inventory"`, `"creation_from_material"`, etc.) | — | Turno | Todos los turnos | No acumulable | `null` → "No disponible" | Visible | `AssistantMetrics.test.tsx:349`, `ConversationDetails.test.tsx:154` |
| `localMode` | `string \| null` | `Option<String>` | Backend (`"inventory"`, `"creation_from_material"`, `"per_source_no_selection"`, etc.) | — | Turno | Solo turnos sin llamada al proveedor | No acumulable | `null` → "No disponible" | Visible | `inventory.rs:106`, `AssistantMetrics.test.tsx:116` |
| `exhaustiveCoverage` | `string \| null` | `Option<String>` | Backend (cobertura exhaustiva) | — | Turno | Solo modo exhaustive | No acumulable | `null` → "No disponible" | Visible | `exhaustive_rag.rs` (múltiples tests) |
| `eligibleMaterials` | `number \| null` | `Option<usize>` | Backend (materiales elegibles) | Conteo | Turno | Modos exhaustive/thematic | No acumulable | `null` → "No disponible" | Visible | `ConversationDetails.test.tsx:154` |
| `materialsInspected` | `number \| null` | `Option<usize>` | Backend (materiales inspeccionados) | Conteo | Turno | Solo modo exhaustive | No acumulable | `null` → "No disponible" | Visible | `ConversationDetails.test.tsx:154` |
| `chunksInspected` | `number \| null` | `Option<usize>` | Backend (fragmentos inspeccionados) | Conteo | Turno | Solo modo exhaustive | No acumulable | `null` → "No disponible" | Visible | `ConversationDetails.test.tsx:154` |
| `lexicalHits` | `number \| null` | `Option<usize>` | Backend (coincidencias léxicas) | Conteo | Turno | Solo modo exhaustive | No acumulable | `null` → "No disponible" | Visible | `exhaustive_rag.rs` (múltiples tests) |
| `semanticHits` | `number \| null` | `Option<usize>` | Backend (coincidencias semánticas) | Conteo | Turno | Solo modo exhaustive | No acumulable | `null` → "No disponible" | Visible | `exhaustive_rag.rs` (múltiples tests) |
| `sourceNames` | `string[]` (optional) | `Vec<String>` | Backend (nombres de fuentes para mostrar) | — | Turno | Turnos con fuentes grounded | No acumulable | `[]` → sin fuentes | Visible (solo en popover, no en compacta) | `AssistantMetrics.test.tsx:107`, `turnMetricsBinding.test.ts:93` |

---

## 4. Matriz de campos — Backend-only (serializados en JSON, no en tipo TS frontend)

Estos campos existen en `TurnMetricsView` (Rust) y se serializan en `project.json`, pero el tipo TypeScript `TurnMetrics` no los incluye. Son ignorados por el frontend.

| Campo JSON | Tipo Rust | Fuente | Disposición |
|---|---|---|---|
| `contextualFollowup` | `Option<bool>` | Backend (seguimiento contextual) | Interno |
| `referentType` | `Option<String>` | Backend (tipo de referente) | Interno |
| `referentCount` | `Option<usize>` | Backend (conteo de referentes) | Interno |
| `originTurnId` | `Option<String>` | Backend (ID del turno de origen) | Interno |
| `baseIntent` | `Option<String>` | Backend (intención base) | Interno |
| `turnKind` | `Option<String>` | Backend (tipo de turno) | Interno |

---

## 5. Matriz de campos — Acumulado de conversación (`ConversationUsageTotals`)

El acumulado se calcula en backend (`accumulated_conversation_usage` en `app.rs`). El frontend recibe `ConversationUsageTotals` y lo renderiza como `SessionUsage`.

| Campo JSON | Agregación | Condición | Prueba exacta |
|---|---|---|---|
| `provider` | Último no-nulo (walk reverso) | — | `app_facade.rs:151` |
| `model` | Último no-nulo (walk reverso) | — | `app_facade.rs:151` |
| `inputTokens` | Suma | Solo turnos con `remoteCalls > 0`; `null` si algún turno incluido es `null` | `app_facade.rs:151` |
| `outputTokens` | Suma | Solo turnos con `remoteCalls > 0`; `null` si algún turno incluido es `null` | `app_facade.rs:151` |
| `cacheReadTokens` | Suma | Solo turnos con `remoteCalls > 0`; `null` si algún turno incluido es `null` | `app_facade.rs:151` |
| `cacheWriteTokens` | Suma | Solo turnos con `remoteCalls > 0`; `null` si algún turno incluido es `null` | `app_facade.rs:151` |
| `totalTokens` | Suma | Solo turnos con `remoteCalls > 0`; `null` si algún turno incluido es `null` | `app_facade.rs:151` |
| `costUsd` | Suma (f64) | Solo turnos con `remoteCalls > 0`; `null` si algún turno incluido es `null` | `app_facade.rs:151` |
| `turnDurationMs` | Suma TOTAL | Todos los turnos (sin condición `remoteCalls`) | `app_facade.rs:151` |
| `remoteCalls` | Suma TOTAL | Todos los turnos | `app_facade.rs:151` |
| `source` | Derivado | `"provider_actual"` si algún turno lo tiene, sino `"unavailable"` | `ConversationMetrics.test.tsx:279` |

**Excluido intencionalmente:** campos de Knowledge (corpus, evidencia, modo, etc.) — son snapshots por turno, no acumulables.

---

## 6. Reglas de disponibilidad por tipo de turno

| Tipo de turno | `remoteCalls` | Provider tokens | Knowledge fields | `localMode` |
|---|---|---|---|---|
| Chat normal (con proveedor) | `1` | Disponibles (`source == "provider_actual"`) | Disponibles si hay Knowledge | — |
| Chat normal (sin proveedor) | `0` | `null` (No disponible) | Disponibles si hay Knowledge | `"creation_from_material"` o valor del backend |
| Inventario local | `0` | `null` (No disponible) | `candidateCount=0`, `evidenceCount=0`, corpus disponible | `"inventory"` |
| Per-source sin selección | `0` | `null` (No disponible) | `candidateCount=0`, `evidenceCount=0` | `"per_source_no_selection"` |
| K6 resumen | `report.remote_calls` (puede ser >1) | Disponibles (`source == "provider_actual"`) | Corpus disponible, evidencia `null` | Valor del backend |
| Per-item resumen | `accounting.remote_calls` (puede ser >1) | Disponibles (`source == "provider_actual"`) | Corpus disponible, evidencia `null` | Valor del backend |
| Turno fallido/cancelado | — | No se renderiza | No se renderiza | — |
| Turno legacy (sin `turnId`) | — | Solo si 1 asistente exacto con status `"ok"` | Solo si 1 asistente exacto | — |

---

## 7. Regla de binding (asistente → turno)

- **Primario:** Asistente → `turnId` (= id del mensaje de usuario) → `turnMetrics`.
- **Fallback legacy:** Solo si el asistente no tiene `turnId` y hay exactamente un asistente con status `"ok"` después del usuario.
- **Fallido/cancelado:** Nunca se renderizan métricas.
- **Ambigüedad:** Falla cerrado a sin métricas (nunca toma prestado de un asistente adyacente).

**Pruebas:** `turnMetricsBinding.test.ts` — M1 a M10.

---

## 8. Disposición final por campo

### Visible — compacta (bajo la respuesta)
- `turnDurationMs` (si no es `null`)
- `inputTokens` / `outputTokens` (solo si `source == "provider_actual"`)
- `contextReductionPct` (solo modos RAG: `normal`, `exhaustive`, `thematic`)

### Visible — popover (sección "Respuesta")
- Timestamp, `turnDurationMs`, `provider`, `model`

### Visible — popover (sección "Uso real del proveedor")
- `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`, `costUsd`, `remoteCalls`
- Todos condicionados a `source == "provider_actual"`

### Visible — popover (sección "Knowledge")
- `materialCount`, `retrievalMode`, `localMode`, `exhaustiveCoverage`
- `retrievalCandidateCount`, `selectedEvidenceCount`, `selectedEvidenceBytes`, `selectedEvidenceUtf8Chars`
- `corpusEstTokens`, `evidenceEstTokens`, `contextReductionPct`, `semanticProviderState`, `requestPreparationMs`

### Visible — popover (sección "Fuentes utilizadas")
- `sourceNames[]`

### Visible — Detalle de conversación ("Último turno")
- Mismos campos que popover, usando `durableUsage()` / `durableKnowledge()`

### Visible — Detalle de conversación ("Acumulado de la conversación")
- `ConversationUsageTotals` mapeado a `SessionUsage`

### Interno / No expuesto
- `contextualFollowup`, `referentType`, `referentCount`, `originTurnId`, `baseIntent`, `turnKind`
- Todos los campos de `PromptContextMetrics` (session log only)
- Todos los campos de `SessionKnowledgeMetrics` (session log only, excepto los que mapean a `TurnMetrics`)

---

## 9. Regla de "No disponible"

**Definición:** `messages.conversationDetails.metrics.unavailable` = `"No disponible"` (`messages.ts:135`)

**Función helper:**
```typescript
function unavailable(value: number | null | undefined, suffix = ""): string {
  return value == null ? "No disponible" : `${value.toLocaleString("es-AR")}${suffix}`;
}
```

**Gating dual para tokens/cost:**
1. Si `value == null` → "No disponible"
2. Si `source != "provider_actual"` → "No disponible" (incluso si el valor no es `null`)

**Pruebas:** `AssistantMetrics.test.tsx:309`, `ConversationDetails.test.tsx:320` (espera `>= 6` ocurrencias de "No disponible").

---

## 10. Verificación de AC-012 y AC-013

### AC-012: Provider-call bound
- Invariante: las llamadas al proveedor están acotadas por la ruta resuelta y su presupuesto serializado.
- **Prueba canónica:** `crates/project-app/tests/exhaustive_rag.rs::j_exhaustive_negative_does_not_issue_per_document_remote_calls`
- **Comando:** `cargo test --locked -p project-app --test exhaustive_rag -- j_exhaustive_negative_does_not_issue_per_document_remote_calls`
- **Impacto en inventario:** `remoteCalls` para turnos de inventario/exhaustive negativo es siempre `0`. No se emiten llamadas por documento.

### AC-013: Corpus privacy
- Invariante: cuerpos crudos de corpus, rutas de sistema y secretos no aparecen en telemetría ni logs.
- **Pruebas canónicas:**
  - `crates/project-app/tests/exhaustive_rag.rs::h_privacy_metrics_and_logs_omit_bodies_paths_and_secrets`
  - `crates/project-app/tests/runtime_gate.rs::h_privacy_no_prompt_or_body_in_metrics`
- **Comandos:**
  - `cargo test --locked -p project-app --test exhaustive_rag -- h_privacy_metrics_and_logs_omit_bodies_paths_and_secrets`
  - `cargo test --locked -p project-app --test runtime_gate -- h_privacy_no_prompt_or_body_in_secrets`
- **Impacto en inventario:** `sourceNames` contiene solo nombres de display, nunca rutas de filesystem ni contenido de materiales.

---

## 11. Campos sin evidencia de semántica (marcados No disponible/interno)

Los siguientes campos no tienen evidencia verificable de su significado para el usuario no técnico. Se marcan como internos hasta que TASK-METRICS-2 o revisión humana defina su disposición:

| Campo | Razón |
|---|---|
| `semanticProviderState` | Estado interno del proveedor semántico; sin definición de usuario |
| `exhaustiveCoverage` | Valor del backend sin documentación de significado visible |
| `eligibleMaterials` | Conteo interno del modo exhaustive/thematic |
| `materialsInspected` | Conteo interno del modo exhaustive |
| `chunksInspected` | Conteo interno del modo exhaustive |
| `lexicalHits` | Conteo interno del modo exhaustive |
| `semanticHits` | Conteo interno del modo exhaustive |
| `requestPreparationMs` | Duración de preparación; sin definición clara de unit para usuario |

> NOTA: Estos campos ya se renderizan en el popover actual. La decisión de visibilidad/final es de `TASK-METRICS-2` y la revisión humana.
