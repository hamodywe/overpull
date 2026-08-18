import { NAME } from './invoked-b.mjs';

export const A = 'a';

// An IIFE runs where it is written. invoked-b.mjs has not evaluated yet, so
// this reads NAME inside its temporal dead zone and throws — even though the
// read is syntactically inside an arrow function.
(() => {
  globalThis.__iifeResult = NAME;
})();
