import { A } from './deferred-a.mjs';

export const LABEL = 'b';

// Reads A only when called, so nothing touches deferred-a.mjs while it is
// still evaluating.
export function withA() {
  return `${LABEL}:${A}`;
}
