# ort 2.0.0-rc.13 Android environment cleanup

Vendored from the crates.io `ort` 2.0.0-rc.13 source (MIT / Apache-2.0).
Only `src/environment.rs` is patched; other source files remain upstream.

On Android with the official ONNX Runtime 1.24.3 Android AAR, the original
executable `.fini_array` callback releases `G_ENV` after the dynamically loaded
runtime's C++ destructors. Native inference completes, then Bionic aborts with
`FORTIFY: pthread_mutex_lock called on a destroyed mutex` at normal process exit.

Android now uses the existing Apple approach: register `__cxa_atexit` immediately
after constructing the environment, then release `G_ENV` before the previously
registered runtime C++ destructors. Exclude Android from the `.fini_array` callback
and give the callback the argument required by `__cxa_atexit`. Windows, Linux and
Apple behavior is unchanged. Do not replace clean destruction with `_exit`.

Regression coverage: the offline ONNX test creates/runs/drops sessions twice;
`scripts/termux/test.sh` requires a successful process exit and successful SIGTERM
shutdown of the real server. The built-in `--onnx-self-test` must exit zero, not
merely print that inference passed before aborting.
