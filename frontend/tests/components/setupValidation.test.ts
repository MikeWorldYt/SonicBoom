import { it, expect } from 'vitest';
import { setupIssues } from '../../src/features/auth/setupValidation';
const policy = { version: 1, password_min_chars: 12, rejected_passwords: ['example-password'], max_inference_steps: 50, auth_modes: ['local', 'token', 'none'], allow_insecure_remote: false };
const form = { usage_mode: 'advanced', bind: '127.0.0.1', port: 17842, auth_mode: 'local', admin_username: 'admin', admin_password: 'a-valid-long-passphrase', inference_steps: 5 };
it('enforces discovered password and number constraints using Unicode character counts', () => {
  expect(setupIssues(form, policy)).toEqual({});
  expect(setupIssues({ ...form, admin_password: '語'.repeat(11) }, policy)).toHaveProperty('admin_password');
  expect(setupIssues({ ...form, admin_password: '語'.repeat(12) }, policy)).toEqual({});
  expect(setupIssues({ ...form, admin_password: 'EXAMPLE-PASSWORD' }, policy)).toHaveProperty('admin_password');
  expect(setupIssues({ ...form, port: 0, inference_steps: 51, admin_username: '  ' }, policy)).toMatchObject({ port: expect.any(Object), inference_steps: expect.any(Object), admin_username: expect.any(Object) });
  expect(setupIssues(form, null)).toHaveProperty('policy');
});
it('validates IP binds and loopback/auth combinations without permitting insecure remote access', () => {
  for (const bind of ['::1', '0:0:0:0:0:0:0:1', '127.25.0.1']) expect(setupIssues({ ...form, bind }, policy)).toEqual({});
  for (const bind of ['localhost', '999.0.0.1', '127.1', '[::1]', 'fe80::1%eth0', '::g']) expect(setupIssues({ ...form, bind }, policy)).toHaveProperty('bind');
  expect(setupIssues({ ...form, bind: '0.0.0.0' }, policy)).toHaveProperty('auth_mode');
  expect(setupIssues({ ...form, bind: '::', auth_mode: 'token' }, policy)).toEqual({});
  expect(setupIssues({ ...form, bind: '::', auth_mode: 'none' }, policy)).toHaveProperty('auth_mode');
  expect(setupIssues({ ...form, bind: '::', auth_mode: 'none' }, { ...policy, allow_insecure_remote: true })).toEqual({});
});
