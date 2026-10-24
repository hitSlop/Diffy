import type { APIRoute } from 'astro';
import { summary, source } from '../data/product';

export const GET: APIRoute = ({ site }) => {
  const root = new URL('/', site).href;
  return new Response(`# Diffy\n\n> ${summary}\n\nDiffy is published by hitSlop. macOS is beta; Windows and Linux are alpha. All platforms require Git.\n\n## Official documentation\n\n- [Product and features](${root}): What Diffy does and its current capabilities.\n- [Downloads and installation](${new URL('/downloads', root)}): Requirements, availability, and installation for each platform.\n- [Complete product documentation](${new URL('/llms-full.txt', root)}): Plain-text features, limitations, pricing, and platform facts.\n- [Source code](${source}): The official MIT-licensed repository.\n`, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
};
