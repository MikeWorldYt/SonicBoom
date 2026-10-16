# Interrupted exploratory runs

`qwen-baseline/` contains only A/B/C of the initial direct-provider run, using the
older reference preprocessing and the existing English synthetic reference.
It was interrupted before its cold/warm sequence finished. Its three files and
manifest are retained as partial evidence, not a passing isolation suite.
`pocket-grid/` contains an interrupted initial grid and is not the final grid.

Use `qwen-final/`, `pocket-es-final/` and `pocket-grid-final/` for completed runs.
`pocket-es-baseline/` and `pocket-es-comparison/` use the same old synthetic
reference, seed and texts and can be compared as an exploratory before/after
reference-preprocessing case. Their ASR metrics are noisy and do not establish
a perceptual quality improvement. Every manifest records its own executable
identity where exposed; runtime git SHA is not a substitute for build identity.
