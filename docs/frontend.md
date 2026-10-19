# Vue frontend

All pages (speech synthesis, providers, voice cloning, configuration, first-run
setup, admin login and token management) live in `frontend/src`. Vue renders
precompiled templates and owns each workflow through components and composables.
Typed services in `src/services` provide the only HTTP entry point. Page data comes from Axum, including session CSRF
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

`npm run typecheck` runs catalog parity checks and `vue-tsc --noEmit`, including
Vue single-file components and template expressions. The production build runs
these checks before Vite compilation.

## Workspace and languages

The studio has separate Create speech, Voice library, Engines and Settings views.
Switching views retains the speech draft, voice upload and unsaved settings.
Settings initially show the Server category; searching includes every category.
Model availability updates in place instead of reloading the browser. Choosing
Use on a saved voice opens the speech composer.

The interface supports **English (`en`) and Spanish (`es`)**. The language menu
is available in the studio, setup and administration pages. The first visit uses
the browser language (Spanish variants select Spanish; other languages fall back
to English). An explicit selection is remembered in `sonicboom.locale` in local
storage. Switching languages does not reload the page or change the selected
speech language. Speech languages are supplied by each engine independently.

Translations live in `frontend/src/i18n/en.json` and `es.json`. Vue templates use
`t(key, parameters)`. Shared async tasks store `Message` objects so loading,
validation and error feedback react to language changes without replacing form
controls. Names, paths, model identifiers and raw diagnostics remain data.
Known server messages are localized; unknown server messages retain their text.

To add a language later:

1. Copy `en.json` to a new locale file and translate every value, retaining
   message keys and named parameters such as `{count}`.
2. Import the catalog in `src/i18n/index.ts`, register it in `catalogs`, and add
   its code and native name to `locales`. Extend initial browser-language
   matching for the new language.
3. Run `npm run build`. The build automatically checks every catalog for
   matching keys, nonempty strings and matching interpolation parameters.
4. Check the new language on setup, admin, each workspace view and mobile.
   Include live status messages and unsaved form values when switching language.

Only English and Spanish are offered until another complete catalog is added.
The UI uses the English catalog as a fallback for future untranslated messages.


## Component architecture

- `AppShell` owns the shared header, language switcher, skip link and footer.
- `StudioLayout` owns responsive navigation, keyboard focus and stable panels.
- `SpeechGenerator` / `VoiceSelector` own synthesis and searchable voice choices.
- `VoiceLibrary` / `CloneWizard` manage profiles and multipart cloning;
  `features/cloning/useCloneWizard` coordinates the engine/reference/review steps.
  Server-discovered limits drive local WAV inspection and transcript validation.
  The lazy reference editor offers explicit waveform preview, exact sample-frame
  cropping and WAV export while retaining the original recording.
- `EnginesPanel` / `EngineCard` consolidate provider status, models and toggles.
- `SettingsPanel` / `SettingField` group frequent and advanced settings and keep
  unsaved values during polling, language changes and filtering.
- `useStudio` shares API data through an epoch-scoped TanStack Query cache;
  `useSynthesis` owns job monitoring and object URLs;
  `useTask` / `FeedbackNotice` share loading and feedback behavior.
- `services/http.ts` attaches same-origin credentials and optional bearer auth;
  `services/api.ts` defines typed endpoints; `services/jobs.ts` reconciles dropped
  SSE connections without creating duplicate inference jobs.

No frontend code outside `services/http.ts` calls `fetch`. Theme colors are
semantic variables in `styles/tokens.css`; `prefers-color-scheme` updates all
pages and native controls immediately. There is no theme override in storage.

API responses are validated with Zod at the boundary. Structured failures retain
status, code, field issues, request correlation and retry advice. The schema
module disables Zod's optional JIT so the production CSP requires no eval.
Reka primitives provide dialog/combobox focus behavior without injected inline
styles. Shared fields expose accessible help and error associations.

## Verification

```sh
cd frontend
npm ci
npm run build
npm test
npm run test:unit
npx playwright install --with-deps chromium firefox
npm run test:e2e
```

The browser suite builds production assets and serves them using the exact CSP
from `src/security.rs`. Mock APIs exercise desktop Chromium, Firefox and mobile
Chromium without model downloads. Tests cover all studio tabs, setup/admin,
keyboard interactions, axe checks, English/Spanish, light/dark, reduced motion
and narrow/wide layouts. They do not measure inference quality or GPU behavior.
See [audit implementation evidence](ui/AUDIT-IMPLEMENTATION.md) for scope and
native hardware verification limits.
