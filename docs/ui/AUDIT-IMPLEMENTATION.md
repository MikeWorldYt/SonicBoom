# Web audit implementation status

Baseline: 2026-10-19 audit supplied by the user. This file records implemented
work and remaining merge criteria; it does not certify the full audit as done.

## Initial implementation

- Shared Rust reference bounds back the constraints endpoint, WAV decoding,
  profile validation and both native provider duration validators.
- Upload schema minimum is 1024 bytes. Upload cap enforcement reads current
  configuration per request, including aggregate multipart overhead.
- Authenticated versioned constraints endpoint derives modes/models/languages
  from the registered providers.
- Multipart clone creation shares authoritative validation, admission gates,
  ownership and profile storage with the legacy route.
- Studio no longer sends transcript or consent in URLs.
- List/create summary responses omit private metadata. Detail exposes transcript
  only after the existing owner authorization check.
- Client WAV inspection bounds allocation by upload size and checks chunk
  integrity, PCM/float format, channels, original rate, duration and finite float
  samples. No AudioContext or implicit audio conversion is used.
- Wizard previews the selected reference, shows measured metadata and transcript
  count, refreshes limits before validation/save, and preserves failed drafts.
- Constraints boundary validation fails closed on invalid shapes/version.
- ApiError retains code, request ID, retry delay and optional field details;
  generic error display includes correlation ID.
- SFC/template checks run during typecheck/build with vue-tsc. TypeScript 5.9.3
  is pinned because the installed vue-tsc cannot load TypeScript 7's tsc entry.
- Comparison history revokes evicted object URLs.

## Second implementation increment

- Shared form fields expose labels, help/error associations and invalid state;
  file dropzone retains a native keyboard file picker.
- Cloning retains creation report with profile ID, server metrics and warnings,
  with explicit Try voice/Create another actions. Validation errors focus the
  relevant field after controls are re-enabled.
- Creation response is validated with Zod before rendering.
- Voice library supports query/provider/language/model filtering and sorting;
  deletion uses a focus-managed Reka dialog with explicit consequences.
- Setup keeps its one-time token in memory, masked with reveal/copy/save and
  required acknowledgement before navigation. Audio test persistence is disclosed.
- Admin expiry explicitly uses UTC+00:00 and validates future dates before the
  existing native form/CSRF submission. New token uses the shared secret control.
- Component tests cover fields, cloning failure/retry/cap changes, secrets and
  expiry. Initial Chromium desktop/mobile Playwright tests include axe checks for
  setup and admin. Existing 26 Node tests remain.
- CI includes frontend build/typecheck, legacy/component and browser tests.

## Third implementation increment

- In-memory TanStack Query cache deduplicates concurrent resource requests;
  snapshots/schema use explicit staleness rules and mutations refresh resources.
  Cache keys use opaque epochs, never tokens. Token changes abort requests and
  clear scoped data; generation stops publishing old audio immediately.
- Shared Rust sampling bounds and current text/body limits are exposed in info.
  Sampling controls validate decimal/comma input, integer ranges and full u64
  seeds before serialization, with accessible field feedback.
- Runtime response schemas cover metadata, config/schema/status, voices,
  creation, setup and jobs, including SSE data. Malformed responses fail closed.
- HTTP errors distinguish cancellation, timeout, offline/network, invalid JSON,
  content type and contract mismatch, while preserving correlation/retry advice.
  Long inference paths have no arbitrary metadata timeout; 429 cooldowns avoid
  repeated network calls. SSE reconnects reconcile the original job only.
- Settings preserve drafts on background changes and require explicit conflict
  review. Concurrent revision errors refresh current values without overwriting
  drafts.
- Searchable voice combobox virtualizes lists over 100 and supports keyboard
  navigation; browser regressions exercise 500 entries.
- Browser fixtures cover studio speech comparison, clone creation/deletion,
  provider readiness, settings conflicts and large voice selection as well as
  setup/admin. Axe runs against real desktop/mobile DOM.
- Server regressions cover malformed/duplicate multipart, MIME rejection,
  runtime cap changes and Unicode transcript boundaries.

## Final implementation increment

- Engine/reference/review components now share useCloneWizard coordination and
  localized field errors. Server constraints are refreshed before analysis/save;
  optimistic revision conflicts preserve drafts. Multipart admission and memory
  reservation precede body parsing. Legacy unknown query parameters stay compatible.
- Opt-in lazy waveform analysis measures source channels; exact-frame cropping
  retains rate/channels/encoding, original recording and explicit WAV export.
  Ancillary WAV metadata chunks are not copied. Local advice is non-blocking
  and is distinguished from server mono reference-health metrics.
- Shared controls, bootstrap/page error boundaries and setup policy validation
  are implemented. Settings expose revisions, conflicts and apply/restart states.
  Supertonic's existing studio languages now come from capability metadata;
  inference preprocessing/execution are unchanged.
- Browser tests build actual production assets under the exact CSP read from
  src/security.rs. Every test checks page errors and CSP violations. Zod runs
  without optional JIT, and the combobox avoids inline style injection; production
  permissions are not relaxed.
- Chromium, Firefox and mobile Chromium exercise comparison, cloning, deletion,
  500-voice selection, provider status, settings conflicts, trim/export/restore,
  setup secrets/policy, admin UTC, English/Spanish, dark/light, reduced motion and
  narrow/wide layouts. Axe checks cover every studio tab plus setup/admin.
- Component regressions include malformed/boundary WAV, provider switching,
  Unicode name/transcript bounds, consent, field focus, HTTP rejection/draft
  retention, duplicate saves, network/cache/jobs/settings and setup validation.
  Token and hot auth-mode changes invalidate scoped data and previously owned
  audio. Existing Node regression checks remain intact.

## Acceptance mapping

| Audit phase | Principal implementation evidence |
| --- | --- |
| 1: constraints/config | Shared Rust constants, live constraints endpoint, hot cap and schema minimum tests |
| 2: multipart/privacy/errors | Bounded multipart/legacy dispatch, summary DTOs, structured errors, API regressions |
| 3: cloning | Extracted steps/composable, WAV analysis, reactive validation, retained reports, component/browser tests |
| 4: UI | Shared accessible fields/buttons/dropzone/dialog/combobox, keyboard and axe checks |
| 5: checking/testing | vue-tsc templates, retained Node tests, Vitest, production-CSP Playwright, CI |
| 6: HTTP/cache/jobs | Runtime schemas, typed errors, scope invalidation, resource cache, same-ID SSE reconciliation |
| 7: speech/audio | Server sampling/text bounds, locale numbers, u64 seeds, four comparisons, URL cleanup, explicit trim |
| 8: library/providers/settings | Filters/sort/delete/readiness/revisions, capability languages, schema validation/conflicts/apply state |
| 9: setup/admin/docs | In-memory one-time secrets, discovered policy, UTC expiry, locale/layout checks, API/frontend docs |

## Deliberate boundaries

Legacy raw-WAV/query cloning remains supported and deprecated. The studio never
puts transcript or consent in URLs; removing the legacy semantics requires a
separately announced API migration. Rust remains authoritative for all clients.
Revision checks detect observed conflicts, not an atomic transaction spanning
configuration and profile storage.

No microphone requests, ASR weights or automatic transcription are introduced.
The optional separate analyze/upload-ID endpoint and future offline ASR remain
future work. No transcript correctness is claimed. Existing service/composable
locations are migrated incrementally instead of introducing duplicate API/state
layers. Local reactive forms, the waveform player and lazy SVG waveform cover
behavior
without requiring every recommended library or target component filename.

Automated keyboard/axe checks are not manual screen-reader certification. Browser
mock APIs verify UX/contracts, not inference quality, latency or actual model
execution. Production still embeds assets in Rust and needs no Node runtime.

## Verification commands

Verified on 2026-10-19: 32 existing/expanded Node tests, 46 Vitest component tests,
30 Playwright tests across three browser projects, Vue/template/catalog checks,
production build, formatting, strict all-target Clippy and native-source preflight
passed. Default Rust suites passed 468 tests; 10 network/real-model tests stayed
ignored. GPU and real-model execution were not certified.

From frontend: npm ci, npm run typecheck, npm test, npm run test:unit,
npm run build, npx playwright install --with-deps chromium firefox,
npm run test:e2e. From the repository root: cargo fmt --check,
cargo clippy --locked --all-targets -- -D warnings, cargo test --locked.

CI runs frontend build/typecheck, existing Node/component tests and all three
browser projects. Browser endpoints are mocked and no model weights are fetched.

## Native feature/hardware verification matrix

| Build | Automated scope here | Additional release validation |
| --- | --- | --- |
| Default server/playback/gui/Qwen/Pocket | Linux CPU formatting, all-target Clippy, Rust tests, embedded assets | Real model download/synthesis/quality, audio/tray smoke tests on supported OSes |
| --no-default-features --features server | Lightweight API/server tests | Deployment smoke test on target platform |
| --features qwen-gguf-vulkan | Requires Vulkan SDK/toolchain | Real Vulkan device/driver, model load, synthesis/cancel/concurrency |
| --features qwen-gguf-cuda | Requires CUDA toolkit | Supported NVIDIA GPU/driver, model load, synthesis/cancel/concurrency |
| ONNX cuda, rocm or coreml | Platform-specific execution provider | Corresponding OS/runtime/device; enable at most one of these features |
| Pocket TTS | Default build compiles CPU execution | Real downloaded bundles and multilingual sample smoke tests |

CPU tests cannot certify GPU execution. Existing ignored network/model tests
remain opt-in and are not reported as executed inference tests.
