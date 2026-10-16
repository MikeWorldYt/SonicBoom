# TikTools plugin TTS contract

Implemented against SonicBoom `d600ad1` and TikTools-app `278ca29` (2026-10-16).
The companion integration selects a live provider through the isolated plugin broker.
See [API documentation](api.md) for the two authenticated option sources and optional
`provider` playback query. Inference engines, resident model lifecycle, OpenAI speech
and HTTP job contracts are unchanged.

## Ownership and playback

Catalogs expose only enabled providers, including enabled-but-unavailable rows. Only
ready providers are selectable. Voice IDs are provider-qualified; playback strips the
prefix only after verifying it agrees with the explicit provider. Omitted/empty
provider retains legacy Supertonic routing. Explicit Supertonic rejects unknown styles;
cloning requires a token-owned, current-revision profile for the resident model.
Pocket language models are separate; a profile for a different resident model is not
listed. Qwen continues to use its validated Base ICL mode.

Profiles are read through the existing manifest/cache-stamp and ownership functions.
Decoded references use the existing memory admission path. All provider synthesis
runs under existing deadlines and bounded admission; all generated WAVs use one
secure temporary-file/queue helper. Cancellation detected during encoding or writing
withholds the audio; queue rejection removes the temporary file. Reloading the catalog
does not unload/reload models or alter playback.

## Reproducible validation

No weights/downloads are required for the new contract tests:

```bash
cargo test --bin SonicBoom plugin_ -- --nocapture
cargo test --no-default-features --features server,playback,pocket-tts --bin SonicBoom plugin_
cargo check --no-default-features --features server
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Results: six plugin tests passed in both feature configurations. Default workspace:
170 library tests passed (7 ignored), 234 server tests passed, one isolation integration
test passed; three real-artifact tests remain ignored by default. Formatting, Clippy
with warnings denied, and server-only compilation passed.

The tests cover authenticated/no-store catalog responses, enabled-vs-ready discovery,
legacy/explicit/qualified identities, missing/foreign/deleted profiles, revision/provider
mismatches, unavailable lifecycle states, language validation and exact cloned request
model/reference/mode mapping. Stub PCM is encoded and inspected at the actual queue
boundary: 2,400 samples at 24 kHz; normal enqueue vs immediate playback; queue-full
cleanup. This is contract evidence, not speech-quality or audible-device evidence.

## Remaining validation limits

- Full Pocket-only server tests: 232 passed, one existing failure in
  `api::openai::tests::supported_models_are_accepted`, whose unguarded assertion expects
  the Qwen model to route as cloning even without `qwen-gguf`. The file/test was not
  changed. The six new plugin tests pass in that configuration.
- Real-model generation and audible playback: NOT RUN for this integration. Model
  quality was accepted separately; no new fidelity claims or engine changes are made.
- GPU/native OS WebView execution: NOT RUN. CPU default features compile/test; plugin
  isolation/iframe transport is exercised by the companion TikTools Playwright suite.
