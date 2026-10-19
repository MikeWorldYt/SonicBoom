import { QueryClient } from '@tanstack/vue-query';
/** One in-memory cache per studio. Keys use an opaque epoch, never a token. */
export function createResourceCache() {
  const client = new QueryClient({ defaultOptions: { queries: {
    retry: false, gcTime: Infinity, refetchOnWindowFocus: false, networkMode: 'always',
  } } });
  return {
    client,
    read<T>(epoch: number, resource: string, loader: (signal: AbortSignal) => Promise<T>, staleTime = 0) {
      return client.fetchQuery({ queryKey: ['studio', epoch, resource], queryFn: ({ signal }) => loader(signal), staleTime });
    },
    async invalidate(resource: string, cancel = false) { if (cancel) await client.cancelQueries({ predicate: query => query.queryKey[2] === resource }); await client.invalidateQueries({ predicate: query => query.queryKey[2] === resource, refetchType: 'none' }); },
    clear() { void client.cancelQueries(); client.clear(); },
  };
}
