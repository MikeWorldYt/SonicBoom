# Vue frontend

All pages (speech synthesis, providers, voice cloning, configuration, first-run
setup, admin login and token management) live in `frontend/src`. Vue renders
precompiled templates; synthesis and configuration interactions
are TypeScript controllers. Page data comes from Axum, including session CSRF
tokens and the one-time token banner. Tokens are never saved in browser storage.

Requires Node.js 22.12+ (Node 24 recommended) and npm. Versions are pinned in
`frontend/package.json` and `package-lock.json`.

## Local development

Run the backend and Vite in two terminals:

```sh
SONICBOOM_FRONTEND_DEV=1 cargo run --no-default-features --features server
```

```sh
cd frontend
npm ci
npm run dev
```

Open **http://localhost:5173**. Vite serves TypeScript, Vue components and hot
updates. Page requests and APIs are proxied to `http://127.0.0.1:17842`.
The proxy preserves admin session cookies and rewrites the local Origin header
to match the backend's same-origin checks. Use localhost:5173 throughout admin
login, logout and token creation. First-run setup uses the same proxy.

For another backend port, set `SONICBOOM_BACKEND_URL=http://127.0.0.1:PORT`
when starting Vite. Backend networking still comes from SonicBoom configuration.
If setup changes the backend port, restart Vite with the new target and reopen
localhost:5173. The setup completion dialog includes the new API address.

The development asset switch is honored only by debug binaries. Vite needs
injected CSS for hot updates, so this explicit mode permits inline styles and
the localhost hot-update WebSocket. Production retains the strict CSP.

## Production build

```sh
cargo build --release
```

Cargo's `build.rs` installs locked npm dependencies if needed, typechecks the
TypeScript, runs Vite and embeds the resulting HTML, JavaScript and CSS in the
Rust executable. Source changes trigger a rebuild. Node and the frontend source
are needed on the build machine only. The executable serves hashed `/assets/`
URLs with immutable caching and works from any directory without asset files,
a frontend server, Node.js or an internet connection for the UI.

For a separate frontend build (as used by Docker):

```sh
npm ci --prefix frontend
npm run build --prefix frontend
SONICBOOM_FRONTEND_PREBUILT=1 cargo build --release
```

Use the prebuilt flag only after building the current frontend source. Plugin
builds without the `server` feature do not build or embed the frontend. Docker
builds the UI in a Node stage and copies its output into each Rust builder;
CI and release workflows install Node before Cargo builds.

`npm run typecheck` checks TypeScript with the current TypeScript compiler;
Vite's Vue plugin compiles and validates the single-file component templates.
