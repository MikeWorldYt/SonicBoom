import { it, expect, vi } from 'vitest';
import { createResourceCache } from '../../src/app/queryClient';
it('deduplicates concurrent reads and invalidates after mutations', async () => {
  const cache = createResourceCache();
  let resolve!: (value: string) => void;
  const loader = vi.fn(() => new Promise<string>(done => { resolve = done; }));
  const first = cache.read(0, 'config-status', loader), second = cache.read(0, 'config-status', loader);
  expect(loader).toHaveBeenCalledTimes(1); resolve('current');
  expect(await first).toBe('current'); expect(await second).toBe('current');
  await cache.invalidate('config-status');
  await cache.read(0, 'config-status', async () => 'new');
  expect(cache.client.getQueryData(['studio', 0, 'config-status'])).toBe('new');
  cache.clear();
});
it('cancels old reads, empties scoped data and never stores token strings in keys', async () => {
  const cache = createResourceCache();
  let signal!: AbortSignal;
  const pending = cache.read(0, 'voices', async value => { signal = value; return new Promise(() => {}); });
  const rejection = expect(pending).rejects.toBeDefined();
  cache.clear(); expect(signal.aborted).toBe(true); await rejection;
  expect(cache.client.getQueryCache().getAll()).toHaveLength(0);
  await cache.read(1, 'voices', async () => []);
  expect(cache.client.getQueryCache().getAll()[0].queryKey).toEqual(['studio', 1, 'voices']);
  cache.clear();
});
