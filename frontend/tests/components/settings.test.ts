import { it, expect } from 'vitest';
import { parseSetting } from '../../src/services/settings';
import type { SettingSchema } from '../../src/services/contracts';
const meta: SettingSchema = { path: 'voices.max_upload_bytes', description: '', type: 'integer', apply: 'hot', min: 1024, max: 67108864, nullable: false, secret: false };
it('settings accept only schema ranges, enums, boolean and nullable values', () => {
  expect(parseSetting(meta, '1024')).toBe(1024);
  for (const value of [' ', 'NaN', '1023', '1024.5', '67108865']) expect(() => parseSetting(meta, value)).toThrow();
  const enumeration: SettingSchema = { ...meta, type: 'string', allowed_values: ['local', 'token'] };
  expect(parseSetting(enumeration, 'token')).toBe('token');
  expect(() => parseSetting(enumeration, 'unknown')).toThrow();
  expect(parseSetting({ ...enumeration, nullable: true }, '')).toBeNull();
  expect(() => parseSetting({ ...meta, type: 'boolean' }, 'oops')).toThrow();
});
