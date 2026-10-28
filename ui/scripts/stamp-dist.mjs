// Stamps ui/dist with the hash of its sources after `ng build paddock-ui`, for the e2e build guard (build-guard.ts).
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { stampBuild } from '../projects/paddock-e2e/src/build-guard.ts';

stampBuild(join(dirname(fileURLToPath(import.meta.url)), '..'), 'dist');
