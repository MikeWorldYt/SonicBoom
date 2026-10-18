"""Regression checks for the security-regression manifest guard."""
import unittest

from verify_regressions import REQUIRED, check, parse_listing


class ParseListingTests(unittest.TestCase):
    def test_counts_leaf_names_ignoring_module_path(self):
        text = (
            'Running unittests src/lib.rs\n'
            'api::queue::tests::cancellation_does_not_release_running_permit: test\n'
            'api::gate::tests::enqueue_rejects_beyond_bound_without_dropping: test\n'
            'benches::perf: bench\n'
            'Doc-tests sonicboom\n'
        )
        counts = parse_listing(text)
        self.assertEqual(counts['cancellation_does_not_release_running_permit'], 1)
        self.assertEqual(counts['enqueue_rejects_beyond_bound_without_dropping'], 1)
        self.assertEqual(counts['perf'], 1)

    def test_ignores_non_test_lines(self):
        self.assertEqual(parse_listing('foo: ok\nbar\n'), {})


class CheckTests(unittest.TestCase):
    def test_all_present_passes(self):
        text = '\n'.join(f'tests::{name}: test' for name in REQUIRED)
        self.assertEqual(check(text), len(REQUIRED))

    def test_missing_test_fails(self):
        text = '\n'.join(f'tests::{name}: test' for name in REQUIRED[:-1])
        with self.assertRaisesRegex(ValueError, 'missing regression tests'):
            check(text)

    def test_duplicate_test_fails(self):
        text = '\n'.join(f'tests::{name}: test' for name in REQUIRED)
        text += '\nother::module::' + REQUIRED[0] + ': test'
        with self.assertRaisesRegex(ValueError, 'matched more than once'):
            check(text)

    def test_renamed_test_is_reported_missing(self):
        text = '\n'.join(f'tests::{name}: test' for name in REQUIRED)
        text = text.replace('malformed_integers_fail_instead_of_defaulting',
                            'malformed_integers_fail')
        with self.assertRaisesRegex(ValueError, 'malformed_integers_fail_instead_of_defaulting'):
            check(text)


if __name__ == '__main__':
    unittest.main()
