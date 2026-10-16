#!/usr/bin/env python3
"""Offline orchestration only: Rust owns all synthesis. Never downloads models."""
import argparse
import json
import os
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument('runner', type=Path)
p.add_argument('models', type=Path)
p.add_argument('authorized_reference', type=Path)
p.add_argument('new_output', type=Path)
a = p.parse_args()
a.new_output.mkdir(exist_ok=False)
texts = {
    'en': ['Hello', 'This is a test of independent speech generation.', 'Hello world. This is the second sentence. Please read every word.'],
    'es': ['hola', 'Esto es una prueba de generación de voz independiente.', 'Hola mundo. Esta es la segunda frase. Lee todas las palabras.'],
    'pt': ['Olá', 'Este é um teste de geração de voz independente.', 'Olá mundo. Esta é a segunda frase. Leia todas as palavras.'],
    'de': ['Hallo', 'Dies ist ein Test unabhängiger Spracherzeugung.', 'Hallo Welt. Dies ist der zweite Satz. Bitte lies jedes Wort.'],
    'it': ['Ciao', 'Questo è un test di generazione vocale indipendente.', 'Ciao mondo. Questa è la seconda frase. Leggi ogni parola.'],
    'fr': ['Bonjour', 'Ceci est un test de génération vocale indépendante.', 'Bonjour le monde. Ceci est la deuxième phrase. Lis chaque mot.'],
}
with (a.new_output / 'runs.jsonl').open('x') as manifest:
    for lang, cases in texts.items():
        model = 'pocket-tts-fr-24l' if lang == 'fr' else f'pocket-tts-{lang}'
        for steps in [1, 2, 4, 8]:
            for temperature in [0.5, 0.7, 0.9]:
                destination = a.new_output / f'{lang}-s{steps}-t{temperature}'
                env = {**os.environ, 'AUDIT_TEXTS_JSON': json.dumps(cases),
                       'AUDIT_POCKET_STEPS': str(steps), 'AUDIT_POCKET_TEMPERATURE': str(temperature)}
                command = [str(a.runner.resolve()), 'pocket', model, lang, str(a.models), str(destination), str(a.authorized_reference)]
                with (a.new_output / f'{destination.name}.log').open('x') as log:
                    result = subprocess.run(command, env=env, stdout=log, stderr=log)
                row = {'model': model, 'language': lang, 'steps': steps, 'temperature': temperature,
                       'exit_code': result.returncode, 'manifest': str(destination / 'manifest.jsonl'),
                       'asr': 'NOT RUN', 'listening': 'NOT RUN'}
                manifest.write(json.dumps(row) + '\n')
                manifest.flush()
                print(json.dumps(row), flush=True)
