import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = path => readFileSync(new URL(`../dist/${path}`, import.meta.url), 'utf8');
const home = read('index.html');
assert(home.includes('$14.99') && home.includes('$24.99') && !home.includes('$19.99'));
assert(home.includes('<s>$24.99</s>') && home.includes('Diffy is in beta.'));
const checkout = process.env.DODO_CHECKOUT_URL;
assert.equal(home.includes('checkout.dodopayments.com/buy/'), !!checkout);
if (checkout) assert(home.includes('redirect_url=https%3A%2F%2Fdiffy.hitslop.com%2Fthanks'));
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
console.log('Pricing, checkout, platform availability, and redirects verified.');
