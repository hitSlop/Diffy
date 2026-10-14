# Diffy landing page

Astro static site for https://diffy.hitslop.com. The page uses Commit Mono and a compact, responsive hero banner from `src/assets/hero.png`, with one small bundled script for the cursor-following SVG eye. It has no tracking or payment verification. Touch devices, reduced-motion users, and JavaScript failures keep the original static artwork. Animation stops when settled or offscreen.

```sh
cd site
npm ci
npm run dev
npm run build
npm run preview
```

## Checkout

Set `DODO_CHECKOUT_URL` to the HTTPS hosted product checkout URL (for example `https://checkout.dodopayments.com/buy/pdt_example`) at build time. Do not set a private API key. Without the URL, the purchase button is disabled and shows “Checkout coming soon.”

Configure a one-time USD $19.99 product in Dodo. The build sets the checkout return URL to `https://diffy.hitslop.com/thanks`. Put `https://diffy.hitslop.com/download` in the product's delivery instructions/external delivery link as well, so buyers can keep it. Dodo setup remains manual until the product is ready.

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

Associate `diffy.hitslop.com` in the Pages project's custom domains, then point its DNS CNAME at the project's Pages hostname. Cloudflare must register the custom domain before its DNS redirect can work.

`public/_redirects` sends `/download` to the fixed-name `Diffy-macos-aarch64.dmg` in GitHub's latest release. The release script creates an identical copy of the signed and notarized versioned DMG. Upload it with every release; never point the website at an unsigned build.

The bundled web font uses the same Commit Mono v143 font as Diffy, under the included SIL Open Font License. The screenshot is stored losslessly as WebP. The mascot and social preview derive from existing project assets.

The screenshot serves 480/960/1440px variants through `srcset`; the original 1920px image remains the full-resolution fallback and zoom link. Pointer motion is coalesced so the eye controller reads geometry at most once per animation frame.
