# Authorized synthetic reference fixtures

`synthetic-hola.wav` was generated locally using eSpeak NG 1.52.0's Spanish
voice at 150 words/minute. It is a synthetic voice, not a human recording. Its
exact input transcript is:

> Hola. Esta grabación es una voz sintética para una prueba independiente.

SHA-256: `66f9b9fccd61ec0994b67c32c630529fd8430d7a3e00b29bd65facf0a283fbfc`.
It provides the reference-containing-`hola` case; it is deliberately distinct
from the requested target `test`. The second reference is the existing English
synthetic eSpeak fixture under `benchmarks/pocket/results/2026-10-15-bc250/`,
whose exact input transcript and hash are in its `summary.json`. That reference
does not contain `hola`. Neither fixture establishes natural speaker similarity.
These diagnostic derivatives carry no human voice consent claim. eSpeak NG is
GPL-3.0-or-later; no eSpeak engine code or binaries are redistributed here.

`synthetic-sin-hola.wav` uses eSpeak NG's Spanish `es+f3` voice at 150 wpm.
Exact transcript: `La lluvia cae sobre la ciudad.`
SHA-256: `503f65df1fc870fd7f603749889830d9c964f20a6c51e256bbbee0bbc9c911be`.
The final Qwen A/B run uses these two Spanish references, so a cross-language
reference is not a confound in the reference-echo test.

`qwen-prompt.json` is produced by calling the pinned MIT upstream PromptBuilder
with deterministic synthetic speaker embeddings/acoustic codes. It covers both
text-trailing and text-padding ICL cases, exact combined token IDs and SHA-256 of
every little-endian f32 prompt/trailing row. Regenerate with
`qwen_prompt_fixture.py PINNED_CHECKOUT MODEL_DIR NEW_JSON`; Python is offline only.
The Rust opt-in `icl_prompt_exactly_matches_pinned_upstream_fixture` test checks
those exact bytes against the pinned converted tables. This validates prompt
construction, not actual speech or model logits.

Pocket numerical fixtures record the exact exporter runtime build as well as
version. `pocket-export-verified-ort128.json` also records complete latent and PCM
hashes. The matching shared-runtime Rust test checks all hashes for all six
bundles; production-static versus Python-build mismatches remain explicit in
the report. The generated reference PCM is a deterministic synthetic waveform,
not human speech or an assessment of speaker quality.
