import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://diffy.hitslop.com',
  output: 'static',
  // Keep the eye controller external so script-src 'self' can remain strict.
  vite: { build: { assetsInlineLimit: 0 } },
});
