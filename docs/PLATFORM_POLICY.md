# Platform Policy

## Current target

**Primary development platform:** Fedora Linux 44 x86_64.

**Initial MVP target:** Linux x86_64.

When packaging is introduced, target Linux distribution artifacts in this
order: AppImage, then RPM.

## Deferred platforms

Windows support begins only after the end-to-end Fedora MVP is stable. Do not
spend current milestone capacity on MSI/EXE installers, WebView2 behavior,
signing, PowerShell installers, Windows packaging or sidecars, Windows
credential storage, or Windows release CI. A future Windows build should use a
native Windows CI runner rather than complex Fedora cross-compilation.

## Portability rule

**Develop on Fedora. Design for portability. Do not implement Windows yet.**

Core and adapters must avoid unnecessary Unix dependencies, hardcoded absolute
paths and separators, and contracts that require Linux-only behavior. Use
cross-platform filesystem APIs and preserve Fedora-executable tests for
traversal, backslash, Windows-style, and UNC-like paths where meaningful. This
does not require Windows-specific runtime behavior or packaging today.

Future target-specific builds may bundle matching application, OpenCode, and
cloudflared binaries independently for Linux and Windows. Those sidecars are
not implemented by this policy or M2.
