import { hello } from './safe-a.mjs';

export function greet() {
  return 'greeting';
}

export function report() {
  return `${greet()}:${hello()}`;
}
