# TikTools LIVE TTS compatibility

The TikTools SonicBoom backend uses `/api/tts/jobs` for Pocket and Qwen chat synthesis. Requests identify clones with `profile=<uuid>`, authenticate with the profile owner's API token, request WAV and `playback=enqueue&play_now=false`, and supply a stable principal-scoped idempotency key. A lost accepted response is recovered using that same key; clients must not retry with a fresh key.

The real job executor now rejects an explicit `provider` that disagrees with the authenticated profile. Existing ownership, resident-model, revision, language and clone-mode checks remain authoritative. Reference recordings are not included in diagnostics.

A completed synthesis is distinct from playback admission. Clients wait for `playback.state` to become `queued`, `started`, `failed` or `skipped` and retain the playback queue ID. Queued/started do not acknowledge audible completion. Failed admission leaves the retained job audio available under the existing ownership/retention rules. Existing audio progression, paused queues, device recovery and temporary-file ownership are unchanged.

The synchronous `/api/tts/play` HTTP route uses 15 seconds for Supertonic, 60 for Pocket, and 180 for Qwen when the generic server request timeout has its default 120-second value. Explicit non-default server request limits cap those budgets. Other routes keep their configured HTTP timeout. Expiration returns 408 with `stage=http_request_deadline`. Inference admission/deadlines and job cancellation remain independent; slow chat should use jobs.

Validation on 2026-10-17:

- `cargo test --no-default-features --features server,playback,pocket-tts,qwen-gguf`: pass (201 library, 250 server, 5 isolation, 1 framing; 10 opt-in tests ignored).
- `cargo clippy --no-default-features --features server,playback,pocket-tts,qwen-gguf --all-targets -- -D warnings`: pass.
- `cargo build --no-default-features --features server,playback,pocket-tts,qwen-gguf --bin SonicBoom`: pass.
- TikTools actual-process/local-HTTP tests cover all three providers, profile routing, replay bursts, bounded generation admission, and cancellation on disconnect/settings change/shutdown. A separate host HTTP test verifies automation settings inheritance.

See the companion TikTools report `docs/SONICBOOM-LIVE-TTS-VALIDATION.md` for the complete command matrix, baseline E2E failures and real-model measurements. No GPU-specific performance or human-audible completion is implied by mock results.
