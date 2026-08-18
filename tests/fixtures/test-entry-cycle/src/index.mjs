// The declared entry point. It reaches config.mjs first, which is why the
// package loads cleanly and why this cycle is not a `crash`.
export { resolveConfig, CONFIG } from './config.mjs';
export { build } from './build.mjs';
