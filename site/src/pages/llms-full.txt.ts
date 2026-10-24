import type { APIRoute } from 'astro';
import { productMarkdown } from '../data/product';

export const GET: APIRoute = ({ site }) => new Response(productMarkdown(new URL('/', site).href), {
  headers: { 'Content-Type': 'text/plain; charset=utf-8' },
});
