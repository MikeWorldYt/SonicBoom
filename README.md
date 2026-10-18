# SonicBoom

A high-performance web server that generates Text-to-Speech (TTS) audio using the Supertonic 3 ONNX model and delivers it via HTTP.

## Documentation Index

This README serves as an index to all SonicBoom documentation:

| Document                                | Description                         |
| --------------------------------------- | ----------------------------------- |
| [API Reference](docs/api.md)            | Complete API documentation          |
| [OpenAI-Compatible API](docs/openai.md) | OpenAI TTS API compatible endpoints |
| [Admin Panel](docs/admin.md)            | Admin panel guide                   |
| [Configuration](docs/config.md)         | TOML, first-run setup and live settings |
| [Security](SECURITY.md)                   | Security policy and credential rotation |

---

## Frontend development

The interface uses Vue, TypeScript and Vite, with English and Spanish localization
and dedicated speech, voice library, engine and settings views. See the [frontend guide](docs/frontend.md)
for localhost hot reload and embedded production builds. Building the server
requires Node.js 22.12+ and npm; deployed binaries need neither.

## Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/nglmercer/SonicBoom.git
cd SonicBoom
git submodule update --init --recursive

# Default build: Supertonic 3 + Pocket TTS + Qwen3-TTS on CPU.
# GPU backends are explicit opt-ins; no GPU SDK is needed here.
cargo build --release

# Desktop system-tray GUI:
cargo build --release --features gui
```

### GPU acceleration

Qwen's Vulkan backend is opt-in (`--features qwen-gguf-vulkan`). Install the
[Vulkan SDK](https://vulkan.lunarg.com/sdk/home) before building. On Windows,
restart the terminal after installation, or set the SDK path in PowerShell:

```powershell
$env:VULKAN_SDK = 'C:\VulkanSDK\<version>'
$env:Path = "$env:VULKAN_SDK\Bin;$env:Path"
cargo build --release --features qwen-gguf-vulkan
```

To build the same application with Qwen on CPU:

```bash
cargo build --release --no-default-features --features server,playback,qwen-gguf,pocket-tts,gui
```

Two independent acceleration stacks exist; enabling one never enables the
other. A requested GPU backend whose SDK is missing fails the build **early**
with an actionable message (it never silently builds CPU-only or silently
selects a different backend).

```bash
# ONNX Runtime (Supertonic / Pocket TTS) — enable at most ONE per platform:
cargo build --release --features cuda     # NVIDIA CUDA (Linux/Windows)
cargo build --release --features rocm     # AMD ROCm (Linux)
cargo build --release --features coreml   # Apple Silicon CoreML (macOS)

# Qwen3-TTS native llama.cpp backend (GPU backends are opt-in):
cargo build --release --features qwen-gguf-vulkan   # Vulkan SDK + SPIRV-Headers + glslc
cargo build --release --features qwen-gguf-cuda     # CUDA Toolkit (nvcc)

# Both llama.cpp backends, only when both SDKs are installed:
cargo build --release --features qwen-gguf-cuda,qwen-gguf-vulkan
```

`cuda`/`rocm`/`coreml` are ONNX Runtime flags and never activate the native
llama.cpp backend; `qwen-gguf-cuda`/`qwen-gguf-vulkan` are the only flags that
do. The build script honours CMake's own discovery: set `CUDAToolkit_ROOT`,
`CUDACXX`, `CUDA_PATH`/`CUDA_HOME`, `VULKAN_SDK`, or `CMAKE_PREFIX_PATH` to
point at an SDK in a non-standard location. Diagnostics and the
`SONICBOOM_SKIP_SDK_CHECK=1` escape hatch are documented in
[docs/providers.md](docs/providers.md#build-time-sdk-validation).

### Run

Desktop builds open a first-run setup wizard. No `.env` file is needed:

```bash
cargo run --release --features gui
```

Choose local or LAN mode, an audio output, model storage, and admin
credentials. Local mode binds loopback without an API token. LAN mode
requires bearer authentication and shows its initial token once.
Configuration lives in the platform application directory as `config.toml`.

For an unattended server, keep using environment variables:

```bash
export PORT=17842
export BIND=127.0.0.1       # use 0.0.0.0 for deliberate LAN exposure
export SONICBOOM_ADMIN_ID=admin
export SONICBOOM_ADMIN_PW=<GENERATE-A-RANDOM-PASSWORD>
export ALLOWED_AUDIO_DIR=./audio
cargo run --release
```

Use a unique password of at least 12 characters. API token hashes remain
in `tokens.json`, which is initialized safely when missing. The admin panel
(`/admin`) displays each newly created token once.

Edit `config.toml` or use `SonicBoom config set` to apply settings live.
See the [configuration guide](docs/config.md) for location overrides,
revisions, migration, source tracking and runtime apply status.

The server will:

1. Start listening on port 17842
2. Download the Supertonic 3 model (first run)
3. Load the model
4. Be ready to serve TTS requests

---

## Features

- **ONNX Runtime Inference** - Supertonic 3 with hardware acceleration (CoreML on Apple Silicon, CUDA/ROCm on Linux via `--features cuda`/`--features rocm`)
- **Buffered Audio Output** - Opus/OGG, WAV, MP3, and FLAC audio output (each response is one fully encoded buffer, not an incremental stream)
- **Token-Based Authentication** - API access control
- **Verified Model Supply Chain** - Pinned revision with compiled-in SHA-256 integrity checks
- **Admin Panel** - Web-based management interface
- **OpenAI-Compatible API** - Drop-in replacement for OpenAI TTS
- **Audio Queue System** - Play audio files directly on the server with queue management
- **Output Device Selection** - Discover and switch server audio outputs (speakers, HDMI, virtual cables) via API
- **Session Management** - Secure admin sessions with lockout protection
- **Desktop Tray (optional)** - System tray GUI via the `gui` feature (macOS/Windows/Linux; Linux uses StatusNotifierItem over D-Bus, no GTK build dependencies)

---

## API Endpoints

### Original TTS API

| Method | Endpoint        | Description                   |
| ------ | --------------- | ----------------------------- |
| `POST` | `/api/tts`      | Generate TTS audio            |
| `POST` | `/api/tts/play` | Synthesize and play on server |
| `GET`  | `/api/status`   | Check model status            |

### Audio Queue API

| Method | Endpoint            | Description                   |
| ------ | ------------------- | ----------------------------- |
| `POST` | `/api/queue`        | Add file to playback queue    |
| `POST` | `/api/queue/next`   | Play next item in queue       |
| `POST` | `/api/queue/pause`  | Pause playback                |
| `POST` | `/api/queue/resume` | Resume playback               |
| `POST` | `/api/queue/stop`   | Stop playback and clear queue |
| `POST` | `/api/queue/volume` | Set playback volume           |
| `GET`  | `/api/queue/status` | Get current queue status      |

### Audio Output Device API (playback builds)

| Method | Endpoint            | Description                        |
| ------ | ------------------- | ---------------------------------- |
| `GET`  | `/api/audio/devices`| List output devices + selection    |
| `GET`  | `/api/audio/output` | Get active output device           |
| `POST` | `/api/audio/output` | Switch output device at runtime    |

### OpenAI-Compatible API

| Method | Endpoint                  | Description                  |
| ------ | ------------------------- | ---------------------------- |
| `POST` | `/v1/audio/speech`        | Generate TTS (OpenAI format) |
| `GET`  | `/v1/models`              | List models                  |
| `GET`  | `/v1/models/list`         | List models (alias)          |
| `GET`  | `/v1/voices`              | List voices                  |

### Voice Cloning (optional native providers)

| Method   | Endpoint                      | Description                              |
| -------- | ----------------------------- | ---------------------------------------- |
| `GET`    | `/api/providers`              | List providers, capabilities, and status |
| `GET`    | `/api/providers/{id}/status`  | One provider's lifecycle status          |
| `POST`   | `/api/voices/clones`          | Upload reference WAV, create a profile   |
| `GET`    | `/api/voices`                 | List your voice profiles                 |
| `GET`    | `/api/voices/{id}`            | One voice profile                        |
| `DELETE` | `/api/voices/{id}`            | Delete a profile and its reference audio |
| `POST`   | `/api/voices/{id}/preview`    | Synthesize text with a profile           |

See [docs/providers.md](docs/providers.md) for Pocket TTS and Qwen3-TTS
setup (disabled by default; CPU works without GPU). Qwen means the native
1.7B GGUF/ONNX backend (`qwen3-gguf`, opt-in `--features qwen-gguf`
build, ICL cloning only, CPU-validated); the old Candle backend was
removed.

### Admin Panel

| Method   | Endpoint                       | Description      |
| -------- | ------------------------------ | ---------------- |
| `GET`    | `/admin`                       | Admin dashboard  |
| `GET`    | `/admin/login`                 | Login page       |
| `POST`   | `/admin/login`                 | Admin login      |
| `POST`   | `/admin/logout`                | Admin logout     |
| `POST`   | `/admin/tokens`                | Create new token |
| `POST`   | `/admin/tokens/{id}/revoke`    | Revoke token     |

### Web Routes

| Method | Endpoint  | Description                        |
| ------ | --------- | ---------------------------------- |
| `GET`  | `/`       | Home page (TTS demo, needs token)  |
| `GET`  | `/health` | Liveness probe                     |
| `GET`  | `/ready`  | Readiness probe (model loaded)     |

---

## Usage

### Generate TTS Audio

```bash
# Using original API
# Audio is encoded as Opus inside an OGG container (Content-Type: audio/ogg; codecs=opus).
curl -X POST "http://localhost:17842/api/tts?voice=F1" \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -d "Hello, world!" \
  --output audio.ogg

# Using OpenAI-compatible API
curl -X POST http://localhost:17842/v1/audio/speech \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"input": "Hello, world!", "voice": "alloy"}' \
  --output audio.ogg
```

---

## Technology Stack

| Component      | Technology                               |
| -------------- | ---------------------------------------- |
| Runtime        | [Tokio](https://tokio.rs/)               |
| Web Framework  | [Axum](https://github.com/tokio-rs/axum) |
| ONNX Inference | [ORT](https://github.com/DBDi/ort)       |
| Audio Encoding | [Opus](https://opus-codec.org/)          |
| Serialization  | [Serde](https://serde.rs/)               |
| Logging        | [Tracing](https://tokio.rs/blog/tracing) |

---

## Project Structure

```textplain
SonicBoom/
├── src/
│   ├── main.rs              # Application entry point
│   ├── config/             # Central ConfigManager, TOML, validation and watcher
│   ├── runtime/            # Subsystems and live reconfiguration
│   ├── server/             # Restartable HTTP listener
│   ├── setup/              # First-run wizard
│   ├── error.rs             # Error types
│   ├── logging.rs           # Logging setup
│   ├── admin/               # Admin panel
│   │   ├── handlers.rs      # Admin HTTP handlers
│   │   ├── lockout.rs       # Failed-login IP lockout
│   │   ├── session.rs       # Admin session helpers
│   │   └── templates.rs     # Session-protected Vue page data
│   ├── api/                 # API handlers
│   │   ├── tts.rs           # Original TTS API
│   │   ├── openai.rs        # OpenAI-compatible API
│   │   ├── queue.rs         # Audio queue API
│   │   └── audio.rs         # Output device API
│   ├── auth/                # Authentication
│   │   ├── store.rs         # Token storage
│   │   └── token.rs         # Token types/validation
│   ├── tts/                 # TTS engine
│   │   ├── audio.rs         # Audio encoding (Opus/OGG/MP3/FLAC/WAV)
│   │   ├── download.rs      # HuggingFace model download
│   │   ├── inference.rs     # ONNX inference
│   │   ├── model.rs         # Model loading
│   │   ├── queue.rs         # Server-side playback queue
│   │   ├── devices.rs       # Output device discovery
│   │   └── text.rs          # Text normalization
│   └── web/                 # Web frontend
│       └── index.rs         # Home page
├── docs/
│   ├── api.md              # API reference
│   ├── openai.md           # OpenAI API guide
│   ├── admin.md            # Admin panel guide
│   └── config.md           # Configuration guide
├── frontend/               # Vue components, TypeScript controllers, Vite
├── build.rs                # Build and embed production UI
├── Cargo.toml
├── Dockerfile
└── docker-compose.yml
```

---

## Docker

```bash
# Build and run with Docker (headless CPU server; no local playback)
docker build -t sonicboom .
docker run -p 127.0.0.1:17842:17842 \
  -e SONICBOOM_ADMIN_ID=admin \
  -e SONICBOOM_ADMIN_PW=<GENERATE-A-RANDOM-PASSWORD> \
  -e HF_TOKEN=your_hf_token \
  -v sonicboom-tokens:/app/data \
  sonicboom

# With local ALSA playback:
docker build --target runtime-playback -t sonicboom:playback .
# With CUDA acceleration (compiles with --features cuda):
docker build --target runtime-cuda -t sonicboom:cuda .
```

Or use docker-compose:

```bash
export SONICBOOM_ADMIN_PW=<GENERATE-A-RANDOM-PASSWORD>
docker compose up -d            # CPU server
docker compose --profile playback up -d   # with local playback
docker compose --profile cuda up -d       # CUDA server
```

Images run as non-root, use digest-pinned bases, and harden the container
(`no-new-privileges`, dropped capabilities, resource limits). See
[Configuration](docs/config.md#container-hardening) for details.

---

## Acknowledgments

- [Supertonic 3](https://huggingface.co/Supertone/supertonic-3) - The TTS model
- [ONNX Runtime](https://onnxruntime.ai/) - Cross-platform ML inference
- Upstream project: [daramkun/SonicBoom](https://github.com/daramkun/SonicBoom)
  (this repository, [nglmercer/SonicBoom](https://github.com/nglmercer/SonicBoom),
  is a hardened fork)

## Reliable native synthesis jobs

Use the authenticated [background job API](docs/inference-jobs.md) for long CPU
inference, with progress and cooperative cancellation. The synchronous routes
remain supported. See [ownership/cache architecture](docs/inference-architecture.md)
for model residency, integrity verification, reloads and cache bounds.

## Binary releases

See [release instructions and runtime requirements](docs/RELEASING.md) for
supported Linux/Windows x64 archives, checksums, smoke tests and troubleshooting.
