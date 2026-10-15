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
        for code, error in [(0, ''), (1, 'HTTP 403: Forbidden'), (1, 'network unavailable')]:
            with patch.object(publish.subprocess, 'run', return_value=subprocess.CompletedProcess([], code, '', error)):
                with self.assertRaises(RuntimeError):
                    publish.preflight('owner/repo', 'v0.0.5')
        with patch.object(publish.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, '', 'HTTP 404: Not Found')):
            publish.preflight('owner/repo', 'v0.0.5')

    def test_verification_rejects_missing_duplicate_and_empty_assets(self):
        good = [{'name': name, 'size': 1} for name in [*package.ASSETS, 'SHA256SUMS']]
        for assets in [good[:-1], [*good, good[0]], [{**a, 'size': 0} for a in good]]:
            with patch.object(publish, 'gh', return_value=json.dumps({'assets': assets})):
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
                    return json.dumps({'assets': [{'name': name, 'size': (local / name).stat().st_size}
                                                 for name in [*package.ASSETS, 'SHA256SUMS']]})
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
