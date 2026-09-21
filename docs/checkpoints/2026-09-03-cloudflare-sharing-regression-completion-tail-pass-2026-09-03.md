## Cloudflare sharing regression — completion tail PASS (2026-09-03)

- **Cloudflare regression investigation: COMPLETE.** The human-reported regression
  is **NOT reproducible in the current runtime**. The three-layer matrix is
  **LOCAL/TUNNEL/PUBLIC = PASS/PASS/PASS** (origin `127.0.0.1:47787`, cloudflared
  2026.8.3, trycloudflare URL, HTTP 200, public body SHA-256 matched local). The
  interrupted `crates/project-tunnel/src/cloudflare.rs` candidate (DNS resolution
  + TCP/443 readiness polling) remains **DISCARDED and absent** from the tree
  (verified: no `getaddrinfo`/`TcpStream`/readiness-poll/2s-connect code present;
  git grep clean). **No production Cloudflare implementation fix is required.**
- **Explicit UI-driven validation (this tail, real FE dist in WebKitGTK 4.1 +
  real Rust bridge wrapping the real AppState publication manager + real
  cloudflared + real trycloudflare URL):**
  - **UI Compartir = PASS** — UI reached `Compartido` (composer + card); public
    URL shown in the share menu; public HTTP **200**; body SHA-256
    `402cf2d3…` byte-identical to the expected Creation (`Actividad`,
    "Palabras que confunden - Inglés", 1113 bytes); no stale/wrong artifact.
    Evidence: `/tmp/opencode/share-ui/share-ui-report.json`,
    `/tmp/opencode/share-ui/u1-shared.png`.
  - **UI Dejar de compartir = PASS** — confirm dialog shown with truthful
    message; UI returned to `Compartir`; no stale `.share-control-menu` /
    `.share-control-url`; sidebar Compartido badge count 0; backend state
    `local`. Evidence: `u2-confirm.png`, `u2-local.png`.
  - **UI Compartir again = PASS** — after unshare, re-share succeeded; UI
    reached `Compartido` again; new public URL HTTP **200**; body SHA-256
    `402cf2d3…` (expected Creation, byte-identical); no stale process/
    publication state blocked the operation. Evidence: `u3-shared.png`.
  - **External flakiness noted (not a defect):** trycloudflare quick-tunnel
    hostnames occasionally never register on the public DNS edge for a given
    run (curl 000 while tunnel + origin were healthy). A subsequent re-share or
    fresh run resolves fine — matches the Codex probe S1/S2/S4 pattern. The
    harness used a bounded outer retry and retained a fully-passing run. No
    production change warranted (per the DISCARD disposition).
- **Independent reviews (fresh OpenCode Go sessions, Herdr `w1N`):**
  - **Product/UX reviewer = OpenCode/DeepSeek V4 Flash = APPROVE.** Reviewed the
    evidence package + the actual share surface (`useShareControl`,
    `PublishPanel`, `CreationsPanel`, `WorkspaceView`, `ConversationsSidebar`,
    `app.rs`, `manager.rs`, `app_facade.rs`). Findings: Compartir truthful
    end-to-end; Dejar de compartir clean; Compartir-again clean (route reused,
    fresh tunnel URL); state derived solely from backend `publication_status`
    after refresh, busy labels truthful, no misleading Compartido state; public
    link shown in auto-open menu, Open is backend-resolved. PNG screenshots not
    renderable by the reviewer model; claims assessed via textual report/log.
  - **Code/Correctness reviewer = OpenCode/Qwen 3.8 Flash = APPROVE.** Confirmed:
    final tree did NOT retain the discarded cloudflare.rs candidate (no
    DNS/TCP readiness change remains); publication state transitions
    (publish/unpublish/status) correct and idempotent; unshare keeps other
    published / last-stop stops tunnel+publisher; republish reuses route;
    publication + app_facade test suites green; session-budget tooling
    unaffected (`scripts/test-session-budget` all checks passed); quoting
    untouched (diff `99aa62f..HEAD` = checkpoint doc + budget-selector fix only);
    GLIBC untouched; M11 NOT STARTED; harness/bridge stayed outside the repo
    (git status clean, HEAD `182be5c`).
- **`./scripts/verify` = PASS, EXIT=0** (log `/tmp/opencode/verify-share-tail.log`):
  cargo fmt --all -- --check, cargo clippy --locked --workspace --all-targets
  -D warnings, **FE 244/244** in 21 files, **Rust 1148 tests passed in 85 suites**
  (0 failed), `pnpm` build/test, fetch-sidecars --check (opencode + cloudflared),
  M10 0.1.0 + UX_REDESIGN_01 contracts, cargo check src-tauri, git diff --check.
  Working tree clean.
- **Quoted prompts: HUMAN-PASS (untouched). GLIBC: pending (untouched). M11: NOT
  STARTED.**
- **Disposition:** all gates pass — **TECHNICALLY READY FOR HUMAN
  RE-ACCEPTANCE. NO HUMAN ACCEPTED.** Next and only gate: human product-owner
  re-acceptance. Stop; do not start GLIBC in this session.
