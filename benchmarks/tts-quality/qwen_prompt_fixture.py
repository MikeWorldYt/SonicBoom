#!/usr/bin/env python3
"""Offline fixture from the exact pinned upstream PromptBuilder, not a reimplementation.
Usage: python qwen_prompt_fixture.py PINNED_UPSTREAM_CHECKOUT MODEL_DIR NEW_JSON
Requires numpy and tokenizers, no inference/Python production dependency.
"""
import hashlib
import importlib
import json
from pathlib import Path
import subprocess
import sys
import types
import numpy as np
from tokenizers import Tokenizer
root, assets_dir, destination = map(Path, sys.argv[1:])
revision = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
assert revision == '74feb581bc8cefb835fc608107e857036d7580a1'
assert not subprocess.check_output(['git', '-C', str(root), 'status', '--porcelain'])
# Load only the upstream prompt module, avoiding its application initializer.
for name, path in [('qwen3_tts_gguf', root / 'qwen3_tts_gguf'),
                   ('qwen3_tts_gguf.inference', root / 'qwen3_tts_gguf/inference')]:
    package = types.ModuleType(name)
    package.__path__ = [str(path)]
    sys.modules[name] = package
# Logging is irrelevant to tensors; the upstream logger is supplied by its app.
sys.modules['qwen3_tts_gguf.inference'].logger = types.SimpleNamespace(info=lambda *a, **k: None)
PromptBuilder = importlib.import_module('qwen3_tts_gguf.inference.prompt_builder').PromptBuilder
text = np.load(assets_dir / 'embeddings/text_embedding_projected.npy', mmap_mode='r')
tables = [np.load(assets_dir / f'embeddings/codec_embedding_{i}.npy', mmap_mode='r') for i in range(16)]
assets = types.SimpleNamespace(text_table=text, emb_tables=tables, tts_pad=text[151671])
tokenizer = Tokenizer.from_file(str(assets_dir / 'tokenizer.json'))
builder = PromptBuilder(tokenizer, assets)
records = []
for count in [1, 32]:
    codes = np.array([[(t * 17 + q * 13) % 2048 for q in range(16)] for t in range(count)], dtype=np.int64)
    speaker = np.array([(i % 17 - 8) / 128 for i in range(2048)], dtype=np.float32)
    voice = types.SimpleNamespace(spk_emb=speaker, text='Hola. ', codes=codes)
    prompt = builder.build_clone_prompt('test', voice, lang_id=2054)
    trailing = prompt.trailing_text_embd
    raw = prompt.embd.astype('<f4').tobytes()
    records.append({'reference_frames': count,
        'combined_ids': tokenizer.encode('Hola. test', add_special_tokens=False).ids,
        'prompt_rows': prompt.embd.shape[1], 'prompt_sha256': hashlib.sha256(raw).hexdigest(),
        'trailing_rows': 0 if trailing is None else trailing.shape[1],
        'trailing_sha256': hashlib.sha256(b'' if trailing is None else trailing.astype('<f4').tobytes()).hexdigest()})
with destination.open('x') as output:
    json.dump({'upstream_revision': revision, 'cases': records}, output, indent=2)
    output.write('\n')
