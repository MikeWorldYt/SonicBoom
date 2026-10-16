// Minimal browser globals for frontend unit tests running under plain node.
// Only what imported modules touch at load time; components under test must
// null-check anything else themselves.
if (typeof globalThis.localStorage === 'undefined') {
  const store = new Map();
  globalThis.localStorage = {
    getItem: (key) => (store.has(key) ? store.get(key) : null),
    setItem: (key, value) => void store.set(key, String(value)),
    removeItem: (key) => void store.delete(key),
    clear: () => store.clear(),
  };
}
if (typeof globalThis.navigator === 'undefined') {
  globalThis.navigator = { language: 'en-US' };
}
if (typeof globalThis.document === 'undefined') {
  // Absorbing element stub: unit tests never render, but vue's runtime-dom
  // touches document at import time.
  const elementStub = () =>
    new Proxy(
      {},
      {
        get: () => elementStub(),
        set: () => true,
      },
    );
  globalThis.document = {
    documentElement: { lang: '' },
    title: '',
    hidden: false,
    createElement: elementStub,
    createElementNS: elementStub,
    createTextNode: (text) => ({ textContent: text }),
    addEventListener: () => {},
    removeEventListener: () => {},
  };
}
