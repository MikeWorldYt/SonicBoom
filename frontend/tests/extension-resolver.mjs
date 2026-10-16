import { existsSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';

// Lets node --test resolve the extensionless relative imports used across
// frontend/src (bundlers handle these; plain node does not).
export async function resolve(specifier, context, next) {
  try {
    return await next(specifier, context);
  } catch (error) {
    if (specifier.startsWith('./') || specifier.startsWith('../') || specifier.startsWith('/')) {
      const parentPath = context.parentURL ? fileURLToPath(context.parentURL) : process.cwd();
      const base = path.resolve(path.dirname(parentPath), specifier);
      for (const candidate of [`${base}.ts`, path.join(base, 'index.ts')]) {
        if (existsSync(candidate)) {
          return { url: pathToFileURL(candidate).href, shortCircuit: true };
        }
      }
    }
    throw error;
  }
}
