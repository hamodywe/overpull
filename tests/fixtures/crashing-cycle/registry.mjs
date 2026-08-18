import { SERVICE_NAME } from './service.mjs';

// Runs while service.mjs is still initializing: SERVICE_NAME is in its
// temporal dead zone, so this line throws.
export const REGISTRY_LABEL = `registry:${SERVICE_NAME}`;
