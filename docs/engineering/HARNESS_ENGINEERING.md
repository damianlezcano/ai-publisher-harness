# Harness Engineering de EducAI

## Estado y autoridad

Este documento es la fuente canónica vigente de metodología de Harness
Engineering para EducAI (prompt, context, harness, evaluation, loop y graph).
La política de costo/sesión/routing está en `docs/engineering/AGENT_POLICY.md`.
El procedimiento de delegación Herdr/worktree está en
`docs/engineering/MULTI_AGENT_WORKFLOW.md`. Los prompts en `prompts/` son
contratos locales de rol y no duplican esta metodología. Define cómo se
organiza y verifica el trabajo asistido por agentes; no reemplaza las
autoridades de producto, arquitectura, seguridad, UX ni los ADRs. Las
decisiones operativas cambiables de runtime están en `RUNTIME.md` y no se
codifican aquí por proveedor o modelo.

El orden de autoridad de entrada está en `CODEX_HANDOFF.md` y `AGENTS.md`.
La ejecución local se verifica con `./scripts/architecture-verify` y
`CI=true ./scripts/verify`.

## Metodología canónica aplicable a EducAI

### Prompt Engineering

Las instrucciones persistentes son contratos por rol: `prompts/orchestrator.md`,
`prompts/worker.md` y `prompts/reviewer.md`. Deben expresar objetivo, límites,
forma de handoff y condiciones de detención. Un prompt no sustituye una
autoridad del repositorio ni autoriza cambios fuera de la tarea.

### Context Engineering

Cada rol recibe sólo el contexto necesario:

- Orchestrator: autoridades, requirement, backlog/checkpoint y contratos afectados.
- Worker: su contrato de tarea, extracto del requirement, contratos afectados y código dentro de su scope.
- Reviewer: requirement, contrato, contratos afectados, base/diff exacto y evidencia.

El contexto debe ser explícito, acotado y recuperable desde el repositorio.

### Harness Engineering

El harness combina reglas (`AGENTS.md`), roles (`prompts/`), lifecycle
(`docs/engineering/REQUIREMENTS.md`, `tasks/`), runtime (`RUNTIME.md`), contratos de
arquitectura y gates ejecutables. Su función es hacer el trabajo repetible,
observable, seguro de delegar y trazable; no cambiar el comportamiento del
producto para adaptarlo al harness.

### Evaluation Engineering

La evidencia ejecutable precede a la afirmación de cierre: formato,
lint/typecheck, tests relevantes, gate de arquitectura, `CI=true
./scripts/verify` y revisión independiente. Los AC describen observables y
tests concretos; `scripts/architecture-verify` valida el manifiesto y ejecuta
las suites focalizadas offline.

### Loop Engineering

El ciclo es `IMPLEMENT → VERIFY → REVIEW → REWORK → VERIFY → REVIEW → PASS`.
`IMPLEMENTATION_COMPLETE` indica handoff de Worker, no aprobación ni cierre.
Un `REWORK` conserva el requirement activo y exige evidencia nueva antes de
otra revisión independiente.

### Graph Engineering

El Orchestrator descompone requirements en tareas con dependencias y ownership
no conflictivo. Una tarea tiene un autor checkout; un reviewer independiente
inspecciona otro checkout o el diff exacto. La integración y el cierre son
responsabilidad del Orchestrator después de gates y la aprobación requerida.

## Lifecycle operativo

1. Un requirement aprobado parte de `tasks/backlog/`; una idea no comprometida reside en `tasks/future/`.
2. El Orchestrator declara milestone, paths, criterios, comandos, gate humano, autor y reviewer; lo mueve a `tasks/active/<REQ-ID>/` y crea contratos acotados.
3. El Worker implementa sólo su contrato y entrega resultado y comandos reales.
4. El Orchestrator ejecuta gates, entrega base/diff/evidencia al Reviewer y conserva `IN_REVIEW` o `REWORK` hasta recibir `PASS` independiente.
5. Sólo tras `PASS` y el gate humano declarado el Orchestrator puede mover el requirement a `tasks/done/`.

Los cambios que pretendan alterar un AC protegido se detienen como
`ARCHITECTURE_CHANGE_REQUIRED: AC-XXX`; requieren ADR, observables/tests/gate
actualizados, revisión independiente y aprobación humana explícita.

## Controles y límites actuales

**HARD local:** gates deterministas (`scripts/architecture-verify`,
`scripts/verify`) y suites offline que éstos ejecutan.

**SOFT procesal:** ownership de checkout, roles, lifecycle, task contracts y
revisión independiente; son reglas operativas auditables, no bloqueo técnico.

**DOCUMENTARY:** definición de rutas protegidas, proceso de cambio de AC y
requisito de aprobación humana. Están documentados y revisables, pero el
repositorio aún no impide técnicamente editar contrato, test y gate en el mismo
cambio.

**Future hardening:** enforcement consciente del diff, CODEOWNERS, CI remoto,
branch protection y una regla que impida modificaciones simultáneas de
contrato/test/gate sin revisión independiente.

## Ejemplo histórico no normativo: TaskBoard

TaskBoard fue una demo pedagógica usada para explicar cómo un harness mínimo
puede evolucionar desde prompts a roles, tasks, gates y loops. No es EducAI, no
describe su producto, sus comandos ni su routing actual, y no impone modelos o
proveedores. Su trazabilidad histórica permanece señalada por
`examples/README.md`; cualquier recreación de la demo debe usar sus propios
comandos y no sustituir esta metodología.
