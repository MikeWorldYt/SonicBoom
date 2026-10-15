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

## Remaining delivery

- Extract reusable shell and workspace layout.
- Replace the home DOM controller with Vue modules and typed services.
- Progressive voice upload, enriched voice selection and grouped settings.
- Component, workflow, accessibility, CSP and embedded production checks.
