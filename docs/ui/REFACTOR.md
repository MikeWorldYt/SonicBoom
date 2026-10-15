# UI refactor delivery

## Phase 1: visual system

- One semantic palette in `frontend/src/styles/tokens.css`.
- System light/dark preference applies immediately, including native inputs.
- Studio, setup and admin share `styles/ui.css`; four overlapping sheets removed.
- Focus rings, touch targets, borders and text use semantic tokens.
- Verified: Vite production build; browser setup, synthesis UI, settings, admin
  token lifecycle, EN/ES, responsive layout and live system theme change.

Before: [desktop](before/studio-desktop.png), [mobile](before/studio-mobile.png),
[setup](before/setup.png).

After visual system: [light](visual-system/light.png),
[dark](visual-system/dark.png), [mobile](visual-system/mobile.png).

The screenshots use isolated test data; generated audio and model readiness are
stubbed. They document UI behavior, not inference performance.

## Phases 2–4: layout, Vue modules and workflows

- Reusable `AppShell` and `StudioLayout`; one heading and one refresh action per
  workspace; desktop/tablet/mobile navigation with roving keyboard focus.
- Removed both DOM controllers and the element-ID/type registry.
- Shared, strict TypeScript services and async feedback; native Vue form state.
- Searchable voice cards with engine/language metadata and preserved selection.
- Three-step cloning wizard: engine, recording/transcript, review/consent.
- Consolidated engine/model cards; frequent settings first, full category/search
  access and advanced file tools. Dirty fields survive refresh and filtering.
- Original endpoint methods, raw WAV upload, bearer/session authentication,
  CSRF forms, generation/cancellation/resume and output download are preserved.
- Model polling does not reload the browser; an audio-download retry reuses its
  completed job instead of submitting new inference.

After: [studio light](after/studio-light.png), [studio dark](after/studio-dark.png),
[mobile](after/mobile-dark.png), [settings](after/settings-dark.png),
[voices](after/voices-dark.png), [engines](after/engines-dark.png),
[setup ES](after/setup-dark-es.png), [admin login ES](after/login-dark-es.png).

Verified during implementation: strict TypeScript + Vite production build,
14 service/component tests and 5 browser scenarios (EN/ES generation,
cancellation, upload, settings, live theme changes, mobile/tablet layouts,
keyboard navigation, all-panel axe audits, palette and native-control contrast).

## Phase 5

Production Rust embedding/CSP/session verification and CI integration follow in
the dependent quality PR. These screenshots use deterministic API fixtures;
real model-backed synthesis is separate from UI protocol and accessibility tests.
