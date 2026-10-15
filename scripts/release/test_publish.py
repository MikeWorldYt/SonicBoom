"""Publishing must fail closed on ambiguous authorization and asset failures."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import package
import publish


class PublishingTests(unittest.TestCase):
    def test_preflight_refuses_existing_release_and_authentication_errors(self):
        for error in ['HTTP 403: Forbidden', 'network unavailable']:
            with patch.object(publish.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, '', error)):
                with self.assertRaises(RuntimeError):
                    publish.preflight('owner/repo', 'v0.0.5')
        with patch.object(publish.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, '', 'HTTP 404: Not Found')):
            with patch.object(publish, 'gh') as gh:
                publish.preflight('owner/repo', 'v0.0.5')
                gh.assert_not_called()
        with patch.object(publish.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, '', '')):
            with self.assertRaises(RuntimeError):
                publish.preflight('owner/repo', 'v0.0.5')

    def test_preflight_refuses_releases_that_carry_assets_or_notes(self):
        occupied = [{'id': 7, 'assets': [{'name': package.ASSETS[0], 'size': 1}], 'body': ''},
                    {'id': 7, 'assets': [], 'body': 'operator release notes'}]
        for release in occupied:
            with patch.object(publish.subprocess, 'run',
                              return_value=subprocess.CompletedProcess([], 0, json.dumps(release), '')):
                with patch.object(publish, 'gh') as gh:
                    with self.assertRaises(RuntimeError):
                        publish.preflight('owner/repo', 'v0.0.5')
                    gh.assert_not_called()

    def test_preflight_removes_only_an_empty_placeholder_release(self):
        placeholder = json.dumps({'id': 42, 'assets': [], 'body': ' \n'})
        with patch.object(publish.subprocess, 'run',
                          return_value=subprocess.CompletedProcess([], 0, placeholder, '')):
            with patch.object(publish, 'gh') as gh:
                publish.preflight('owner/repo', 'v0.0.5')
                gh.assert_called_once_with('api', '-X', 'DELETE', 'repos/owner/repo/releases/42')

    def test_release_lookup_resolves_drafts_and_rejects_missing_or_ambiguous(self):
        empty = {'tag_name': 'v0.0.5', 'draft': False, 'assets': []}
        draft = {'tag_name': 'v0.0.5', 'draft': True, 'assets': [{'name': package.ASSETS[0], 'size': 1}]}
        other = {'tag_name': 'v0.0.5', 'draft': True, 'assets': [{'name': package.ASSETS[1], 'size': 1}]}
        for pages, expected in [([[draft]], draft), ([[empty, draft]], draft), ([[empty]], empty)]:
            with patch.object(publish, 'gh', return_value=json.dumps(pages)):
                self.assertEqual(publish.find_release('owner/repo', 'v0.0.5'), expected)
        for pages in [[], [[]], [[draft, other]]]:
            with patch.object(publish, 'gh', return_value=json.dumps(pages)):
                with self.assertRaises(RuntimeError):
                    publish.find_release('owner/repo', 'v0.0.5')

    def test_verification_rejects_missing_duplicate_and_empty_assets(self):
        good = [{'name': name, 'size': 1} for name in [*package.ASSETS, 'SHA256SUMS']]
        for assets in [good[:-1], [*good, good[0]], [{**a, 'size': 0} for a in good], []]:
            release = json.dumps([[{'tag_name': 'v0.0.5', 'draft': True, 'assets': assets}]])
            with patch.object(publish, 'gh', return_value=release):
                with self.assertRaises(RuntimeError):
                    publish.verify('owner/repo', 'v0.0.5', Path('unused'))

    def test_downloaded_bytes_must_match_locally_tested_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            local = Path(temp)
            for name in package.ASSETS:
                (local / name).write_bytes(b'tested archive bytes')
            package.checksums(local)
            tamper = False
            def gh(*args):
                if args[0] == 'api':
                    return json.dumps([[{'tag_name': 'v0.0.5', 'draft': True,
                                         'assets': [{'name': name, 'size': (local / name).stat().st_size}
                                                    for name in [*package.ASSETS, 'SHA256SUMS']]}]])
                downloaded = Path(args[-1])
                for source in local.iterdir():
                    shutil.copyfile(source, downloaded / source.name)
                if tamper:
                    # Even a self-consistent replacement must fail comparison
                    # against the original tested artifacts.
                    (downloaded / package.ASSETS[0]).write_bytes(b'replacement bytes')
                    package.checksums(downloaded)
                return ''
            with patch.object(publish, 'gh', side_effect=gh):
                publish.verify('owner/repo', 'v0.0.5', local)
                tamper = True
                with self.assertRaises(RuntimeError):
                    publish.verify('owner/repo', 'v0.0.5', local)


if __name__ == '__main__':
    unittest.main()
