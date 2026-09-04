# Distribution

## Support matrix

| Platform | Official artifact | Minimum supported OS | Controlled build environment | Status |
| --- | --- | --- | --- | --- |
| Linux x86_64 | AppImage | Ubuntu 24.04 family / glibc 2.39 | Ubuntu 24.04 Podman image | technical gate implemented; target-machine validation pending |
| Windows x64 | NSIS installer | Windows 11 x64 | native Windows x64 MSVC runner | technically ready for Windows runtime validation |

Linux uses Ubuntu 24.04 because it is the oldest evaluated LTS with both glibc
2.39 and the required WebKitGTK 4.1 development ABI. Ubuntu 22.04 ships an
older WebKitGTK ABI and is not a viable build root. The build root deliberately
does not inherit Fedora libraries. AppImage does not make a newer glibc
portable, so `scripts/check-appimage-glibc` extracts every shipped ELF and
rejects requirements above GLIBC 2.39.

## Build commands

On Linux with Podman:

```bash
./scripts/package linux-appimage
```

This controls the Ubuntu 24.04 baseline, verifies the Node 22.14.0 archive,
uses Rust 1.97.1 and the repository lockfiles, and checksum-verifies Linux
sidecars. Apt security updates and the Ubuntu image digest must be recorded in
the release metadata until a CI image digest is frozen. The output is under
`app/src-tauri/target/release/bundle/appimage/`; record its source HEAD, size,
SHA-256, tool versions, and GLIBC-gate result in release metadata.

On a native Windows 11 x64 machine with Visual Studio Build Tools (MSVC),
Windows SDK, Rust 1.97.1, Node 22.14.0/Corepack, pnpm, and the Tauri CLI:

```powershell
./packaging/windows/build.ps1
```

The command downloads only the pinned Windows OpenCode 1.18.25 archive and
cloudflared 2026.8.3 executable, checks their SHA-256 values, and emits one
NSIS installer under `app/src-tauri/target/release/bundle/nsis/`. No Linux
binary is reused. Capture its SHA-256 and source HEAD with the release.
The PowerShell implementation intentionally performs its own checksum-gated
fetch because the existing fetch helper is Bash and is not a Windows runtime
prerequisite.

## Runtime contracts

Tauri packages `opencode.exe` and `cloudflared.exe` for Windows, and the
Linux-named equivalents for AppImage. The existing owned-PID supervisor is
platform-neutral: it terminates only its child handles and never uses a
process-name kill. `opener` remains the sole bounded open-folder/open-document
abstraction, which uses native Explorer behavior on Windows. `EducAI.exe
--debug` retains current-session stdout/stderr logging; the in-app Log Viewer
does not persist diagnostics.

## Release validation

Use the same Linux AppImage on the Ubuntu 24.04 baseline, KDE Neon/Ubuntu
24.04-family target, and Fedora. Validate launch, UI, simple chat,
Preview/Abrir, a public trycloudflare URL, and clean owned-child shutdown.

Validate the NSIS installer on a real Windows 11 x64 target: launch, chat,
quoted prompt, attachment, Creation/Preview/Abrir, share/unshare, public URL,
and no owned OpenCode/cloudflared residue on close. Until that run is recorded,
Windows is **TECHNICALLY READY FOR WINDOWS RUNTIME VALIDATION**, not human-pass.
