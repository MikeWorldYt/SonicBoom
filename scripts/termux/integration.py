"""Native binary integration probe. Downloads only Supertonic into the normal cache.
Run --smoke-only for an offline model-free listener test. Never counts 503 as inference.
"""
import argparse
import concurrent.futures
import contextlib
import json
import hashlib
import datetime
import http.cookiejar
import math
import os
from pathlib import Path
import secrets
import signal
import socket
import struct
import subprocess
import tempfile
import time
import threading
import urllib.error
import urllib.request
import wave


def wav_info(data):
    import io
    with wave.open(io.BytesIO(data)) as wav:
        assert wav.getnchannels() in (1, 2)
        assert 8000 <= wav.getframerate() <= 192000
        assert wav.getsampwidth() == 2
        frames = wav.readframes(wav.getnframes())
        assert frames, 'empty PCM'
        samples = struct.unpack('<' + 'h' * (len(frames) // 2), frames)
        rms = math.sqrt(sum(s * s for s in samples) / len(samples))
        assert all(math.isfinite(s) for s in samples)
        assert rms > 1 and max(map(abs, samples)) > 16, 'silent output'
        return {'duration_seconds': wav.getnframes() / wav.getframerate(),
                'sample_rate': wav.getframerate(), 'channels': wav.getnchannels(), 'rms': rms}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=Path)
    parser.add_argument('--smoke-only', action='store_true')
    parser.add_argument('--offline', action='store_true', help='route download requests to a closed proxy; requires cached models')
    parser.add_argument('--timeout', type=int, default=1800)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--audio-output', type=Path)
    parser.add_argument('--report', type=Path, default=Path('termux-results.json'))
    args = parser.parse_args()
    if not 3 <= args.repetitions <= 1000:
        parser.error("--repetitions must be 3..1000")
    binary = str(args.binary.resolve())
    env = os.environ.copy()
    if args.offline:
        for name in ('HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy'):
            env[name] = 'http://127.0.0.1:9'
        env['NO_PROXY'] = env['no_proxy'] = ''
    with tempfile.TemporaryDirectory(prefix='sonicboom-termux-') as directory:
        env['SONICBOOM_CONFIG'] = str(Path(directory) / 'config.toml')
        password = secrets.token_urlsafe(32)
        env['SONICBOOM_ADMIN_PW'] = password
        env['BIND'] = '127.0.0.1'
        env.pop('SONICBOOM_AUTH_REQUIRED', None)
        # Bind an available loopback port; subprocess binding still detects races.
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        env['PORT'] = str(port)
        subprocess.run([binary, 'init', '--headless'], env=env, check=True)
        # Credential persistence must work without the bootstrap environment.
        del env['SONICBOOM_ADMIN_PW']
        def config_set(key, value):
            subprocess.run([binary, 'config', 'set', key, str(value)], env=env, check=True,
                           stdout=subprocess.DEVNULL)
        config_set('server.port', port)
        cache = Path(os.environ.get('XDG_CACHE_HOME', str(Path.home() / '.cache'))) / 'sonicboom/models/supertonic-3'
        config_set('model.cache_dir', cache)
        config_set('server.auth_mode', 'local')
        # Doctor must report absent configuration without creating repo-relative models.
        absent = Path(directory) / 'absent.toml'
        doctor = subprocess.run([binary, 'doctor'], env=dict(env, SONICBOOM_CONFIG=str(absent)), capture_output=True, check=True)
        assert 'configuration_status' in json.loads(doctor.stdout) and not absent.exists()
        subprocess.run([binary, 'config', 'validate'], env=env, check=True)
        # Missing library must return an actionable failure, not a panic.
        bad = subprocess.run([binary, '--onnx-self-test'], env=dict(env, ORT_DYLIB_PATH=directory + '/missing.so'),
                             capture_output=True)
        assert bad.returncode != 0 and b'panicked' not in bad.stderr
        wrong = Path(directory) / 'wrong.so'
        wrong.write_bytes(b'not an ELF library')
        bad = subprocess.run([binary, '--onnx-self-test'], env=dict(env, ORT_DYLIB_PATH=str(wrong)), capture_output=True)
        assert bad.returncode != 0 and b'panicked' not in bad.stderr
        # An Android DSO without the ORT API must fail without a panic.
        no_api = subprocess.run([binary, '--onnx-self-test'], env=dict(env, ORT_DYLIB_PATH='/system/lib64/libc.so'), capture_output=True)
        assert no_api.returncode != 0 and b'panicked' not in no_api.stderr
        # Actual binary ENOSPC handling, injected only for its isolated storage.
        fault_library = Path(directory) / 'fault-storage.so'
        subprocess.run(['clang', '-shared', '-fPIC', str(Path(__file__).with_name('fault-storage.c')),
                        '-o', str(fault_library), '-ldl'], env=env, check=True)
        full_storage = Path(directory) / 'full-storage'
        full_storage.mkdir()
        fault_env = dict(env, SONICBOOM_CONFIG=str(full_storage / 'config.toml'),
                         SONICBOOM_ADMIN_PW=password, SONICBOOM_TEST_FULL_STORAGE=str(full_storage) + '/',
                         LD_PRELOAD=str(fault_library) + (':' + env['LD_PRELOAD'] if env.get('LD_PRELOAD') else ''))
        no_space = subprocess.run([binary, 'init', '--headless'], env=fault_env, capture_output=True)
        assert no_space.returncode != 0 and b'panicked' not in no_space.stderr
        assert b'No space left on device' in no_space.stderr, no_space.stderr[:500]
        assert not (full_storage / 'config.toml').exists()
        # Invalid configuration fails without panic.
        bad_config = Path(directory) / 'invalid.toml'
        bad_config.write_text('this is not TOML = [')
        invalid = subprocess.run([binary, 'config', 'validate'], env=dict(env, SONICBOOM_CONFIG=str(bad_config)), capture_output=True)
        assert invalid.returncode != 0 and b'panicked' not in invalid.stderr
        # ELF machine mismatch must be caught by the actual dynamic loader.
        native = bytearray(Path(env.get('ORT_DYLIB_PATH', os.environ['PREFIX'] + '/lib/sonicboom/libonnxruntime.so')).read_bytes())
        native[18:20] = struct.pack('<H', 62)  # EM_X86_64, Android target is AArch64
        wrong.write_bytes(native)
        bad = subprocess.run([binary, '--onnx-self-test'], env=dict(env, ORT_DYLIB_PATH=str(wrong)), capture_output=True)
        assert bad.returncode != 0 and b'panicked' not in bad.stderr
        wrong.unlink()
        del native
        started = time.monotonic()
        log = open(Path(directory) / 'server.log', 'w+')
        process = subprocess.Popen([binary] + (['--smoke-test'] if args.smoke_only else []), env=env,
                                   stdout=log, stderr=subprocess.STDOUT)
        stop_sampling = threading.Event()
        samples = []
        def sample_process():
            while not stop_sampling.wait(.25):
                try:
                    status = Path(f'/proc/{process.pid}/status').read_text()
                    memory = {line.split(':')[0]: int(line.split()[1]) for line in status.splitlines()
                              if line.startswith(('VmRSS:', 'VmHWM:', 'Threads:'))}
                    stat = Path(f'/proc/{process.pid}/stat').read_text().rsplit(')', 1)[1].split()
                    samples.append((time.monotonic(), int(stat[11]) + int(stat[12]), memory))
                except (OSError, ValueError):
                    pass
        sampler = threading.Thread(target=sample_process, daemon=True)
        sampler.start()
        base = f'http://127.0.0.1:{port}'
        auth_token = None
        def request(path, data=None, content_type=None, expected=200, method=None):
            headers = {'Content-Type': content_type} if content_type else {}
            if auth_token:
                headers['Authorization'] = 'Bearer ' + auth_token
            req = urllib.request.Request(base + path, data=data, headers=headers, method=method)
            try:
                with urllib.request.urlopen(req, timeout=args.timeout) as response:
                    status, body = response.status, response.read()
            except urllib.error.HTTPError as error:
                status, body = error.code, error.read()
            assert status == expected, (path, status, body[:500])
            return body
        report = {'platform': 'android', 'kernel': os.uname().sysname, 'architecture': os.uname().machine,
                  'android': subprocess.check_output(['getprop', 'ro.build.version.release'], text=True).strip(),
                  'model': 'Supertonic 3', 'threads_intra_op': env.get('SONICBOOM_CPU_THREADS', '2'),
                  'smoke_only': args.smoke_only, 'offline_download_proxy': args.offline,
                  'tested_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  'device_model': subprocess.check_output(['getprop', 'ro.product.model'], text=True).strip(),
                  'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
                  'build_info': json.loads(subprocess.check_output([binary, '--build-info'], env=env))}
        assert report['build_info']['platform'] == 'android'
        assert report['build_info']['architecture'] == 'aarch64'
        assert report['build_info']['mode'] == 'headless'
        assert report['build_info']['playback'] is False and report['build_info']['web_ui'] is False
        try:
            deadline = time.monotonic() + args.timeout
            while True:
                assert process.poll() is None, 'server exited'
                try:
                    request('/health')
                    break
                except (OSError, AssertionError):
                    if time.monotonic() > deadline:
                        raise TimeoutError('listener did not start')
                    time.sleep(.25)
            for endpoint in ('/', '/setup', '/assets/missing.js'):
                request(endpoint, expected=404)
            for endpoint in ('/setup/status', '/api/status', '/api/info', '/api/providers', '/v1/models', '/v1/voices'):
                request(endpoint)
            # A second process on the occupied port must fail, preserving the first.
            occupied = subprocess.run([binary, '--smoke-test'], env=env, capture_output=True, timeout=30)
            assert occupied.returncode != 0 and b'panicked' not in occupied.stderr
            request('/health')
            request('/api/tts?voice=F1&format=wav', b'x' * 10001, 'text/plain', expected=400)
            request('/api/tts?voice=F1&format=wav', b'x' * 70000, 'text/plain', expected=413)
            # Exercise password persistence, JSON admin sessions, CSRF and bearer auth.
            opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
            def admin_request(path, fields=None):
                data = urllib.parse.urlencode(fields).encode() if fields is not None else None
                with opener.open(urllib.request.Request(base + path, data=data), timeout=30) as response:
                    return json.load(response)
            admin = admin_request('/admin/login', {'id': 'admin', 'pw': password})
            token_data = admin_request('/admin/tokens', {'csrf_token': admin['csrf']})
            minted_token = token_data['newToken']
            assert minted_token
            request('/api/config', json.dumps({'changes': {'server': {'auth_mode': 'token'}}}).encode(),
                    'application/json', method='PATCH')
            auth_token = 'incorrect-token'
            request('/api/tts?voice=F1&format=wav', b'Hello', 'text/plain', expected=401)
            auth_token = minted_token
            request('/api/config')
            info = json.loads(request('/api/info'))
            assert b'"playback":true' not in json.dumps(info, separators=(',', ':')).encode()
            if args.smoke_only:
                request('/ready', expected=503)
                report['inference'] = 'not tested'
            else:
                last_state = None
                loading_started = None
                while True:
                    assert process.poll() is None, 'server exited during model load'
                    state = json.loads(request('/api/status'))['status']
                    if state != last_state:
                        print('Model state:', state, flush=True)
                        last_state = state
                        if state == 'loading':
                            loading_started = time.monotonic()
                    if state == 'failed':
                        raise RuntimeError('model load failed')
                    try:
                        request('/ready')
                        break
                    except AssertionError:
                        if time.monotonic() > deadline:
                            raise TimeoutError('model never became ready')
                        time.sleep(1)
                report['model_ready_seconds'] = time.monotonic() - started
                if loading_started is not None:
                    report['observed_model_load_seconds'] = time.monotonic() - loading_started
                ready_at = time.monotonic()
                during_load = [s[2].get('VmRSS', 0) for s in samples if s[0] <= ready_at and (loading_started is None or s[0] >= loading_started)]
                if during_load:
                    report['peak_sampled_model_load_rss_kib'] = max(during_load)
                def cpu_ticks():
                    try:
                        fields = Path(f'/proc/{process.pid}/stat').read_text().rsplit(')', 1)[1].split()
                        return int(fields[11]) + int(fields[12])
                    except (OSError, ValueError):
                        return None
                measurements = []
                for index in range(args.repetitions):
                    ticks = cpu_ticks()
                    start = time.monotonic()
                    audio = request('/api/tts?voice=F1&format=wav', b'Hello from Android Termux', 'text/plain')
                    latency = time.monotonic() - start
                    if index == 0 and args.audio_output:
                        args.audio_output.write_bytes(audio)
                    measurement = wav_info(audio)
                    measurement.update(latency_seconds=latency, real_time_factor=latency / measurement['duration_seconds'])
                    end_ticks = cpu_ticks()
                    if ticks is not None and end_ticks is not None:
                        measurement['cpu_percent_one_core'] = (end_ticks - ticks) / os.sysconf('SC_CLK_TCK') / latency * 100
                    if samples:
                        measurement['sampled_rss_kib'] = samples[-1][2].get('VmRSS')
                    measurements.append(measurement)
                report['synthesis'] = measurements
                rss_values = [m['sampled_rss_kib'] for m in measurements if m.get('sampled_rss_kib') is not None]
                if rss_values:
                    report['synthesis_rss_growth_kib'] = rss_values[-1] - rss_values[0]
                if len(rss_values) >= 10:
                    assert max(rss_values[-5:]) - min(rss_values[-5:]) < 16384, 'fixed-input RSS did not stabilize'
                wav_info(request('/v1/audio/speech', json.dumps({'model': 'tts-1', 'voice': 'F1',
                         'input': 'Hello from Android Termux', 'response_format': 'wav'}).encode(), 'application/json'))
                request('/api/tts?voice=INVALID&format=wav', b'Hello', 'text/plain', expected=400)
                with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                    outputs = list(pool.map(lambda _: request('/api/tts?voice=F1&format=wav', b'Concurrent test', 'text/plain'), range(2)))
                for output in outputs:
                    wav_info(output)
                # Background jobs retain bounded admission and cancellation/deadline controls.
                long_text = ('A bounded native Android inference cancellation test. ' * 30).encode()
                job = json.loads(request('/api/tts/jobs?voice=F1&format=wav', long_text, 'text/plain', expected=202))
                cancelled = json.loads(request('/api/tts/jobs/' + job['id'] + '/cancel', method='POST'))
                assert cancelled['cancellation_requested']
                def await_job(job_id):
                    limit = time.monotonic() + 60
                    while time.monotonic() < limit:
                        status = json.loads(request('/api/tts/jobs/' + job_id))
                        if status['stage'] in ('completed', 'failed', 'cancelled'):
                            return status
                        time.sleep(.1)
                    raise TimeoutError('job did not stop')
                assert await_job(job['id'])['stage'] == 'cancelled'
                request('/api/config', json.dumps({'changes': {'inference': {'generation_deadline_secs': 1}}}).encode(),
                        'application/json', method='PATCH')
                timed = json.loads(request('/api/tts/jobs?voice=F1&format=wav', long_text, 'text/plain', expected=202))
                assert await_job(timed['id'])['deadline_exceeded'], 'long inference did not respect its deadline'
                request('/api/config', json.dumps({'changes': {'inference': {'generation_deadline_secs': 1800}}}).encode(),
                        'application/json', method='PATCH')
                # Disconnect a synchronous client during inference; the process must recover.
                with socket.create_connection(('127.0.0.1', port), timeout=10) as client:
                    headers = (f'POST /api/tts?voice=F1&format=wav HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n'
                               f'Authorization: Bearer {auth_token}\r\nContent-Type: text/plain\r\n'
                               f'Content-Length: {len(long_text)}\r\nConnection: close\r\n\r\n').encode()
                    client.sendall(headers + long_text)
                    time.sleep(.1)
                time.sleep(.25)
                request('/health')
                report['inference'] = 'passed'
            with contextlib.suppress(OSError):
                report['process_status'] = '\n'.join(line for line in Path(f'/proc/{process.pid}/status').read_text().splitlines()
                    if line.startswith(('VmRSS:', 'VmHWM:', 'Threads:')))
            if samples:
                report['peak_sampled_rss_kib'] = max(s[2].get('VmRSS', 0) for s in samples)
                report['peak_sampled_threads'] = max(s[2].get('Threads', 0) for s in samples)
                if len(samples) > 1:
                    wall = samples[-1][0] - samples[0][0]
                    cpu_seconds = (samples[-1][1] - samples[0][1]) / os.sysconf('SC_CLK_TCK')
                    report['cpu_seconds'] = cpu_seconds
                    report['cpu_percent_one_core'] = cpu_seconds / wall * 100
            # SIGTERM exercises Axum listener shutdown on Android.
            if not args.smoke_only:
                process.send_signal(signal.SIGTERM)
            assert process.wait(timeout=30) == 0
            if not args.smoke_only:
                # Same config, native runtime and model cache, no bootstrap password.
                process = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
                limit = time.monotonic() + 60
                while True:
                    assert process.poll() is None, 'restart exited'
                    try:
                        request('/ready')
                        break
                    except (OSError, AssertionError):
                        if time.monotonic() > limit:
                            raise TimeoutError('restart did not become ready')
                        time.sleep(.25)
                request('/api/config')  # previously minted bearer token survives restart
                process.send_signal(signal.SIGINT)
                assert process.wait(timeout=30) == 0
                report['restart_and_sigint'] = 'passed'
            args.report.write_text(json.dumps(report, indent=2) + '\n')
            print(json.dumps(report, indent=2))
        except BaseException:
            log.flush()
            log.seek(0)
            print(log.read()[-12000:])
            raise
        finally:
            stop_sampling.set()
            sampler.join(timeout=2)
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            log.close()


if __name__ == '__main__':
    main()
