## KNOWLEDGE LOCAL EMBEDDINGS PASS COMPLETE (2026-09-05)

- **K2:** verified multilingual-e5-small fp32, Rust tokenizer, schema v2 vectors, incremental current-generation reuse, exact project-local semantic retrieval, and K1 lexical preservation are complete. DOCUMENT EMBEDDING REMOTE LLM CALLS: ZERO. QUERY EMBEDDING REMOTE LLM CALLS: ZERO.
- **Linux package:** ONNX Runtime CPU 1.22.0 is checksum-gated from archive `8344d55f93d5bc5021ce342db50f62079daf39aaafb5d311a451846228be49b3` (7,798,730 bytes); final AppImage payload is only core/provider libraries plus required links at `usr/lib/educai/onnxruntime/`. Extracted artifact GLIBC gate PASS (core 2.27, provider 2.2), model excluded; real extracted-path E5 inference was 384 dimensions, norm 1.000000, OpenShift 0.913079 > photosynthesis 0.793906.
- **Model:** first-use checksum/atomic managed cache at `<app-data>/knowledge-models/intfloat--multilingual-e5-small/ccc66d3bcd826f577e26b9a4072cc5fe3a7ad6a3/`; model 470,268,510 bytes plus tokenizer artifacts, never AppImage content.
- **Windows:** CURRENT WINDOWS RELEASED FEATURE SET: **HUMAN-PASS**. KNOWLEDGE LOCAL EMBEDDINGS WINDOWS RUNTIME: **NOT YET HUMAN-VALIDATED**. **M11: NOT STARTED.**
- **Next:** HYBRID RETRIEVAL — FTS5 + SEMANTIC EXACT SEARCH + RRF (not started).
