import { register } from 'node:module';

register('./extension-resolver.mjs', import.meta.url);
