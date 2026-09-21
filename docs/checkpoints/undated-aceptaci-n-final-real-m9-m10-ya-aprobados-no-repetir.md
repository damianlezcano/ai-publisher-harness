## Aceptación final real (M9/M10 ya aprobados, no repetir)

- **AppImage NUEVO POST-T7 (main `773278d`, post-T7 blocker fixes integrados):**
  `app/src-tauri/target/release/bundle/appimage/EducAI_0.1.0_amd64.AppImage`,
  180.816.376 bytes, SHA-256
  `930ee074bfbe40b4cf1e5c9582c93b884d695d6348bf7521e764ade5b9f6834d`,
  timestamp 2026-09-01 20:47:14 -0300. **ESTE es el artefacto para la
  re-aceptación humana.** (AppImage previo T7 `3dba67a8…` es STALE — predata los
  fixes A/B/C; NO usar como evidencia de aceptación.)
- Modelo gratis real confirmado: `big-pickle` (providerID `opencode`), cost 0,
  respuesta "¡Hola! ¿Cómo puedo ayudarte?". `modelGetSelected`/`default_free_model`
  determinista (ADR-0015); NO hardcodear nombres (solo tests/fake).
- PATH-independencia y sidecars bundled verificados en M10 y re-verificados en
  T7 contra el AppImage NUEVO (opencode 1.18.25 + cloudflared 2026.8.3 en
  payload; launch con PATH sin sidecars usa el bundled).
- **Este pass NO re-testea M1-M10; solo integró las correcciones A-G, corrió el
  Playwright headed (T6) y construyó/verificó el AppImage NUEVO (T7) para
  revisión humana.**
