import * as b from './unsafe-b.mjs';

export function hello() {
  return 'a';
}

// Same shape, one difference: PREFIX is a `const`, so it is in the temporal
// dead zone until unsafe-b.mjs evaluates. This line throws.
export const LABEL = b.PREFIX;
