// A spec file whose first runtime import is the *other* half of the cycle.
// Loaded on its own, this produces the failing order.
import { build } from '../src/build.mjs';

export const RESULT = build({});
