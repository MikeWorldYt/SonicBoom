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
