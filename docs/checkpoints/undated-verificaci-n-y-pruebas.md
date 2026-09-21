## Verificación y pruebas

- Frontend: `cd app && pnpm format:check && pnpm lint && pnpm typecheck && pnpm test`.
- Rust: `cargo fmt --check && cargo clippy --all-targets && cargo test`.
  (Drift de toolchain: rustup no instalado; clippy 1.98.0 vs pin 1.97.1 — el
  drift NO se manifestó en la corrida previa; si aparece, es preexistente.)
- Gate completo: `./scripts/verify` (exit 0 al final del pass).
- `git diff --check` limpio; `git status --short` limpio antes del handoff.
- Budget: `scripts/check-session-budget` ANTES de cada lanzamiento de worker
  (CONTINUE <80K; CHECKPOINT_WARNING 80K-99,999; ROTATE 100K-129,999; HARD >=130K).
