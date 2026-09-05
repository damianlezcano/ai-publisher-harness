# ADR-0016: Knowledge local inference runtime distribution

- Status: Accepted

## Context

Knowledge K1 has delivered local TXT/Markdown normalization, chunking, SQLite,
and FTS5. K2 must later add local semantic embeddings for
`intfloat/multilingual-e5-small`, but must not make native inference an
uncontrolled Fedora-only development dependency. EducAI's distribution contract
is an Ubuntu 24.04-controlled Linux x86_64 AppImage with every shipped ELF
checked for `GLIBC <= 2.39`, and a native Windows 11 x64 MSVC/NSIS installer.
The current Windows feature set is HUMAN-PASS; it did not contain a Knowledge
runtime and is not evidence for that future payload.

The decision is limited to the runtime, tokenizer, model-export, artifact trust,
and delivery contract. It does not implement K2, add a dependency, download an
artifact, modify AppImage/NSIS packaging, change K1 lexical behavior, add
hybrid retrieval, or start M11.

## Decision

### 1. Runtime: bundled ONNX Runtime CPU, dynamically loaded by `ort`

Use the Rust [`ort` crate 2.0.0-rc.10](https://docs.rs/ort/2.0.0-rc.10/ort/)
with `default-features = false` and `features = ["std", "load-dynamic"]` when
K2 is implemented. This release family is a Rust wrapper for ONNX Runtime
1.22. Do **not** enable `download-binaries`, `copy-dylibs`, GPU execution
providers, or a build-time ORT download. The application will initialize
`ort::init_from(absolute_library_path)` before creating a session; it will not
use a global `ORT_DYLIB_PATH`, `PATH`, cwd, Python, Conda, Docker, a daemon, or
a GPU.

Ship the matching **ONNX Runtime CPU 1.22.0** native runtime with each EducAI
platform artifact. This is **BUNDLED WITH EDUCAI**, not a first-use runtime
download and not static linkage. A native runtime is a small, stable product
payload compared with the model, and bundling it avoids a second first-use
network operation and a partially usable native installation. The Rust-crate
and runtime versions are separately pinned: the crate lockfile pins `ort`, and
the static component manifest pins Microsoft release `v1.22.0` plus an archive
SHA-256.

The selected release is Microsoft ONNX Runtime's official
[`v1.22.0` release](https://github.com/microsoft/onnxruntime/releases/tag/v1.22.0),
MIT licensed. It is an actively maintained upstream CPU inference engine with
Linux x86_64 and Windows x64 release artifacts. Its CPU provider meets this
model's 384-float embedding output requirement. No execution provider other
than CPU is enabled.

### 2. Platform payload and deterministic loading

| Target | Verified upstream archive | Shipped files | Install/extraction location | Loading |
| --- | --- | --- | --- | --- |
| Linux x86_64 | `onnxruntime-linux-x64-1.22.0.tgz`, 7,798,730 bytes | `libonnxruntime.so.1.22.0` (about 21 MiB), `libonnxruntime_providers_shared.so` (about 15 KiB), with `libonnxruntime.so -> libonnxruntime.so.1 -> libonnxruntime.so.1.22.0` symlinks retained | AppDir `usr/lib/educai/onnxruntime/` (therefore extracted AppImage `usr/lib/educai/onnxruntime/`) | The shell resolves its own executable with `current_exe`, derives the AppDir install root, and passes the absolute `.../libonnxruntime.so` path into the Tauri-free Knowledge runtime adapter. |
| Windows x64 MSVC | `onnxruntime-win-x64-1.22.0.zip`, 72,368,545 bytes | `onnxruntime.dll` and `onnxruntime_providers_shared.dll` from the archive's `lib/`; CPU-only—no `DirectML.dll`, CUDA, or TensorRT DLL | the NSIS application directory, beside `educai.exe` | The shell derives `current_exe().parent()` and passes the absolute `onnxruntime.dll` path to `ort::init_from`; Windows normal DLL dependency lookup then finds the co-located provider DLL. No global PATH is used. |

The archive filenames and sizes above come from the official GitHub release
metadata. `onnxruntime.dll` also requires the supported Microsoft Visual C++
runtime (`VCRUNTIME140*`/`MSVCP140*`), which the native MSVC Tauri/NSIS release
must prove is present or explicitly install by its normal prerequisite policy.
The CPU archive has no GPU-provider DLL dependency. The complete archive file
list and installed-file SHA-256 values must be checked during the future
platform packaging task before a component entry is committed; no library may
be loaded until the fetched archive and expected extracted payload have passed
that check.

For Linux, Microsoft documents Linux x64 CPU support, while its published
prebuilt C++ archive does not publish a per-file GLIBC-symbol floor. The Python
1.22 release uses a manylinux 2.27/2.28 target, which is useful but not a
substitute for inspecting this C++ archive. It is compatible in principle with
EducAI's `<= 2.39` ceiling, but this is deliberately not a packaging PASS:
the controlled Ubuntu 24.04 build must put the actual `.so` files in the
AppImage and run `scripts/check-appimage-glibc ARTIFACT 2.39`. It must also run
`readelf --version-info` on both shipped libraries, record the highest
`GLIBC_*` requirement, inspect `DT_NEEDED`/`ldd` in the controlled root, and
prove the existing graphics boundary is unchanged. If a future vendor archive
needs `GLIBC > 2.39`, the release is rejected; remediation is a reproducible
Ubuntu-24.04 controlled source build with a separately reviewed operator set,
not silently accepting a Fedora-built library.

The runtime's archive SHA-256 is a component trust pin, not an implicit claim
that Microsoft publishes a checksum sidecar for this release. Before runtime
integration, the release engineer must make one audited HTTPS fetch from the
official release URL, record the archive SHA-256 in committed static metadata,
and require the fetch/packaging script to verify it before extraction. A
second manifest field set records expected extracted filenames, byte lengths,
and SHA-256 values. This matches ADR-0013's checksum-gated component pattern;
the first published EducAI release carrying this payload establishes the
reviewed reproducible pin. Later changes require a new explicit pin, not
`latest`.

### 3. Tokenizer

Use the Rust-native [`tokenizers`](https://github.com/huggingface/tokenizers)
crate, pinned by the workspace lockfile, with its `tokenizer.json` loader. It
has no separate shared-library payload: its core is Rust and its license is
Apache-2.0. K2 loads the tokenizer only from the verified model generation; it
does not invoke Python or SentencePiece C++.

The selected model generation includes all tokenizer data needed for exact
local encoding: `tokenizer.json`, `tokenizer_config.json`,
`special_tokens_map.json`, and `sentencepiece.bpe.model`. `tokenizer.json` is
the runtime input; the others are retained because they are part of the
upstream generation and aid validation/provenance. The implementation must
add regression fixtures that prove the E5 `query: ` and `passage: ` prefixes,
special-token handling, and hard maximum are applied before inference. The
model configuration declares `max_position_embeddings: 512`; K2's effective
input contract is therefore **512 tokenizer tokens including special tokens**.

### 4. Model export and first-use delivery

Use the upstream owner repository
[`intfloat/multilingual-e5-small`](https://huggingface.co/intfloat/multilingual-e5-small)
at immutable revision
`ccc66d3bcd826f577e26b9a4072cc5fe3a7ad6a3`, licensed MIT. Use the upstream
`onnx/model.onnx` **fp32** export, SHA-256
`ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665`,
470,268,510 bytes (about 449 MiB). This model's declared hidden size is 384
and its declared maximum position count is 512.

The complete initial generation is approximately 470 MiB for the model plus
22 MiB of tokenizer data. Its large size is the reason it remains **FIRST-USE
DOWNLOAD TO APP-DATA**, not AppImage/NSIS media. Its exact required files are:

| Relative artifact | Bytes / SHA-256 when supplied upstream |
| --- | --- |
| `onnx/model.onnx` | 470,268,510 / `ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665` |
| `onnx/tokenizer.json` | 17,082,730 / `0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39` |
| `onnx/sentencepiece.bpe.model` | 5,069,051 / `cfc8146abe2a0488e9e2a0c56de7952f7c11ab059eca145a0a727afce0db2865` |
| `onnx/config.json`, `onnx/tokenizer_config.json`, `onnx/special_tokens_map.json` | small JSON metadata; K2 records their exact byte lengths and SHA-256 in the static generation manifest before download is enabled |

The app-data location is
`<app-data>/knowledge-models/intfloat--multilingual-e5-small/ccc66d3bcd826f577e26b9a4072cc5fe3a7ad6a3/`.
It is global application-managed cache state, not project data and never an
AppImage/NSIS resource. A downloader fetches each exact HTTPS revision URL to
a private temporary sibling directory, verifies length and SHA-256, then
atomically renames the complete verified generation into place. Incomplete,
corrupt, or mismatched content is deleted/rejected and is never opened by the
tokenizer or ONNX Runtime. The cache root needs the same owner-only treatment
as app data on Unix.

FP32 is the K2 default. The upstream repository also exposes
`model_qint8_avx512_vnni.onnx` (about 118 MiB), but its AVX-512/VNNI name makes
it unsuitable as a universal x86_64 default; choosing it would add a CPU
feature gate and quality/compatibility validation that does not yet exist.
FP32 is the source-owner export with the clearest provenance and broadest CPU
compatibility. Int8 is a later benchmark candidate only after a generally
compatible export, output-quality comparison, and both-platform ORT tests are
recorded.

### 5. Manifest and update lifecycle

Extend `config/components.json` at future runtime integration with static,
target-specific `onnxruntime` entries following existing component fields
(`name`, `platform`, `version`, official `source`, archive `sha256`, `format`,
and `bundleName`) plus the minimally necessary extraction/payload map. It is
the right location for a product-bundled, platform-specific native component.

Create a separate committed `config/knowledge-models.json` for static model
generation metadata because the existing component manifest assumes one
packaged executable/archive, whereas a model generation is a multi-file,
app-data download contract. Its single initial entry contains: model ID,
immutable revision, license/source base URL, runtime compatibility (`ORT
1.22`), dimension, 512-token and E5-prefix contracts, normalization rule, and
per-file relative path/length/SHA-256. It contains no user cache state. The
app-data cache itself remains discoverable/rebuildable state only.

Runtime upgrades are explicit product changes: new `ort` compatibility review,
new archive and payload checksums, controlled Linux GLIBC/AppImage validation,
and a new Windows Knowledge-runtime NSIS validation. Model upgrades are a new
application-managed generation with new checksums; existing vectors remain
isolated by generation until re-embedding succeeds. A model update does not
upgrade the runtime unless the model manifest declares an incompatibility.

### 6. Failure, offline, and validation contract

- Runtime present but model absent: K1 lexical Knowledge continues; semantic
  use offers the local model-install action.
- Runtime absent, unreadable, or checksum-invalid: K1 lexical Knowledge
  continues; semantic use is disabled with a local runtime error.
- Model mismatch/corruption: reject it; do not tokenize or load it.
- Offline: no remote embedding fallback. Lexical Knowledge continues and
  semantic use stays unavailable until a valid local generation exists.

Future integration must add focused path-resolution/payload completeness tests,
an fp32 E5 smoke inference that asserts the expected 384-dimensional output,
token-limit and prefix tests, corrupted-artifact rejection tests, offline
tests, controlled Linux payload/GLIBC checks, and native Windows NSIS install
and semantic-inference validation. It must then run the ordinary repository
format, lint/type, relevant test, integration, and `./scripts/verify` gates.

## Alternatives considered

| Candidate | Compatibility and native payload | Linux risk | Windows risk | Verdict |
| --- | --- | --- | --- | --- |
| `ort` with Microsoft ONNX Runtime CPU dynamic library | Direct ONNX execution, mature Rust API, real tokenizer remains Rust-native, CPU supports fp32 384-d output | One bounded shared-library payload; controlled AppImage GLIBC check required | One DLL set plus MSVC runtime; deterministic explicit path | **Selected** |
| `ort` managed/downloaded runtime | Technically viable `ort` feature family, but introduces hidden build/network behavior or a second first-use native download | Host/build variability and cache provenance | Same extra download and DLL lifecycle | Rejected; disable download features and bundle the verified runtime |
| Native Rust alternative such as Candle | Could require a different safetensors model conversion and XLM-Roberta/tokenizer integration; not the selected upstream ONNX export | Pure-Rust reduces `.so` payload but shifts large model-kernel and compatibility work into EducAI | Same unproven model-path work | Rejected for K2: higher integration/benchmark risk than the maintained ONNX CPU path |
| Pure-Rust ONNX engines such as `tract`; Burn ONNX | Potentially no native runtime, but transformer/operator coverage, optimization, tokenizer/model parity, throughput, and maintenance must be proven for this exact export | Less packaging complexity but materially higher inference uncertainty | Same unproven parity/performance work | Rejected as unsafe speculative substitutions |
| Static-link ONNX Runtime from source | Removes runtime path lookup but requires two reproducible, target-specific C/C++ builds and complicates security/update/audit | Build and GLIBC provenance burden | MSVC/static redistribution burden | Rejected for initial K2 |
| First-use runtime download | Saves installer bytes but makes semantic setup two downloads and introduces executable download/update behavior | User may fail offline before use | Same, plus native DLL lifecycle | Rejected for non-technical UX |

## Consequences

- K2 is unblocked at the design level, but no runtime packaging or Windows
  Knowledge validation has passed yet.
- The existing Windows HUMAN-PASS remains the PASS for the released feature
  set only. The future Knowledge native runtime **REQUIRES WINDOWS VALIDATION
  AFTER INTEGRATION**.
- Linux AppImage packaging and its host graphics boundary remain unchanged in
  this decision. Future runtime packaging is not PASS until it has passed the
  controlled build's extracted-payload checks and target-machine validation.
- This decision preserves local-only inference, no daemon, no Python, and no
  remote embedding fallback, while keeping the model's large payload out of
  product media.
