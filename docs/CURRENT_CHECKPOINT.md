# Current Checkpoint

> Handoff operativo del estado ACTUAL del repositorio. No es documentación
> histórica: se reescribe al cambiar de fase/milestone. El repositorio es la
> memoria durable; este documento es la entrada a la sesión siguiente.

## Estado actual

- Current milestone: **M10 — Packaging** (**CLOSED**)
- Current phase: **CLOSED_READY_FOR_M11**
- Current main commit: (see `git log -1` after this checkpoint commit)
- **UX_RELEASE_GATE_01: APROBADO** (2026-08-31) — ver `docs/UX_RELEASE_GATE_01.md` §11. La UI
  2×2 dashboard observada **no es** la UI objetivo; dirección aprobada: chat-first simple.
- M1-M10: cerrados. M10 implementado, revisado e integrado (T1-T5).
- ADR-0013 (sidecar resolution, pinning, checksum verification): **Accepted** e
  implementado.
- M10 design: `docs/M10_DESIGN.md` (**Implemented and closed**).
- Pins aprobados por el humano y commiteados en `config/components.json`:
  - `opencode` **1.18.25** — `anomalyco/opencode` (ex-sst/opencode, redirect 301
    verificado) — `opencode-linux-x64.tar.gz` —
    `58a3729a6f3432dd6d2917fcc4a949788891a035818646ad480e12c947f56e78`.
  - `cloudflared` **2026.8.3** — `cloudflare/cloudflared` —
    `cloudflared-linux-amd64` —
    `f29324fe934d1e100617484c78deef803c4dc2cd351d645bbde42e96b4fccc5e`.

## Entregables M10 (integrados en main)

- `crates/project-app/src/sidecar.rs` (+ `tests/sidecar.rs`): `resolve_sidecar`
  / `resolve_sidecar_from_env`, `SidecarLocation { Bundled | OnPath }`, override
  `EDUCAI_SIDECAR_DIR`, fallback `<name>-x86_64-unknown-linux-gnu`.
- `crates/project-app/src/app.rs`: helper puro `apply_sidecar_locations`.
- `app/src-tauri/src/lib.rs`: `build_state` resuelve sidecars desde
  `current_exe().parent()`; fallback PATH dev preservado.
- `app/src-tauri/tauri.conf.json`: version `0.1.0`, `targets = [appimage, rpm]`,
  `bundle.externalBin = ["../sidecars/opencode", "../sidecars/cloudflared"]`
  (Tauri añade `-<target-triple>` al source; el bundle queda con el nombre
  simple). Crate versions 0.1.0 alineadas.
- `config/components.json` (schema v1) + `scripts/fetch-sidecars` (fetch +
  SHA-256 fail-closed + install triple-suffixed en `sidecars/` gitignored;
  `--check` offline).
- `scripts/verify`: gate M10 offline (`fetch-sidecars --check`, version
  alignment 0.1.0, `cargo check` src-tauri con `externalBin` neutralizado vía
  `TAURI_CONFIG`, gate final imprime `verify: M10 contract passed`).
- `docs/VERIFY.md`: sección `## M10 packaging behavior`.
- `scripts/smoke-package`: smoke manual de packaging (SKIP exit 3 sin tooling).

## Verificación final (session M10)

- `./scripts/verify` → `verify: M10 contract passed`, exit 0 (offline; sin
  descargas de sidecars, sin bundle).
- `git diff --check` limpio.
- Suites Rust + frontend verdes.
- Artifacts AppImage/RPM **no** construidos en esta sesión (sin `cargo tauri`
  CLI / appimagetool / rpmbuild en el entorno); `scripts/smoke-package` SKIP
  (exit 3). El smoke real es operación de release manual.

## Próximo milestone

- M11 (Component Updates + native Windows CI): **no iniciado**. NO se ha
  empezado work de M11.
- **Próximo paso (sin código en esta sesión):** delta de arquitectura acotado para la UX
  chat-first (persistencia D1 + vocabulario D2), aprobado por el owner antes de planificar el
  milestone de UX. B1/B2 son blockers de release.

## Decisión UX — UX_RELEASE_GATE_01 (2026-08-31)

Gate de validación visual de la UI actual contra la dirección de producto aprobada (chat-first).
**APROBADO** por el owner. Evidencia: `docs/ux-release-gate-01/`.

- **B1** (dashboard-first, no chat-first) y **B2** (superficie técnica modelo/proveedor) deben
  corregirse **antes del release**.
- **D1** — Persistencia de conversación **requerida** (sobrevive al restart). **No** se eligió ni
  implementó mecanismo (localStorage u otro) en esta sesión; requiere una **decisión de
  arquitectura acotada** antes de implementar.
- **D2** — **"Conversación"** como concepto contenedor user-facing; el modelo de dominio interno
  `Project` / `ProjectId` permanece sin cambios salvo que la revisión de arquitectura pruebe un
  cambio aditivo mínimo.
- **D3** — **"Compartir"** como acción en el área inferior de la conversación; **sin** panel
  permanente `Compartir` en el dashboard.
- **M11** — **no iniciado**; no se implementó ni diseñó la rediseño en esta sesión.