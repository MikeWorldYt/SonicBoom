# Android implementation audit

- `Cargo.toml`: desktop defaults previously included server, GUI, rodio playback,
  Pocket and Qwen/Vulkan. `server` itself did not activate these. Added `web-ui`
  to desktop defaults, moved frontend build behind it, and added `android-ort`.
  Android target dependencies select ORT API 24/dynamic loading and Rustls HTTP
  TLS; desktop dependency defaults remain intact. TikTools still selects neither
  server, playback nor web assets.
- `build.rs`: previously ran npm for every server build. Now exits before reading
  frontend inputs unless `web-ui` is selected; prebuilt/CSP embedding is unchanged.
- `src/web`, `src/setup`, admin routing: health/readiness remain in every server.
  HTML/static/setup pages require web-ui; JSON setup APIs remain available. Admin session/token data remain usable
  as JSON without embedding assets. TTS/config/provider/voice API router is reused.
- `src/main.rs`, `src/config`: existing bootstrap, configuration writer, secret
  store, authentication validation and paths are reused. API-only incomplete
  initialization fails with instructions. Headless init persists credentials.
  Build information retains its existing keys and labels runtime as unchecked.
- `src/runtime`, `src/tts`: existing lifecycle, trusted downloads and bounded
  inference admission are reused. Defaults already serialize inference (1 + 8
  pending). Android Supertonic sessions default to two CPU threads, configurable
  without changing diffusion steps/output quality. Playback capability already
  reports compile-time exclusion correctly.
- `src/tts/providers`: Supertonic/Pocket/Qwen ONNX paths now explicitly initialize
  the runtime before session construction. No provider is reimplemented.
- `native/qwen-llama`: CPU CMake already disables OpenMP, GPU backends by default
  and host CPU tuning. Android now links `c++_shared`, avoiding GNU `stdc++` and
  a separate pthread library. Vendor sources remain pinned by submodule.
- Existing SIGINT/SIGTERM shutdown and atomic downloads/configuration are retained.
- Native execution evidence belongs in the results file; an NDK check alone does
  not establish model/runtime compatibility or Android GPU acceleration.

- Native runtime testing exposed an upstream ORT Android exit-order bug. The
  vendored rc.13 patch applies its existing Apple atexit strategy to Android;
  desktop destructor paths remain unchanged. Native probe execution must include
  normal process exit, since successful inference alone missed this crash.
