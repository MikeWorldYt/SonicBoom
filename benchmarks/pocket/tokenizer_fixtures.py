"""Regenerate direct SentencePiece token fixtures using verified local bundles.
Usage: python tokenizer_fixtures.py MODEL_ROOT > fixtures/languages.json
No downloads; runtime tests verify the pinned manifests separately.
"""
import hashlib
import json
import pathlib
import sys
import sentencepiece as spm
from sentencepiece import sentencepiece_model_pb2 as pb

cases = {
    'portuguese': ('pocket-tts-pt', 'pt', 'Olá mundo, esta é uma prova de síntese de voz em português.'),
    'german': ('pocket-tts-de', 'de', 'Hallo Welt, dies ist ein Test der deutschen Sprachsynthese.'),
    'italian': ('pocket-tts-it', 'it', 'Ciao mondo, questa è una prova di sintesi vocale in italiano.'),
    'french_24l': ('pocket-tts-fr-24l', 'fr', 'Bonjour le monde, ceci est un test de synthèse vocale en français.'),
}
fixtures = []
for bundle, (model, language, text) in cases.items():
    path = pathlib.Path(sys.argv[1]) / 'onnx' / bundle / 'tokenizer.model'
    proto = pb.ModelProto()
    proto.ParseFromString(path.read_bytes())
    assert proto.trainer_spec.model_type == pb.TrainerSpec.UNIGRAM
    assert proto.trainer_spec.byte_fallback
    assert proto.normalizer_spec.name == 'identity'
    assert not proto.normalizer_spec.precompiled_charsmap
    assert proto.normalizer_spec.add_dummy_prefix
    assert proto.normalizer_spec.escape_whitespaces
    assert not proto.normalizer_spec.remove_extra_whitespaces
    processor = spm.SentencePieceProcessor(model_file=str(path))
    texts = [text, 'ÁÉÍÓÚ àâçéêëîïôùûü ß äöü ãõ ação coração.',
             'One; two: three, four! 12345?', '  Two  spaces  ', '😀 漢字 こんにちは',
             'e\u0301 café', 'Line one\nline two\tend', '.!...?', ',;:']
    fixtures.append(dict(model=model, language=language, bundle=bundle,
                         tokenizer_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                         sentencepiece_version=spm.__version__, synthesis_text=text,
                         cases=[dict(text=t, ids=processor.encode(t)) for t in texts]))
print(json.dumps(fixtures, ensure_ascii=False, indent=2))
