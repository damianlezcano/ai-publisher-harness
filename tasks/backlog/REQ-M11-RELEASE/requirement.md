# REQ-M11-RELEASE: Versionado y publicación atómica de EducAI

- Status: `BACKLOG` — aprobado para futura selección; no activo ni autorizado para implementación.
- Planning baseline SHA: `09bcb280ba7670b42c86e62111a814a410f38561` (`main`).
- Human gate: `RELEASE_HUMAN` antes de una publicación pública.
- Repositorio oficial: `https://github.com/damianlezcano/educai.git`.

## Objetivo y alcance

Formalizar una única versión visible, `EducAI X.Y.Z`, que identifica un conjunto compatible y probado de componentes internos. Una operación futura determina la nueva versión, mantiene coherentes sus fuentes, construye nativamente Linux y Windows, valida los dos artefactos y sólo entonces publica un GitHub Release. OpenCode, cloudflared, ONNX Runtime, modelo local y demás sidecars no son actualizables ni visibles independientemente para el usuario.

Incluye el contrato de `scripts/release`; preparación, commit/tag; Actions nativas Linux AppImage y Windows NSIS; validación de artefactos/componentes; SHA-256, `checksums.txt`, `release.json`; publicación atómica; fallo parcial; reemplazo explícito limitado a staging no publicado; permisos; documentación y gates. EducAI se publicará bajo Apache License 2.0. No incluye updater/UI, descarga/instalación automática, rollback de una instalación, canales beta/nightly, firma/notarización, stores ni actualización autónoma de componentes. La actualización in-app queda separada para futuro `REQ-M11-UPDATE` o equivalente.

## Inventario y política de versión

| Fuente real baseline | Valor | Decisión de implementación |
| --- | --- | --- |
| `crates/project-app/Cargo.toml` | `0.1.0` | fuente canónica propuesta |
| `app/src-tauri/Cargo.toml` | `0.1.0` | derivada y alineada |
| `app/src-tauri/tauri.conf.json` | `0.1.0` | derivada y alineada |
| `app/package.json` | `0.0.0` | derivada: alinear a EducAI o documentar/probar una excepción; se propone alinear |
| `packaging/linux/build-linux-appimage` | nombre con `0.1.0` | derivada; eliminar literal |
| `packaging/linux/construct-appdir` | `version="0.1.0"` | derivada; eliminar literal |
| `scripts/verify` | `m10_version='0.1.0'` | gate derivado; eliminar literal |
| `Cargo.lock`, `app/src-tauri/Cargo.lock` | resolución de paquetes | no son versión de EducAI; sólo cambian si Cargo legítimamente los actualiza |

La tarea de inventario debe detectar nuevas fuentes de versión de producto, incluidos manifests, scripts, nombres de artefacto y documentación/contratos que dependan de ella. No confundirá `0.0.0` de crates internos ni versiones de dependencias/toolchains con la versión de producto.

`config/components.json` fija, por plataforma, OpenCode `1.18.25`, cloudflared `2026.8.3` y ONNX Runtime `1.22.0`, con SHA-256 y payloads. `config/knowledge-models.json` fija la generación `intfloat/multilingual-e5-small`, revisión `ccc66d3bcd826f577e26b9a4072cc5fe3a7ad6a3` y hashes; el modelo es descarga verificada de primer uso, no payload del binario. Ambos manifiestos se validan y se incluyen como provenance, pero no se bump-ean al versionar EducAI.

## Contrato futuro de `./scripts/release`

```text
./scripts/release                 # patch
./scripts/release patch|minor|major
./scripts/release X.Y.Z [--replace]
```

- Sin argumentos equivale a `patch`: `X.Y.Z -> X.Y.(Z+1)`; `minor` -> `X.(Y+1).0`; `major` -> `(X+1).0.0`.
- `X.Y.Z` es SemVer estable exacto, sin prefijo `v`, prerelease ni build metadata en esta primera política; debe ser mayor que la canónica. `0.2.0` prepara exactamente `EducAI 0.2.0`.
- Rechaza argumentos ambiguos/invalidos, downgrade/igualdad, working tree sucio, branch distinta de `main`, `main` no sincronizado con `origin/main`, gates fallidos y tag/release/asset existente salvo `--replace`.
- Antes de estado remoto: gates locales, actualización de todas las derivadas, comprobación de coherencia y commit de release limitado a cambios esperados. Tras push exitoso crea/pushea tag anotado `vX.Y.Z`; informa el resultado del workflow, no simula que publicó.
- Si falla antes de commit no deja cambio; tras editar o commit local nunca descarta trabajo automáticamente. Si falla el push no borra refs ni recrea commits.

La convención definitiva propuesta, normalizada desde outputs nativos después de validar su versión, es:

```text
EducAI_<X.Y.Z>_x86_64.AppImage
EducAI_<X.Y.Z>_x64-setup.exe
checksums.txt
release.json
```

No se seleccionará “el primer” `.exe` por glob ni un artefacto stale. El actual `..._amd64.AppImage` es evidencia a reconciliar cuando se implemente, no cambio autorizado ahora.

## Inmutabilidad y reemplazo explícito

Un GitHub Release publicado es inmutable. Su tag correspondiente tampoco se
reescribe, mueve, borra ni reemplaza. Si `v0.2.0` fue publicado, cualquier
corrección se publica como una versión nueva, por ejemplo `v0.2.1`.

La existencia de tag `vX.Y.Z`, GitHub Release `vX.Y.Z` o asset aborta por
defecto. `--replace` sólo acepta versión exacta; jamás acompaña a
patch/minor/major ni habilita una sobreescritura silenciosa. Sólo puede operar
antes de la publicación final sobre recursos de staging de esa misma versión.
Requiere preflight por Git/API que compruebe identidad exacta, release id/estado,
tag SHA, commit objetivo, autor/fecha y hashes de assets; también autorización
explícita y scopes mínimos.

Un recurso es reemplazable sólo si se cumple todo lo siguiente: (1) la API de
GitHub identifica exactamente el tag solicitado; (2) el release asociado tiene
`draft: true` y `published_at: null`; (3) no existe otro release publicado con
ese tag; (4) el tag, si existe, apunta al commit de release esperado y no se
modifica; y (5) los assets pertenecen a ese draft o son artefactos internos del
workflow identificados por su run. Un release con `draft: false` o
`published_at` no nulo se considera publicado aunque sus assets estén vacíos,
incompletos, fallidos u ocultos de otro modo: `--replace` aborta. La ausencia de
release puede ser staging interno, pero no autoriza tocar un tag que apunte a
otro commit.

`--replace` puede eliminar y regenerar únicamente assets/metadata del draft o
artefactos internos identificados, manteniendo el tag intacto y conservando un
inventario previo para trazabilidad. Tag divergente/protegido, release publicado,
identidad ambigua, assets no atribuibles al draft o API sin operación segura
implican abortar sin borrar estado. Nunca se borra/recrea un tag público, ni
siquiera con aprobación humana automatizada. La trazabilidad registra actor,
fecha, motivo, release id, tag/commit, estado del draft y hashes retirados/
publicados, sin secretos.

## GitHub Actions y publicación atómica

```text
commit de versión + tag vX.Y.Z
             -> validar tag/version/manifests/contracts
             -> Linux nativo Ubuntu 24.04: AppImage + verificación
             -> Windows nativo x64/MSVC: NSIS + verificación
             -> agregar y verificar assets, checksums y release.json
             -> publicar GitHub Release (única transición final)
```

No habrá cross-compilation. Linux reutiliza `./scripts/package linux-appimage` y el flujo Ubuntu 24.04, incluidos gates de GLIBC 2.39, WebKit, graphics boundary y ONNX Runtime. Windows reutiliza `packaging/windows/build.ps1` y REQ-WIN-001: NSIS, OpenCode, cloudflared, `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, providers, tamaños/hashes y contrato Knowledge. No reabre REQ-WIN-001 sin evidencia de conflicto.

Ambos jobs parten del SHA del tag y sólo suben artefactos intermedios privados del run tras validar. Un job agregador descarga ambos, verifica nombre/versión/arquitectura/bytes/hash, genera SHA-256 de cada binario y publica. Es condición obligatoria: `linux PASS AND windows PASS AND final verification PASS`. Un fallo sólo puede dejar artefactos internos o draft no público; jamás un release estable parcial. La única transición de un draft válido a publicado ocurre al final; después de ella no hay replace. Si GitHub no permite mantener esa frontera con seguridad, aborta.

## Contrato de metadata

`checksums.txt` será UTF-8 y tendrá una entrada SHA-256 inequívoca por cada binario publicado, con el nombre exacto del asset. `release.json` será JSON UTF-8 y schema-versionado, con `schemaVersion`, `educaiVersion`, `tag`, `sourceCommit`, `publishedAt` UTC RFC3339, y por artefacto plataforma, arquitectura, nombre, bytes y SHA-256. Incluye snapshot por plataforma de componentes, versiones, hashes y payloads; y modelo/generación Knowledge, revisión, runtime y hashes relevantes. Puede declarar imagen/toolchain fijados si aporta reproducibilidad. Nunca incluye token, secreto, ruta local, usuario, HOME/TEMP ni metadata privada. Es input potencial para el futuro updater, no su diseño.

## Gates y modelo de fallos

### Antes del tag

- tree limpio; branch autorizada; `main` sincronizado; SHA registrado;
- SemVer/monotonía, fuentes de versión y manifests coherentes; tag/release inexistente o preflight `--replace` completo;
- `./scripts/architecture-verify`, formato/lint/type/tests aplicables, `./scripts/test-distribution-contracts` y `CI=true ./scripts/verify`;
- componentes/modelo con schema, hashes, formatos, payloads y compatibilidad runtime válidos; sin secretos o cambios no autorizados.

### En Actions y después de construir

- checkout del SHA del tag y doble check tag/version; lockfiles y entradas fijadas; acciones por SHA; runners/control image declarados;
- artefactos recién generados; payloads/sidecars correctos; smoke/package checks aplicables;
- `checksums.txt` y `release.json` coherentes; cuatro assets obligatorios subidos y releídos/verificados antes de visibilidad pública.

Fallo de Linux, Windows, checksum, metadata, upload o publish deja tag como evidencia de candidato fallido y sin release final; sólo el draft/staging verificablemente no publicado puede reintentarse mediante `--replace`. Una vez publicado, se corrige con una nueva versión. El cleanup sólo alcanza drafts/artefactos internos identificados y nunca borra/modifica un release o tag publicado. Esto es rollback/cleanup de publicación; rollback de instalación de usuario queda fuera de alcance.

## Seguridad y documentación futura

El workflow público es auditable: sin PAT hardcodeado, `GITHUB_TOKEN`, `contents: read` para build y `contents: write` únicamente para job final; permisos declarados por job; publishing restringido a tag/entorno protegido del repositorio principal; acciones fijadas por SHA; validación de inputs; no secretos en logs/artefactos. `--replace` puede requerir `contents: write` para el draft y aprobación/environment; si faltan, falla cerrado. La implementación futura deberá alinear manifests y metadata del proyecto, además de avisos/documentación de distribución, con Apache License 2.0; este requirement no autoriza modificar todavía esos archivos ni crear `LICENSE`.

Al implementar se actualizarán proceso/política de versión, guía de maintainers, `docs/distribution/DISTRIBUTION.md`, componentes incluidos, contrato de metadata y recovery. No se produce todavía documentación de implementación, workflow, script, tag, release, commit o push.

## Criterios de aceptación

1. Default patch, patch/minor/major y versión exacta son probados; invalid/downgrade/estado inseguro se rechazan.
2. Una fuente canónica mantiene todas las fuentes reales de producto coherentes sin mutar locks/componentes accidentalmente.
3. Un release/tag publicado es inmutable; una corrección usa nueva versión. `--replace` es explícito, verificable, trazable y fail-closed sólo para draft/staging que satisface los cinco criterios definidos, sin tocar tags.
4. Linux nativo produce AppImage y Windows nativo NSIS con todos los payloads correctos.
5. Los binarios tienen SHA-256; checksums y `release.json` completos, públicos y sin secretos.
6. No hay release estable hasta PASS de Linux, Windows y verificación final; no hay release parcial visible.
7. Workflow/manifests son auditables y razonablemente reproducibles; fallos y cleanup están documentados/probados.
8. La implementación alinea manifiestos/metadata y avisos con Apache License 2.0; documentación, review independiente `PASS` y `RELEASE_HUMAN` son obligatorios antes de publicar de verdad.

## Riesgos y decisiones humanas pendientes

No quedan open questions funcionales ni de policy para este requirement: la
licencia es Apache License 2.0 y los releases/tags publicados son inmutables.
La selección futura deberá convertir estas decisiones en cambios de manifests,
avisos y controles verificables, con el `RELEASE_HUMAN` ya declarado.

## Tasks planificadas

1. `TASK-M11-1` — inventario ejecutable y contrato de versión.
2. `TASK-M11-2` — transacción y CLI de `scripts/release`.
3. `TASK-M11-3` — build/gates Linux nativos.
4. `TASK-M11-4` — build/gates Windows nativos.
5. `TASK-M11-5` — assets, checksums y `release.json`.
6. `TASK-M11-6` — publicación atómica y `--replace`.
7. `TASK-M11-7` — documentación, drills, review y human gate.

Al seleccionarse, se refresca SHA, se mueven los contratos a Active y se asignan autor/reviewer independientes. Este backlog no ejecuta ninguna task.
