// Proves this cycle is harmless: the modules load and compute correctly.
import { isEven } from './even.mjs';

if (isEven(10) !== true || isEven(7) !== false) {
  console.error('FAIL: mutual recursion produced the wrong answer');
  process.exit(1);
}
console.log('OK: benign cycle loads and runs');
