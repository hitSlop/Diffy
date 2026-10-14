import { readFileSync, writeFileSync } from 'node:fs';
const platforms = JSON.parse(readFileSync(new URL('../src/data/platforms.json', import.meta.url), 'utf8'));
const published = new Set((process.env.DIFFY_RELEASED_PLATFORMS || 'macos').split(',').map(value => value.trim()));
const base = 'https://github.com/hitSlop/diffy/releases/latest/download/';
const lines = [`/download ${base}Diffy-macos-aarch64.dmg 302`];
for (const platform of platforms) {
  lines.push(`/download/${platform.id} ${published.has(platform.id) ? base + platform.asset : '/downloads#' + platform.id} 302`);
}
writeFileSync(new URL('../dist/_redirects', import.meta.url), lines.join('\n') + '\n');
