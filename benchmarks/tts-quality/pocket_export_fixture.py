#!/usr/bin/env python3
"""Call the pinned exporter on synthetic PCM, temperature zero, all six bundles.
Usage: python pocket_export_fixture.py PINNED_RUNTIME MODEL_ROOT NEW_JSON
No downloads and no production Python dependency. Export runtime is CC-BY-4.0;
source: KevinAHM/pocket-tts-onnx, revision 58a6d00cf13d239b6748cb0769f35c580a8f606c.
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import numpy as np
import onnxruntime as ort
runtime, models, destination = map(Path, sys.argv[1:])
assert hashlib.sha256(runtime.read_bytes()).hexdigest() == '4381a4396ba08b2626a25a87001e3c51dbacd136e1022d2d40a8cefb14b44be0'
spec = importlib.util.spec_from_file_location('pinned_pocket_export', runtime)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
records = []
for model_id, bundle, language, target in [
    ('pocket-tts-en', 'english_2026-04', 'en', 'Hello'),
    ('pocket-tts-es', 'spanish', 'es', 'hola'),
    ('pocket-tts-pt', 'portuguese', 'pt', 'Olá'),
    ('pocket-tts-de', 'german', 'de', 'Hallo'),
    ('pocket-tts-it', 'italian', 'it', 'Ciao'),
    ('pocket-tts-fr-24l', 'french_24l', 'fr', 'Bonjour'),
]:
    model = module.PocketTTSOnnx(str(models / 'onnx'), bundle, device='cpu', temperature=0.0, lsd_steps=1)
    pcm = np.array([((i * 17 % 4096) - 2048) / 16384 for i in range(24000)], dtype=np.float32)
    voice = model.mimi_encoder.run(None, {'audio': pcm.reshape(1, 1, -1)})[0]
    latents = model.generate_latents(target, voice)
    audio = model.decode_latents(latents)
    cases = []
    for text in [target, '¿Hola? sí, no.\n¿Qué tal?', ' é  á  ü ', ('Hello world, this is a longer sentence with several words. ' * 12).strip()]:
        prepared, guess = model._prepare_text_prompt(text)
        cases.append({'text': text, 'prepared': prepared, 'frames_guess': guess,
                      'ids': model._tokenize(text).reshape(-1).tolist(), 'chunks': model._split_into_best_sentences(text)})
    indices = sorted(set([0, len(audio) - 1] + list(range(0, len(audio), max(1, len(audio) // 32)))))
    records.append({'model': model_id, 'language': language, 'target': target, 'cases': cases,
                    'voice_frames': voice.shape[-2], 'latent_frames': latents.shape[1],
                    'latents': latents.reshape(-1).tolist(), 'sample_count': len(audio),
                    'pcm_samples': [[i, float(audio[i])] for i in indices],
                    'pcm_sha256': hashlib.sha256(audio.astype('<f4').tobytes()).hexdigest(),
                    'latents_sha256': hashlib.sha256(latents.astype('<f4').tobytes()).hexdigest()})
    print(model_id, latents.shape, len(audio), flush=True)
    del model
with destination.open('x') as output:
    json.dump({'export_revision': '58a6d00cf13d239b6748cb0769f35c580a8f606c',
               'ort_version': ort.__version__, 'ort_build': ort.get_build_info(), 'temperature': 0.0, 'lsd_steps': 1, 'cases': records}, output, ensure_ascii=False, indent=2)
    output.write('\n')
