import { LABEL } from './deferred-b.mjs';

export const A = 'a';

// The same arrow function, never invoked at module scope. Nothing reads
// LABEL until someone calls this, long after both modules have evaluated.
export const describe = () => `label:${LABEL}`;
