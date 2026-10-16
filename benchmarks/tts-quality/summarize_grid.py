#!/usr/bin/env python3
"""Offline aggregation of explicitly pinned ASR hypotheses, not a quality gate."""
import csv
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
from score_asr import normalize, edits

root, targets_path, asr_path, destination = map(Path, sys.argv[1:])
targets = json.loads(targets_path.read_text())
asr = {r['request_id']: r for r in map(json.loads, asr_path.read_text().splitlines())}
with destination.open('x', newline='') as output:
    fields = ['language', 'steps', 'temperature', 'clips', 'wer', 'cer', 'insertions', 'omissions',
              'mean_rtf', 'mean_duration_secs', 'max_peak', 'max_clipping_fraction', 'manual_review']
    writer = csv.DictWriter(output, fieldnames=fields)
    writer.writeheader()
    for path in sorted(root.glob('*/manifest.jsonl')):
        rows = list(map(json.loads, path.read_text().splitlines()))
        we = ce = wc = cc = inserted = omitted = 0
        for row in rows:
            expected = normalize(targets[row['text_hash']])
            actual = normalize(asr[row['request_id']]['transcript'])
            word = edits(expected.split(), actual.split())
            char = edits(list(expected), list(actual))
            we += word[0]; ce += char[0]; wc += len(expected.split()); cc += len(expected)
            inserted += word[3]; omitted += word[2]
        writer.writerow({'language': rows[0]['language'], 'steps': rows[0]['pocket_steps'],
            'temperature': rows[0]['pocket_temperature'], 'clips': len(rows),
            'wer': we / max(1, wc), 'cer': ce / max(1, cc), 'insertions': inserted, 'omissions': omitted,
            'mean_rtf': sum(r['rtf'] for r in rows) / len(rows),
            'mean_duration_secs': sum(r['duration_secs'] for r in rows) / len(rows),
            'max_peak': max(r['peak'] for r in rows),
            'max_clipping_fraction': max(r['clipping_fraction'] for r in rows), 'manual_review': 'NOT RUN'})
