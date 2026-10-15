#!/usr/bin/env python3
"""Development-only ONNX artifact audit; never imported by SonicBoom.

Usage: python audit_qwen_onnx.py MODEL_DIR
Requires the `onnx` package in an isolated audit environment. Does not download
weights, execute inference, or establish compatibility with ONNX Runtime.
"""
import argparse
import hashlib
import json
from pathlib import Path

import onnx


def inspect(path, expected):
    with path.open('rb') as source:
        digest = hashlib.file_digest(source, 'sha256').hexdigest()
    result = {'path': expected['path'], 'sha256': digest,
              'integrity_ok': digest == expected['sha256'] and path.stat().st_size == expected['size']}
    try:
        model = onnx.load(path, load_external_data=False)
        result['opsets'] = {op.domain: op.version for op in model.opset_import}
        for direction in ('input', 'output'):
            result[direction] = [
                {'name': value.name, 'type': onnx.helper.printable_type(value.type)}
                for value in getattr(model.graph, direction)]
        result['external_data'] = sorted({entry.value
            for tensor in model.graph.initializer for entry in tensor.external_data
            if entry.key == 'location'})
        onnx.checker.check_model(path)
        result['checker_ok'] = True
    except Exception as error:
        result['checker_ok'] = False
        result['error'] = str(error)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('model_dir', type=Path)
    args = parser.parse_args()
    inventory = json.loads((Path(__file__).resolve().parents[1] /
        'docs/models/qwen-gguf-onnx.inventory.json').read_text())
    results = []
    for file in inventory['files']:
        if not file['path'].endswith('.onnx'):
            continue
        path = args.model_dir / file['path']
        if path.is_file():
            results.append(inspect(path, file))
    print(json.dumps({'revision': inventory['revision'], 'results': results}, indent=2))
    return 0 if results and all(r['integrity_ok'] and r['checker_ok'] for r in results) else 1


if __name__ == '__main__':
    raise SystemExit(main())
