// Reduced from packages/vite/src/node/config.ts.
import { DEFAULTS } from './build.mjs';

// Read at module-evaluation time. Safe only if build.mjs has already run.
export const CONFIG = Object.freeze({ build: DEFAULTS });

export function resolveConfig(inline) {
  return { ...CONFIG, ...inline };
}
