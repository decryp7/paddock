import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { addEntryMains } from './entry-mains.mjs';

const changed = addEntryMains(join(dirname(fileURLToPath(import.meta.url)), '..', 'dist/paddock-ui'));
console.log(`entry-mains: main added to ${changed.length} secondary entry package.json files`);
