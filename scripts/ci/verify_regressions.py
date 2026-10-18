#!/usr/bin/env python3
"""Verify that every security-regression test still exists exactly once.

The authoritative behavioral check is the main `cargo test --all-targets` run.
libtest, however, exits successfully when a `-- <filter>` matches nothing, so a
renamed or deleted regression test can silently vanish. This script consumes a
single `cargo test --all-targets -- --list` listing and fails when any required
test is missing or ambiguous (matched more than once).

Usage:
    cargo test --locked --all-targets -- --list > listing.txt
    python3 scripts/ci/verify_regressions.py listing.txt
"""
import sys

# Keep this list synchronized with real test function names. Each entry is the
# leaf (unqualified) name; the module path is intentionally not pinned so
# moving a test between modules does not break the guard.
REQUIRED = (
    'cancellation_does_not_release_running_permit',
    'cancellation_with_pending_queue_keeps_running_slot',
    'spoofed_leftmost_entry_cannot_be_selected',
    'multiple_trusted_proxies_skipped_right_to_left',
    'malformed_chain_fails_safe',
    'multiple_xff_headers_do_not_allow_spoofing',
    'enqueue_rejects_beyond_bound_without_dropping',
    'dead_audio_thread_reports_unavailable_on_every_operation',
    'stop_clears_current_and_queued_temps',
    'sweep_deletes_only_owned_uuid_wavs',
    'failed_create_does_not_touch_live_memory',
    'failed_revoke_leaves_token_active_in_memory',
    'malformed_integers_fail_instead_of_defaulting',
    'malformed_booleans_fail_instead_of_defaulting',
    'bounded_values_are_enforced',
    'content_length_mismatch_is_rejected',
    'oversized_stream_is_aborted_and_cleaned',
    'download_timeout_cleans_temporary_file',
    'unknown_models_are_rejected',
    'unknown_voices_are_rejected_not_defaulted',
    'unknown_formats_are_rejected_not_defaulted',
    'malformed_expiry_is_rejected_not_perpetual',
    'hostile_voice_names_are_escaped',
    'volume_accepts_only_finite_unit_range',
)


def parse_listing(text):
    """Count leaf test names in a `cargo test -- --list` listing.

    Only `name: test` and `name: bench` lines are counted; doctests and
    `--list` headers are ignored. The leaf is the final `::`-separated segment.
    """
    counts = {}
    for line in text.splitlines():
        line = line.strip()
        if not (line.endswith(': test') or line.endswith(': bench')):
            continue
        qualified = line.rsplit(':', 1)[0].strip()
        if not qualified:
            continue
        leaf = qualified.split('::')[-1]
        counts[leaf] = counts.get(leaf, 0) + 1
    return counts


def check(text, required=REQUIRED):
    counts = parse_listing(text)
    missing = [name for name in required if counts.get(name, 0) == 0]
    duplicates = [name for name in required if counts.get(name, 0) > 1]
    problems = []
    if missing:
        problems.append('missing regression tests: ' + ', '.join(missing))
    if duplicates:
        duplicates_detail = ', '.join(
            f'{name} (x{counts[name]})' for name in duplicates
        )
        problems.append('ambiguous regression tests matched more than once: ' + duplicates_detail)
    if problems:
        raise ValueError('; '.join(problems))
    return len(required)


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[1], encoding='utf-8') as stream:
        total = check(stream.read())
    print(f'All {total} security regression tests are present exactly once')
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv))
