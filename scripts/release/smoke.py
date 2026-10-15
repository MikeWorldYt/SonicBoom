#!/usr/bin/env python3
"""Extract and run the shipped archive in an isolated, model-free environment."""
import argparse
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tarfile
import tempfile
import time
import urllib.request
import zipfile


def smoke(archive):
    windows = archive.suffix == '.zip'
    with tempfile.TemporaryDirectory(prefix='sonicboom-smoke-') as temporary:
        root = Path(temporary)
        app = root / 'app'
        app.mkdir()
        if windows:
            with zipfile.ZipFile(archive) as z:
                z.extractall(app)
        else:
            with tarfile.open(archive) as tar:
                tar.extractall(app, filter='data')
        binary = app / ('SonicBoom.exe' if windows else 'SonicBoom')
        assert binary.is_file() and binary.stat().st_size > 0, binary
        assert (app / 'licenses/dependencies.json').is_file()
        env = os.environ.copy()
        # No inherited credentials/configuration/ORT overrides enter the probe.
        for key in list(env):
            if key.startswith(('SONICBOOM_', 'ORT_', 'HF_', 'QWEN', 'GPT_SOVITS')) or key in ('PORT', 'BIND_ADDR'):
                del env[key]
        env['SONICBOOM_CONFIG'] = str(root / 'data/config.toml')
        env['SONICBOOM_ADMIN_PW'] = 'local-smoke-only-password-12345'
        env['LD_LIBRARY_PATH'] = str(app)
        if windows:
            # Deliberately exclude developer-tool DLL directories. The supported
            # system MSVC redistributable must resolve alongside packaged DLLs.
            env['PATH'] = str(app) + os.pathsep + str(Path(env['SystemRoot']) / 'System32')
            import ctypes
            for dll in app.glob('*.dll'):
                ctypes.WinDLL(str(dll))
            # Record PE import tables with the installed MSVC toolchain.
            vswhere = Path(os.environ['ProgramFiles(x86)']) / 'Microsoft Visual Studio/Installer/vswhere.exe'
            install = subprocess.check_output([str(vswhere), '-latest', '-products', '*', '-property', 'installationPath'], text=True).strip()
            tools = sorted(Path(install).glob('VC/Tools/MSVC/*/bin/Hostx64/x64/dumpbin.exe'))
            if not tools:
                raise RuntimeError('MSVC dumpbin required to inspect native DLL dependencies')
            for pe in [binary, *app.glob('*.dll')]:
                subprocess.run([str(tools[-1]), '/DEPENDENTS', str(pe)], check=True)
        else:
            assert os.access(binary, os.X_OK), 'Linux executable permissions lost'
            for native in [binary, *app.glob('*.so*')]:
                result = subprocess.run(['ldd', str(native)], env=env, capture_output=True, text=True, check=True)
                print(result.stdout)
                if 'not found' in result.stdout:
                    raise RuntimeError(f'Unresolved shared library in {native}')
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        env['PORT'] = str(port)
        # The default bootstrap bind is loopback, and the application additionally
        # enforces loopback in --smoke-test mode.
        log = root / 'startup.log'
        with log.open('w') as output:
            process = subprocess.Popen([str(binary), '--smoke-test'], cwd=root, env=env,
                                       stdout=output, stderr=subprocess.STDOUT)
            try:
                deadline = time.monotonic() + 12
                healthy = False
                opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
                while time.monotonic() < deadline and process.poll() is None:
                    try:
                        with opener.open(f'http://127.0.0.1:{port}/health', timeout=1) as response:
                            payload = json.load(response)
                            assert response.status == 200 and isinstance(payload, dict)
                            healthy = True
                            break
                    except (OSError, ValueError):
                        time.sleep(0.2)
                if not healthy:
                    raise RuntimeError('Packaged server did not answer /health within 12 seconds')
                # Diagnostic mode shuts down through the normal server.stop path.
                code = process.wait(timeout=25)
                if code != 0:
                    raise RuntimeError(f'Packaged server exited with {code}')
                if 'Server shut down gracefully.' not in log.read_text():
                    raise RuntimeError('Missing graceful shutdown confirmation')
                if list((root / 'data/models').glob('**/*')):
                    raise RuntimeError('Model-free probe unexpectedly created model files')
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                print(log.read_text())
        print(f'PASS: {archive.name}: native libraries, HTTP health, clean shutdown (no models)')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    smoke(parser.parse_args().archive.resolve())
