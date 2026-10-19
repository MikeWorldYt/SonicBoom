# SonicBoom studio tour — 480 px demo

Visual guide to the redesigned studio at a 480×480 viewport (mobile), with
1280×800 desktop references. Shots are captured with Playwright against the
mocked API fixtures, so every screen renders without a backend or model.

Regenerate after any UI change:

```bash
cd frontend
npm run build
node --experimental-strip-types tour/capture.mts
```

The script serves `dist/` through `tests/e2e/server.mjs`, walks every tab in
English light/dark schemes, and writes this folder. It exits non-zero when a
screen fails to render, so it doubles as a smoke check.

## Create — editor first, result beside it

![Create at 480 px](tour-480/m480-create-light.png)

One label, one counter, one primary action. Byte counts appear only near the
server limit; the last voice, model, and language are remembered. Every
finished generation shows a waveform player (wavesurfer.js) with its own
transport, so the recording can be scrubbed without the native controls.

![Create with a finished generation](tour-480/m480-result-light.png)
![Create in dark mode](tour-480/m480-create-dark.png)

## Voice library — compact cards, creator in a dialog

![Voice library at 480 px](tour-480/m480-voices-light.png)

Search stays visible; engine/language/model/sort filters collapse into
`Filters` with an active-count badge. Each card shows a status dot and hides
technical detail behind `Details`.

![Voice creator dialog at 480 px](tour-480/m480-creator-light.png)
![Voice creator with a reference file](tour-480/m480-creator-file-light.png)

`New voice` opens an accessible modal (focus trap, `Esc` to close,
full-height scroll on small screens) hosting the three-step clone wizard.
`Esc` returns focus to the button that opened it. The reference step uses a
drag-and-drop zone that turns into a file card with the recording details, a
duration hint and a waveform player; the trim editor keeps its waveform and
stats while health advice collapses into `Recording health notes`.

## Engines — status you can scan, diagnostics on demand

![Engines at 480 px](tour-480/m480-engines-light.png)

Three status dots (enabled / model loaded / ready) plus one `?` tooltip
replace the diagnostic dump; engine-specific notes and the Vulkan/CPU solution
live behind disclosures. Pocket's six per-model Activate buttons collapsed
into one model picker with a single Activate action.

## Settings — one line per setting

![Settings at 480 px](tour-480/m480-settings-light.png)

Internal keys (`audio.output_device`, …) stay hidden behind `Show internal
keys`. Each row saves on its own with an apply badge.

## Desktop references (1280×800)

![Create on desktop](tour-480/d1280-create-light.png)
![Voice library on desktop](tour-480/d1280-voices-light.png)
![Voice creator on desktop](tour-480/d1280-creator-light.png)
![Engines on desktop](tour-480/d1280-engines-light.png)
![Settings on desktop](tour-480/d1280-settings-light.png)
