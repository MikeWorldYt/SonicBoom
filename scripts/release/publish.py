#!/usr/bin/env python3
"""Refuse replacement and verify bytes downloaded through the GitHub API."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

from package import ASSETS, checksums, digest, validate_build_info
import tarfile
import zipfile


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


def preflight(repo, tag):
    result = subprocess.run(['gh', 'api', f'repos/{repo}/releases/tags/{tag}'], capture_output=True, text=True)
    if result.returncode != 0:
        if 'HTTP 404' not in result.stderr:
            raise RuntimeError(f'Cannot establish release absence: {result.stderr}')
        return
    try:
        release = json.loads(result.stdout)
    except ValueError as error:
        raise RuntimeError(f'Cannot parse existing release for {repo}@{tag}: {error}') from error
    if release.get('assets') or (release.get('body') or '').strip():
        raise RuntimeError(f'Release {repo}@{tag} already exists; refusing duplicate/replacement upload')
    release_id = release.get('id')
    if release_id is None:
        raise RuntimeError(f'Cannot identify empty release for {repo}@{tag}')
    gh('api', '-X', 'DELETE', f'repos/{repo}/releases/{release_id}')
    print(f'Removed empty placeholder release {repo}@{tag}; staging a verified draft')


def find_release(repo, tag):
    # Get-by-tag-name only resolves published releases; drafts must come from
    # the paginated list so the staged draft can be verified before publication.
    pages = json.loads(gh('api', '--paginate', '--slurp', f'repos/{repo}/releases?per_page=100'))
    matches = [release for page in pages for release in page if release.get('tag_name') == tag]
    with_assets = [release for release in matches if release.get('assets')]
    candidates = with_assets or matches
    if not candidates:
        raise RuntimeError(f'No published or draft release exists for {repo}@{tag}')
    if len(candidates) != 1:
        raise RuntimeError(f'Ambiguous releases for {repo}@{tag}: {len(candidates)} candidates')
    return candidates[0]


def verify(repo, tag, directory):
    release = find_release(repo, tag)
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
        for name in ASSETS:
            archive = downloaded / name
            if name.endswith('.zip'):
                with zipfile.ZipFile(archive) as z:
                    info = json.loads(z.read('BUILD-INFO.json'))
            else:
                with tarfile.open(archive) as tar:
                    info = json.load(tar.extractfile('BUILD-INFO.json'))
            validate_build_info(info)
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
