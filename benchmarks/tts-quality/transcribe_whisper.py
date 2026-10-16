#!/usr/bin/env python3
"""Optional offline validation only. Install faster-whisper==1.2.1 in a dev venv.
The caller supplies a locally downloaded pinned Systran/faster-whisper-tiny model.
No automatic downloads, no VAD trimming, no cross-recording transcription context.
Only use on authorized audit artifacts. Hypotheses are retained locally in JSONL.
"""
import argparse
import hashlib
import json
from pathlib import Path
from faster_whisper import WhisperModel

MODEL = 'Systran/faster-whisper-tiny'
REVISION = 'd90ca5fe260221311c53c58e660288d3deb8d356'
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('local_model', type=Path)
p.add_argument('audit_root', type=Path)
p.add_argument('new_output', type=Path)
a = p.parse_args()
# Record actual binary identity as well as claimed revision.
weights_hash = hashlib.sha256((a.local_model / 'model.bin').read_bytes()).hexdigest()
model = WhisperModel(str(a.local_model), device='cpu', compute_type='int8', cpu_threads=2)
with a.new_output.open('x') as output:
    for manifest in sorted(a.audit_root.rglob('manifest.jsonl')):
        for line in manifest.read_text().splitlines():
            record = json.loads(line)
            if record['provider'] == 'mock':
                continue
            path = manifest.parent / record['output']
            if hashlib.sha256(path.read_bytes()).hexdigest() != record['wav_hash']:
                raise ValueError(f'WAV identity mismatch: {path}')
            segments, info = model.transcribe(str(path), language=record['language'],
                beam_size=5, condition_on_previous_text=False, vad_filter=False)
            transcript = ' '.join(segment.text.strip() for segment in segments).strip()
            row = {'request_id': record['request_id'], 'wav_hash': record['wav_hash'],
                   'transcript': transcript, 'asr_model': MODEL, 'asr_revision': REVISION,
                   'asr_weights_sha256': weights_hash, 'asr_runtime': 'faster-whisper==1.2.1',
                   'language': record['language'], 'manually_reviewed': False}
            output.write(json.dumps(row, ensure_ascii=False) + '\n')
            output.flush()
            print(f'{path}: transcribed', flush=True)
