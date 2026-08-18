// Reduced from packages/vite/src/node/build.ts.
import { resolveConfig } from './config.mjs';

export const DEFAULTS = Object.freeze({ target: 'modules' });

export function build(inline) {
  return resolveConfig(inline);
}
