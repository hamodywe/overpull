import { REGISTRY_LABEL } from './registry.mjs';

export const SERVICE_NAME = 'billing';

export function describeService() {
  return `${SERVICE_NAME} (${REGISTRY_LABEL})`;
}
