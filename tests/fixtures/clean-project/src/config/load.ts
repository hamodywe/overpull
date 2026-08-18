import { readFile } from 'node:fs/promises';

import { titleCase } from '../text/case.js';

export async function loadConfig(path: string): Promise<Record<string, string>> {
  const raw = await readFile(path, 'utf8');
  const parsed: Record<string, string> = JSON.parse(raw);
  parsed.label = titleCase(parsed.label ?? 'default');

  if (parsed.plugins) {
    const { applyPlugins } = await import('./plugins.js');
    applyPlugins(parsed);
  }
  return parsed;
}
