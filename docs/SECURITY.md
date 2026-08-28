# Security Invariants

1. User inputs are never publicly served.
2. Agent workspace is never publicly served.
3. Only explicit publish roots are registered with the publisher.
4. Prevent `..` path traversal.
5. Prevent symlink escape from a publish root.
6. Avoid exposing hidden/system metadata unintentionally.
7. Publishing one project cannot expose another project's files.
8. Credentials must never be written into project files, logs, URLs or exported bundles.
9. Public routes are read-only.
10. HTTP publication must never become a remote command execution surface.
11. Root URL must not reveal all active projects by default.
12. Treat externally supplied HTML/JS as untrusted content in the desktop preview context; isolate preview appropriately.
