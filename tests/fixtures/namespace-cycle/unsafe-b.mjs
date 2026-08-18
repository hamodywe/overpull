import { hello } from './unsafe-a.mjs';

export const PREFIX = 'prefix';

export function report() {
  return `${PREFIX}:${hello()}`;
}
