// Nothing imports this and nothing declares it. It is a leftover, not a
// program start: a cycle only reachable from here is conditional.
import { CORE } from './core.js';

export const LEGACY = `legacy:${CORE}`;
