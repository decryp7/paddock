import { readdirSync, readFileSync } from 'node:fs';
import { checkRegistrationSource, isRegistrationFile } from '@decryp7/paddock-ui/shell/testing';

/**
 * Lazy by construction (debrief-1-2-api.md §14.3, workbench.md §9): a registration file (`*.contributions.ts`) imports
 * no store, service, backend or command class and no component; it holds static data and loaders. The shell's
 * `checkRegistrationSource` is the check; the family's ESLint or conformance rule for it is not built (the move plan's
 * §7 item 6, with Y12), so Debrief carries it as this spec.
 */
const SRC = 'src/app';

function registrationFiles(): { path: string; text: string }[] {
  return readdirSync(SRC, { recursive: true })
    .map((p) => String(p).replaceAll('\\', '/'))
    .filter((p) => isRegistrationFile(p))
    .map((p) => ({ path: p, text: readFileSync(`${SRC}/${p}`, 'utf8') }));
}

describe('registration files', () => {
  it('exist (the share features register through one)', () => {
    expect(registrationFiles().map((f) => f.path)).toContain('share/share.contributions.ts');
  });

  it('import no store, service, backend, command class or component', () => {
    expect(registrationFiles().flatMap((f) => checkRegistrationSource(f.path, f.text))).toEqual([]);
  });
});
