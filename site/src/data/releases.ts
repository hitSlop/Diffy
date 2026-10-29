import definitions from './platforms.json';
const published = new Set((import.meta.env.DIFFY_RELEASED_PLATFORMS || 'macos').split(',').map((value: string) => value.trim()));
export const platforms = definitions.map(platform => ({ ...platform, available: published.has(platform.id), href: `/download/${platform.id}` }));
export const price = { beta: '$7.99' };
