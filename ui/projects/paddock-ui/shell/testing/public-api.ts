/**
 * `@decryp7/paddock-ui/shell/testing`: the shell's test kit for TestBed (ADR 0012 §7). From plan S2: the contributions
 * lint (`assertContributionsValid`) and the lazy-registration lint (`checkRegistrationSource`). `provideShellTesting` and
 * the boot-every-dialog harness arrive with plan S6 and S16.
 */
export { contributionProblems, assertContributionsValid } from './src/contributions-lint';
export { checkRegistrationSource, assertRegistrationsLazy, isRegistrationFile, staticImports } from './src/registration-lint';
