import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../dist/${path}`, import.meta.url), 'utf8');
const home = read('index.html');
assert(home.includes('$12.99') && home.includes('$24.99') && !home.includes('$19.99') && !home.includes('$14.99'));
assert(home.includes('<s>$24.99</s>') && home.includes('during beta'));
assert(!home.includes('Downloads &amp; installation') && !home.includes('Diffy is in beta.'));
const checkout = process.env.DODO_CHECKOUT_URL;
assert.equal(home.includes('checkout.dodopayments.com/buy/'), !!checkout);
if (checkout) assert(home.includes('redirect_url=https%3A%2F%2Fdiffy.run%2Fthanks'));
const published = new Set((process.env.DIFFY_RELEASED_PLATFORMS || 'macos').split(','));
const redirects = read('_redirects');
for (const page of ['downloads/index.html', 'thanks/index.html']) {
  const html = read(page);
  for (const platform of ['macos', 'windows', 'linux']) {
    assert(html.includes(`id="${platform}"`));
    assert.equal(html.includes(`href="/download/${platform}"`), published.has(platform));
    const line = redirects.split('\n').find(line => line.startsWith(`/download/${platform} `));
    assert(line && line.endsWith(' 302'));
    assert.equal(line.includes('github.com'), published.has(platform));
  }
}
assert(redirects.includes('/download https://github.com/hitSlop/diffy/releases/latest/download/Diffy-macos-aarch64.dmg 302'));
for (const [page, path] of [['index.html', '/'], ['downloads/index.html', '/downloads/'], ['thanks/index.html', '/thanks/']]) {
  const html = read(page);
  assert(!html.includes('diffy.hitslop.com'));
  assert(html.includes(`<link rel="canonical" href="https://diffy.run${path}"`));
  assert(html.includes('content="https://diffy.run/_astro/'));
  assert.equal(html.includes('noindex, follow'), path === '/thanks/');
}
const schema = JSON.parse(home.match(/<script type="application\/ld\+json">(.*?)<\/script>/s)[1]);
const app = schema['@graph'].find(entity => entity['@type'] === 'SoftwareApplication');
assert.equal(app.url, 'https://diffy.run/');
assert.deepEqual(app.operatingSystem, ['macOS', 'Windows', 'Linux']);
assert(app.sameAs.includes('https://github.com/hitSlop/Diffy'));
assert(!app.offers && !app.aggregateRating);
assert(home.includes('Diffy is a native Git diff viewer built in Rust with GPUI Kit.'));
const robots = read('robots.txt');
assert(robots.includes('User-agent: *\nAllow: /'));
assert(robots.includes('Sitemap: https://diffy.run/sitemap.xml'));
const sitemap = read('sitemap.xml');
assert(sitemap.includes('<loc>https://diffy.run/</loc>'));
assert(sitemap.includes('<loc>https://diffy.run/downloads/</loc>'));
assert(!sitemap.includes('thanks') && !sitemap.includes('404'));
for (const file of ['llms.txt', 'llms-full.txt']) {
  const markdown = read(file);
  assert(markdown.startsWith('# Diffy\n'));
  assert(markdown.includes('https://diffy.run/downloads'));
  assert(!markdown.includes('diffy.hitslop.com'));
}
const full = read('llms-full.txt');
assert.equal(full.includes('Checkout is not yet available.'), !checkout);
for (const platform of ['macos', 'windows', 'linux']) {
  assert.equal(full.includes(`/downloads/#${platform}`), published.has(platform));
}
assert(read('404.html').includes('noindex, follow'));
console.log('Pricing, checkout, downloads, canonical URLs, crawler access, schema, sitemap, and AI documentation verified.');
