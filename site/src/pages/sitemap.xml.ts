import type { APIRoute } from 'astro';

export const GET: APIRoute = ({ site }) => new Response(`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${['/', '/downloads/'].map(path => `  <url><loc>${new URL(path, site)}</loc></url>`).join('\n')}\n</urlset>\n`, {
  headers: { 'Content-Type': 'application/xml; charset=utf-8' },
});
