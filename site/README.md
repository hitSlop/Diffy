# Diffy landing page

Astro static site for https://diffy.run. The page uses Commit Mono and a compact, responsive hero banner from `src/assets/hero.png`, with one small bundled script for the cursor-following SVG eye. It has no tracking or payment verification. Touch devices, reduced-motion users, and JavaScript failures keep the original static artwork. Animation stops when settled or offscreen.

```sh
cd site
npm ci
npm run dev
npm run build
npm run preview
```

## Checkout

Set `DODO_CHECKOUT_URL` to the HTTPS hosted product checkout URL (for example `https://checkout.dodopayments.com/buy/pdt_example`) at build time. Do not set a private API key. Without the URL, the purchase button is disabled and shows “Checkout coming soon.”

Configure a one-time USD $14.99 product in Dodo. The build sets the checkout return URL to `https://diffy.run/thanks`. Put `https://diffy.run/downloads` in a Digital Files entitlement's external delivery link and attach it to the product as well, so buyers can keep it. Dodo setup remains manual until the product is ready.

```sh
DODO_CHECKOUT_URL='https://checkout.dodopayments.com/buy/YOUR_PRODUCT_ID' npm run build
```

Payment is suggested support, not an entitlement gate. `/thanks` and `/download` remain public. The return page does not claim a payment was verified.

## Cloudflare Pages

Project name: `diffy-site`. Cloudflare production deployment label: `main`. Repository production branch: `master`. Build directory: `site/dist`.

```sh
wrangler login
npm run build
wrangler pages deploy dist --project-name diffy-site --branch main
```

This project uses Direct Upload. Deployments marked `main` update production; that label is independent of the repository's `master` branch.

The Website workflow checks both checkout configurations on PRs. After both checks pass, pushes to `master` deploy production. Manual workflow runs on `master` can redeploy it. PRs and other branches do not deploy or receive Cloudflare credentials.

Before merging the deployment workflow, configure these repository settings:

- Actions secret `CLOUDFLARE_API_TOKEN`: a Cloudflare API token with **Account → Cloudflare Pages → Edit**, restricted to the account hosting `diffy-site`. Use a dedicated CI token; do not copy a local Wrangler OAuth or refresh token into GitHub.
- Repository variable `CLOUDFLARE_ACCOUNT_ID`: the account hosting `diffy-site`.
- Optional repository variable `DODO_CHECKOUT_URL`: the real HTTPS hosted checkout URL. Leave it unset while checkout is unavailable.

Production builds separately from the test matrix, so the placeholder checkout URL cannot be deployed. The pinned Wrangler action uploads the build using the production `main` label and attaches the exact Git commit hash. Deployment jobs are serialized.

The commands above remain available for manual deployment with your existing local Wrangler login. Rebuild with the correct production checkout setting before uploading; never deploy an artifact built with the CI checkout fixture.

Keep domain registration at Namecheap and use Cloudflare DNS for `diffy.run`. Associate `diffy.run` in the Pages project's custom domains, then point its proxied apex CNAME at `diffy-site.pages.dev`. The Pages custom-domain association is required in addition to DNS. `www.diffy.run` redirects to the apex, preserving paths and query strings. The former subdomain has been retired without a redirect.

## Crawlers and product facts

The site is prerendered HTML; product information and download instructions do not require JavaScript. `robots.txt` allows all crawlers and advertises `/sitemap.xml`. Keep Cloudflare's Search, Agent, and Training crawler policies set to Allow, with no AI crawler paywall or bot challenges on public documentation.

`src/data/product.ts` supplies the product definition, visible questions, and `/llms-full.txt`; `/llms.txt` links to the canonical documentation. These text endpoints are an additional convenience for assistants, not a ranking guarantee. `/sitemap.xml` includes the homepage and downloads; `/thanks` and the error page use `noindex, follow`. JSON-LD describes the publisher, website, pages, and software application using factual capabilities, with no fabricated reviews or unavailable purchase offers. Update the shared facts when availability or pricing changes.

The post-build `scripts/write-redirects.mjs` generates `dist/_redirects` from the same platform metadata used by the pages. `/download` retains its macOS behavior. `/download/macos`, `/download/windows`, and `/download/linux` use fixed-name assets in GitHub's latest release; unavailable targets redirect to `/downloads` instead. The release script creates an identical copy of the signed and notarized versioned DMG. Upload it with every release; never point the website at an unsigned build.

The bundled web font uses the same Commit Mono v143 font as Diffy, under the included SIL Open Font License. The screenshot is stored losslessly as WebP. Open Graph and Twitter link previews use the same meme as the hero, from `src/assets/hero.png`.

The screenshot serves 480/960/1440px variants through `srcset`; the original 1920px image remains the full-resolution fallback and zoom link. Pointer motion is coalesced so the eye controller reads geometry at most once per animation frame.

## Beta pricing and platform availability

The landing page displays regular price $24.99 crossed out and $14.99 during beta. It keeps the purchase CTA and “Or build for free”; platform downloads are offered after purchase on `/thanks` and through Dodo delivery. There is no separate beta announcement line. macOS is Beta; Windows/Linux are Alpha. The purchase supports all platforms. Keep the Dodo product's actual one-time USD price at 1499 cents to match the site.

`src/data/platforms.json` supplies download choices, requirements, asset names, and installation instructions for `/downloads` and `/thanks`. `DIFFY_RELEASED_PLATFORMS` is a comma-separated list of published platforms, default `macos`. After the complete release is verified and published, set the GitHub repository variable to `macos,windows,linux` and rebuild the site. Keep it at `macos` until then; the site never advertises missing assets.

`node scripts/check-build.mjs` checks the built pricing, checkout return URL, availability, and redirect targets. Run it with the same environment used for `npm run build`. CI exercises both checkout configurations and all-platform availability.

## Dodo dashboard setup

No Dodo CLI or product is configured for this repository. Create one **Diffy — all desktop platforms** product with a one-time **USD $14.99** price (1499 cents), covering macOS beta and Windows/Linux alpha. Create a Digital Files entitlement named **Diffy downloads**, set its external link to **https://diffy.run/downloads**, and attach it to the product. Suggested delivery instructions:

> Choose your platform on the download page. All versions require Git. macOS is beta; Windows and Linux are early alphas with limited hardware testing. Save the page for future releases.

Copy the hosted `https://checkout.dodopayments.com/buy/PRODUCT_ID` URL into the GitHub `DODO_CHECKOUT_URL` repository variable. The site adds the return URL `/thanks`. Keep checkout disabled until the product exists and its displayed charge matches $14.99. Public downloads and the return page do not verify payment or grant a license.
