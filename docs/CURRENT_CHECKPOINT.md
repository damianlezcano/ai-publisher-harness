# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: **M10 — Packaging**
- Current phase: **DESIGN_APPROVED_IMPLEMENTATION_PENDING**
- Current main commit: `77e5dbe` (merge final M9 T9 + gate T10; M9 closed)
- M1-M9: cerrados. M9 design Implemented. ADR-0012: Accepted.
- ADR-0013 (sidecar resolution, pinning, checksum verification): **Accepted**.
- M10 design: **Approved** en `docs/M10_DESIGN.md` (finalizado como diseño
  aprobado). No iniciada la implementación.
- Implementación: NO se implementa M10 en esta sesión (HIGH_ARCHITECTURE
  cierra con handoff durable).

## Alcance M10 (aprobado)

- **Objetivo (Linux-first, self-contained distribution):** una persona no
  técnica debe poder instalar y ejecutar la app en Linux sin OpenCode,
  cloudflared, Rust, Cargo, Node, pnpm/npm ni setup técnico previo.
  Windows/macOS fuera de alcance.
- **Artifact targets (aprobados, exactos):** AppImage, luego RPM
  (Fedora 44 x86_64). No agregar formatos adicionales.
- **Sidecars objetivo:** `opencode` + `cloudflared`. No agregar otros binaries.
- **Resolver strategy (orden aprobado, no ampliar):**
  1. `EDUCAI_SIDECAR_DIR` override explícito (dev/test, si se permite);
  2. bundled sidecar (install dir: `<name>`, `<name>-<triple>`);
  3. `PATH` fallback (development compatibility only).
  La distribución final prefiere binaries bundled.
- **Pinning:** NO usar `latest`. Cada componente con version exacta, source
  oficial, platform/arch, SHA-256 esperado, destino/nombre de bundle. La info
  vive en `config/components.json` (o el mecanismo aprobado en M10_DESIGN).
- **Supply chain (ADR-0013):** official-source-only + pinning exacto +
  verificación SHA-256 + fail closed. Checksum incorrecto MUST FAIL; no
  empaquetar binary no verificado; no mirrors arbitrarios.
- **`scripts/fetch-sidecars`:** fuentes oficiales, versión exacta, SHA-256,
  fail ante mismatch, target/platform explícito, reproducible, no updater, no
  depende del cwd. `--check` offline (wired en `verify`).
- **`scripts/verify` offline:** no descarga sidecars; comprueba manifest/schema,
  pins, formato de checksums, resolver tests, bundled-path rules, Tauri config,
  fake sidecars determinísticos.
- **Tauri:** `bundle.externalBin` (o mecanismo Tauri 2 real aprobado);
  version `0.1.0`; nombres de binaries/target triples correctos.
- **No product behavior changes:** project-core/fs, AgentEngine, provider
  OpenCode, PublicationManager, LocalPublisher, tunnel, credentials, preview,
  attachments, sharing. Sin commands/capabilities nuevos. Si hace falta tocar
  algo: `ARCHITECTURE_ESCALATION_REQUIRED`.
- **Signing:** diferido. Artifacts M10 may be unsigned; production signing
  deferred. No generar claves.
- **Diferido (no iniciar):** Windows/macOS, auto-update, code signing, CSP de
  producción, update/rollback (M11).

## Task graph M10 (exacto, de M10_DESIGN)

```
T1 sidecar resolution ── T2 Tauri bundle ──┬── T5 package smoke
                                            │
T3 components/pinning ── T4 verify ────────┴── T6 gate/checkpoint
```

| # | Task | Depends |
| --- | --- | --- |
| 1 | Sidecar resolution (`resolve_sidecar`) + tests | — |
| 2 | Tauri shell wiring + bundle config + version 0.1.0 | 1 |
| 3 | Component manifest + fetch-sidecars (+ `--check`) | — |
| 4 | verify gate + docs (VERIFY.md M10, version check) | 1, 3 |
| 5 | smoke-package script | 2, 3 |
| 6 | Integration + gate + checkpoint (DoD) | 4, 5 |

T1 y T3 son paralelos tras T0; T2 tras T1; T4 tras T1+T3; T5 tras T2+T3; T6
tras T4+T5.

- **Next task:** T1 (sidecar resolution) y T3 (components/pinning), paralelos.
- **Implementation orchestrator:** `opencode-go/deepseek-v4-flash` (fresh
  session). HIGH_ARCHITECTURE = `opencode-go/deepseek-v4-pro` solo en
  escalation. AUTHOR != REVIEWER. FAIL TWICE -> SWITCH AGENT. Contexto
  task-local.
- **Session rotation (Flash):** ~100K actualizar checkpoint/reach checkpoint;
  >=150K rotar a Flash fresco. No crecer a 200K-400K.

## Documentos para la próxima sesión fresca

- CODEX_HANDOFF.md, docs/AGENT_POLICY.md, docs/ARCHITECTURE.md, docs/SECURITY.md
- docs/PLATFORM_POLICY.md (AppImage→RPM), docs/M10_DESIGN.md (**Approved**),
  docs/decisions/0013-* (**Accepted**), docs/VERIFY.md (agregar sección M10)
- docs/CURRENT_CHECKPOINT.md (este documento)

## Pendiente de aprobación humana (para T3, no bloquea T1/T2)

- Aprobar las versiones exactas + SHA-256 de `opencode`/`cloudflared` que T3
  rellenará en `config/components.json` desde canales oficiales (no fabricadas
  por el diseño).

## Próximo milestone

- M11 (Component Updates + native Windows CI): **no iniciado**.
