#!/usr/bin/env python3
"""Model-free container probe: real HTTP health and normal bounded shutdown."""
import argparse
import subprocess
import time
import uuid


def smoke(image):
    name = 'sonicboom-smoke-' + uuid.uuid4().hex[:12]
    try:
        subprocess.run(['docker', 'run', '-d', '--name', name,
                        '-e', 'SONICBOOM_ADMIN_PW=ci-local-testing-password-123',
                        '--entrypoint', '/app/sonicboom', image, '--smoke-test'], check=True)
        healthy = False
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            result = subprocess.run(['docker', 'exec', name, 'curl', '-fsS',
                                     'http://127.0.0.1:17842/health'], capture_output=True)
            if result.returncode == 0 and result.stdout == b'OK':
                healthy = True
                break
            time.sleep(0.2)
        if not healthy:
            raise RuntimeError('Container did not answer health check')
        result = subprocess.run(['docker', 'wait', name], check=True, capture_output=True,
                                text=True, timeout=30)
        if result.stdout.strip() != '0':
            raise RuntimeError(f'Container failed graceful shutdown: {result.stdout}')
        print(f'PASS: {image}: HTTP health and graceful shutdown without model downloads')
    finally:
        subprocess.run(['docker', 'logs', name], check=False)
        subprocess.run(['docker', 'rm', '-f', name], check=False)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image')
    smoke(parser.parse_args().image)
