#!/usr/bin/env python3
"""Offline only. Join explicitly supplied ASR hypotheses to hashed audit requests.

ASR JSONL: request_id, transcript, asr_model, asr_revision, manually_reviewed.
Targets JSON: {sha256_of_exact_target: exact_target}. No cloud calls/downloads.
Scores are diagnostics, never automatic proof of TTS quality or speaker identity.
"""
import argparse
import hashlib
import json
import unicodedata


def normalize(text):
    text = unicodedata.normalize('NFC', text).casefold()
    return ' '.join(''.join(c if c.isalnum() or unicodedata.category(c).startswith('M') else ' '
                            for c in text).split())


def edits(reference, hypothesis):
    # (cost, substitutions, deletions, insertions), deterministic tie-breaking.
    previous = [(i, 0, 0, i) for i in range(len(hypothesis) + 1)]
    for i, token in enumerate(reference, 1):
        current = [(i, 0, i, 0)]
        for j, predicted in enumerate(hypothesis, 1):
            diagonal = previous[j - 1]
            if token == predicted:
                current.append(diagonal)
                continue
            candidates = [
                (diagonal[0] + 1, diagonal[1] + 1, diagonal[2], diagonal[3]),
                (previous[j][0] + 1, previous[j][1], previous[j][2] + 1, previous[j][3]),
                (current[-1][0] + 1, current[-1][1], current[-1][2], current[-1][3] + 1),
            ]
            current.append(min(candidates, key=lambda item: item[0]))
        previous = current
    return previous[-1]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('manifest')
    p.add_argument('targets')
    p.add_argument('asr_jsonl')
    p.add_argument('new_output')
    args = p.parse_args()
    with open(args.targets) as f:
        targets = json.load(f)
    for digest, target in targets.items():
        if hashlib.sha256(target.encode()).hexdigest() != digest:
            raise ValueError('target hash mismatch')
    with open(args.asr_jsonl) as f:
        hypotheses = {}
        for line in f:
            record = json.loads(line)
            if record['request_id'] in hypotheses:
                raise ValueError('duplicate ASR request identity')
            if not record.get('asr_model') or not record.get('asr_revision'):
                raise ValueError('ASR model and immutable revision required')
            hypotheses[record['request_id']] = record
    with open(args.manifest) as source, open(args.new_output, 'x') as output:
        for line in source:
            request = json.loads(line)
            hypothesis = hypotheses.get(request['request_id'])
            if hypothesis is None:
                output.write(json.dumps({'request_id': request['request_id'], 'asr': 'NOT RUN'}) + '\n')
                continue
            expected = normalize(targets[request['text_hash']])
            actual = normalize(hypothesis['transcript'])
            words = edits(expected.split(), actual.split())
            chars = edits(list(expected), list(actual))
            row = {'request_id': request['request_id'], 'wav_hash': request['wav_hash'],
                   'asr_model': hypothesis['asr_model'], 'asr_revision': hypothesis['asr_revision'],
                   'wer': words[0] / max(1, len(expected.split())),
                   'cer': chars[0] / max(1, len(expected)),
                   'substituted_words': words[1], 'omitted_words': words[2], 'inserted_words': words[3],
                   'hola_test_for_test': expected == 'test' and actual == 'hola test',
                   'manual_review_required': words[0] > 0 and not hypothesis.get('manually_reviewed', False)}
            output.write(json.dumps(row) + '\n')


if __name__ == '__main__':
    main()
