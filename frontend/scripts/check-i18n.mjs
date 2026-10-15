import { readFileSync, readdirSync } from 'node:fs';
const directory = new URL('../src/i18n/', import.meta.url);
const read = file => JSON.parse(readFileSync(new URL(file, directory), 'utf8'));
const english = read('en.json');
const parameters = text => [...text.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort().join(',');
const errors = [];
const catalogs = readdirSync(directory).filter(file => file.endsWith('.json'));
for (const file of catalogs) {
  const catalog = read(file);
  for (const key of new Set([...Object.keys(english), ...Object.keys(catalog)])) {
    if (typeof english[key] !== 'string' || typeof catalog[key] !== 'string' || !english[key] || !catalog[key]) errors.push(`${file}: missing translation: ${key}`);
    else if (parameters(english[key]) !== parameters(catalog[key])) errors.push(`${file}: interpolation parameters differ: ${key}`);
  }
}
if (errors.length) { console.error(errors.join('\n')); process.exit(1); }
console.log(`${catalogs.length} language catalogs: ${Object.keys(english).length} messages, matching keys and parameters.`);
