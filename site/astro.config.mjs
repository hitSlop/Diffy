import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://diffy.run',
  output: 'static',
  // Keep the eye controller external so script-src 'self' can remain strict.
  vite: { build: { assetsInlineLimit: 0 } },
});
