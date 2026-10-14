# Diffy landing page

Astro static site for https://diffy.hitslop.com. The page uses Commit Mono and existing Diffy artwork, with one small bundled script for the cursor-following SVG eye. It has no tracking or payment verification. Touch devices, reduced-motion users, and JavaScript failures keep the original static mascot. Animation stops when settled or offscreen.

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

Project name: `diffy-site`. Production branch: `main`. Build directory: `site/dist`.

```sh
wrangler login
npm run build
wrangler pages deploy dist --project-name diffy-site --branch main
```

This project uses Direct Upload. Rebuild with the checkout URL in your local build environment when ready, then deploy with the command above. Deployments marked `main` update production. Git commits do not automatically deploy this project; CI integration can be added separately.

Associate `diffy.hitslop.com` in the Pages project's custom domains, then point its DNS CNAME at the project's Pages hostname. Cloudflare must register the custom domain before its DNS redirect can work.

`public/_redirects` sends `/download` to the fixed-name `Diffy-macos-aarch64.dmg` in GitHub's latest release. The release script creates an identical copy of the signed and notarized versioned DMG. Upload it with every release; never point the website at an unsigned build.

The bundled web font uses the same Commit Mono v143 font as Diffy, under the included SIL Open Font License. The screenshot is stored losslessly as WebP. The mascot and social preview derive from existing project assets.
