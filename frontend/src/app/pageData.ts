import { z } from '../services/schema';
import type { PageData } from '../types';
const optionalString = z.string().nullish().transform(value => value ?? undefined);
const provider = z.object({ status: z.object({ provider_id: z.string(), state: z.string(), enabled: z.boolean(), effective_device: optionalString, active_model: optionalString, detail: optionalString }), models: z.string() });
const schema: z.ZodType<PageData> = z.union([
  z.object({ page: z.literal('home'), modelReady: z.boolean(), voices: z.array(z.string()), status: z.string(), providers: z.array(provider) }),
  z.object({ page: z.literal('setup') }),
  z.object({ page: z.enum(['admin', 'login', 'error']), error: optionalString, csrf: z.string().optional(), newToken: z.string().nullish(), tokens: z.array(z.object({ id: z.string(), fingerprint: z.string(), created: z.string(), expires: z.string(), status: z.string(), revoked: z.boolean() })).optional() }),
]);
export function parsePageData(value: string): PageData { return schema.parse(JSON.parse(value)); }
