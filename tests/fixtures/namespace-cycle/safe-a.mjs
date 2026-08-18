import * as b from './safe-b.mjs';

export function hello() {
  return 'a';
}

// b.mjs has NOT evaluated yet when this line runs. Reading `greet` off its
// namespace is still legal: function declarations are initialized during
// instantiation, before any module body runs.
export const LABEL = b.greet();
