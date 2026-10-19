import { z } from 'zod';
// The production CSP prohibits eval, including Zod's optional feature probe.
// Configure this before constructing any application schemas.
z.config({ jitless: true });
export { z };
