# Test Conventions

Tests are organized by the behavior they protect, not by agent or UI screen.
Use deterministic temporary directories, fixed clocks/identifiers where needed,
and local fakes for external processes. Tests must never contact a real AI
provider, Cloudflare, or the public Internet unless an explicitly approved
manual release check requires it.

## Required test levels

| Change | Minimum evidence |
| --- | --- |
| Core domain or filesystem behavior | Unit tests plus filesystem integration tests |
| HTTP publisher or route isolation | Integration tests using a local ephemeral server |
| UI workflow | Component tests and an end-to-end happy path; add accessibility checks where supported |
| Tunnel/OpenCode process adapter | Contract tests with controlled fake subprocesses; manual smoke test before release |
| Security invariant | A focused regression test named for that invariant |

Test names describe observable behavior, for example: `does_not_serve_inputs`
or `stopping_project_a_keeps_project_b_available`. Do not assert internal
implementation details when public behavior can be asserted instead.

## Security regression matrix

M2 and later must include tests for: publish-root-only serving, traversal,
symlink escape, hidden metadata, project isolation, read-only requests, and a
non-enumerating root route. Preview work must test untrusted-content isolation.

## Test data

Fixtures contain only synthetic, non-sensitive material. Keep binary fixtures
small and explain their purpose. Never place credentials, real student data,
or public tunnel URLs in a fixture, snapshot, log, or assertion.
