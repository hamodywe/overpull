import type { User } from './types.js';
import { titleCase } from '../text/case.js';

export function formatUser(user: User): string {
  return `${titleCase(user.name)} <${user.email}>`;
}
