Source: crates.io ksni 0.3.6, checksum
814b44c24cd2cb236c3b8a41c7f08237b452a8e76ecaa81f1cec40b5b678215b.
License: UNLICENSE (included).

Temporary patch: use zbus 6, which absorbed zvariant/zbus_names and their derives.
The standalone crates are unmaintained (RUSTSEC-2026-0370/0371/0372).
The application still uses the same ksni blocking tray API over Tokio.
Changes: Cargo dependency/features, wire type imports, nested variant matching,
and the now-infallible session connection builder. No tray behavior is removed.
Remove the patch when upstream ksni releases zbus 6 support.
