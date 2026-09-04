# Platform Policy

## Current target

**Primary development platform:** Fedora Linux 44 x86_64.

**Distribution targets:** Linux x86_64 and Windows 11 x64.

When packaging is introduced, target Linux distribution artifacts in this
order: AppImage, then RPM.

## Supported distribution policy

Linux is distributed as an AppImage built only inside the controlled Ubuntu
24.04 root documented in `docs/DISTRIBUTION.md`. The support floor is Ubuntu
24.04-family / glibc 2.39. Fedora is a development platform, never a release
library source.

Windows is distributed as a native x64 NSIS installer built on a Windows x64
runner using MSVC. Cross-compilation from Fedora is not supported. The initial
runtime floor is Windows 11 x64; Windows 10 is not claimed until separately
validated. Each platform ships its own checksum-pinned OpenCode and cloudflared
sidecars.

## Portability rule

**Develop on Fedora. Package in controlled native build roots.**

Core and adapters must avoid unnecessary Unix dependencies, hardcoded absolute
paths and separators, and contracts that require Linux-only behavior. Use
cross-platform filesystem APIs and preserve Fedora-executable tests for
traversal, backslash, Windows-style, and UNC-like paths where meaningful. This
does not require Windows-specific runtime behavior or packaging today.

Future target-specific builds may bundle matching application, OpenCode, and
cloudflared binaries independently for Linux and Windows. Those sidecars are
not implemented by this policy or M2.
