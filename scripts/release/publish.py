#!/usr/bin/env python3
"""Refuse replacement and verify bytes downloaded through the GitHub API."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

from package import ASSETS, checksums, digest


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


def preflight(repo, tag):
    result = subprocess.run(['gh', 'api', f'repos/{repo}/releases/tags/{tag}'], capture_output=True, text=True)
    if result.returncode == 0:
        raise RuntimeError(f'Release {repo}@{tag} already exists; refusing duplicate/replacement upload')
    if 'HTTP 404' not in result.stderr:
        raise RuntimeError(f'Cannot establish release absence: {result.stderr}')


def verify(repo, tag, directory):
    release = json.loads(gh('api', f'repos/{repo}/releases/tags/{tag}'))
    expected = {*ASSETS, 'SHA256SUMS'}
    assets = release['assets']
    if len(assets) != 3 or {a['name'] for a in assets} != expected or any(a['size'] <= 0 for a in assets):
        raise RuntimeError('Release must contain exactly both nonempty archives and SHA256SUMS')
    with tempfile.TemporaryDirectory() as temp:
        gh('release', 'download', tag, '--repo', repo, '--dir', temp)
        downloaded = Path(temp)
        checksums(downloaded, verify=True)
        checksums(directory, verify=True)
        for name in expected:
            if digest(downloaded / name) != digest(directory / name):
                raise RuntimeError(f'GitHub download differs from tested artifact: {name}')
    print(f'Verified GitHub API assets and downloaded checksums for {repo}@{tag}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['preflight', 'verify'])
    parser.add_argument('--directory', type=Path, default=Path('artifacts'))
    args = parser.parse_args()
    repo, tag = os.environ['GITHUB_REPOSITORY'], os.environ['GITHUB_REF_NAME']
    if args.operation == 'preflight':
        preflight(repo, tag)
    else:
        verify(repo, tag, args.directory)
