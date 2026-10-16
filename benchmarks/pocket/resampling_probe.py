"""Synthetic anti-alias probe; NumPy mirror of Rust linear interpolation.
Run with NumPy/SciPy. Timings compare Python candidates, not Rust throughput.
"""
import json
import math
import time
import numpy as np
import scipy
from scipy.signal import resample_poly


def linear(x, source, target):
    if source == target:
        return x.copy()
    ratio = target / source
    positions = np.arange(math.ceil(len(x) * ratio), dtype=np.float64) / ratio
    indices = positions.astype(np.int64)
    frac = (positions - indices).astype(np.float32)
    a = x[np.minimum(indices, len(x) - 1)]
    b = x[np.minimum(indices + 1, len(x) - 1)]
    return a + (b - a) * frac


for rate in [16000, 22050, 24000, 44100, 48000]:
    # 15 kHz above target Nyquist tests alias rejection on downsampling;
    # 1 kHz checks in-band level on every rate.
    for hz in ([1000, 15000] if rate > 24000 else [1000]):
        x = np.sin(2 * np.pi * hz * np.arange(rate * 3) / rate).astype(np.float32)
        divisor = math.gcd(rate, 24000)
        for name, fn in [('linear_numpy_mirror', lambda: linear(x, rate, 24000)),
                         ('scipy_polyphase_kaiser5', lambda: resample_poly(x, 24000 // divisor, rate // divisor))]:
            start = time.perf_counter()
            y = fn()
            elapsed = time.perf_counter() - start
            middle = y[2400:-2400]  # exclude boundary transients
            gain = float(np.sqrt(np.mean(middle.astype(np.float64) ** 2)) * np.sqrt(2))
            print(json.dumps(dict(source_rate=rate, tone_hz=hz, candidate=name,
                                  amplitude_db=20 * math.log10(max(gain, 1e-15)),
                                  seconds=elapsed, numpy=np.__version__, scipy=scipy.__version__)))
