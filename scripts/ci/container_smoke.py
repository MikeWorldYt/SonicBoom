#!/usr/bin/env python3
"""Model-free container probe: real HTTP health and normal bounded shutdown.

The runtime images set `BIND=0.0.0.0` for production networking, but the
application deliberately refuses `--smoke-test` on anything but a loopback
bind. This probe therefore overrides `BIND=127.0.0.1` for the smoke container
only; production images are left untouched.
"""
import argparse
import subprocess
import sys
import time
import uuid

HEALTH_URL = 'http://127.0.0.1:17842/health'


def _docker(*args, **kwargs):
    return subprocess.run(['docker', *args], **kwargs)


def _logs(name):
    result = _docker('logs', name, capture_output=True, text=True)
    return result.stdout + result.stderr


def _container_state(name):
    """Return the container's `Status` (e.g. `running`, `exited: 0`)."""
    result = _docker(
        'inspect', '--format', '{{.State.Status}} (exit {{.State.ExitCode}})',
        name, capture_output=True, text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else ''


def _is_running(name):
    result = _docker('inspect', '--format', '{{.State.Running}}', name,
                     capture_output=True, text=True)
    return result.returncode == 0 and result.stdout.strip() == 'true'


def _health_status(name):
    """HTTP status the container reports, or None if it cannot be reached."""
    result = _docker(
        'exec', name, 'curl', '-sS', '-o', '/dev/null', '-w', '%{http_code}',
        HEALTH_URL, capture_output=True, text=True,
    )
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def smoke(image, health_timeout=20.0, shutdown_timeout=30.0):
    name = 'sonicboom-smoke-' + uuid.uuid4().hex[:12]
    try:
        docker_args = [
            'run', '-d',
            '--name', name,
            '-e', 'BIND=127.0.0.1',
            '-e', 'SONICBOOM_ADMIN_PW=ci-local-testing-password-123',
            '--entrypoint', '/app/sonicboom',
            image,
            '--smoke-test',
        ]
        started = _docker(*docker_args, capture_output=True, text=True)
        if started.returncode != 0:
            raise RuntimeError(
                f'Failed to start smoke container for {image}: '
                f'{started.stderr.strip()}'
            )

        deadline = time.monotonic() + health_timeout
        healthy = False
        while time.monotonic() < deadline:
            if not _is_running(name):
                raise RuntimeError(
                    f'Container exited before becoming healthy: '
                    f'{_container_state(name)}\n{_logs(name)}'
                )
            if _health_status(name) == '200':
                healthy = True
                break
            time.sleep(0.25)
        if not healthy:
            raise RuntimeError(
                f'Container did not answer HTTP 200 on /health within '
                f'{health_timeout:.0f}s\n{_logs(name)}'
            )

        try:
            result = _docker('wait', name, capture_output=True, text=True,
                             timeout=shutdown_timeout)
        except subprocess.TimeoutExpired:
            raise RuntimeError(
                f'Container did not shut down within {shutdown_timeout:.0f}s\n'
                f'{_logs(name)}'
            )
        if result.returncode != 0 or result.stdout.strip() != '0':
            raise RuntimeError(
                f'Container failed graceful shutdown: {result.stdout.strip()!r} '
                f'({_container_state(name)})\n{_logs(name)}'
            )
        print(f'PASS: {image}: HTTP 200 health and graceful shutdown without '
              f'model downloads')
    finally:
        print(_logs(name))
        _docker('rm', '-f', name, capture_output=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image')
    parser.add_argument('--health-timeout', type=float, default=20.0)
    parser.add_argument('--shutdown-timeout', type=float, default=30.0)
    options = parser.parse_args()
    try:
        smoke(options.image, options.health_timeout, options.shutdown_timeout)
    except RuntimeError as error:
        print(f'FAIL: {error}', file=sys.stderr)
        raise SystemExit(1)
