// Generated from `jobs::Kind` by the app's tests. Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell.

/** The kinds of job Spotter registers, and whether their success is left out of the notifications. */
export const JOB_KINDS = [
  { slug: 'sp.download', quietOnSuccess: false },
  { slug: 'sp.install-renderer', quietOnSuccess: false },
  { slug: 'sp.check', quietOnSuccess: true },
  { slug: 'sp.check-all', quietOnSuccess: false },
  { slug: 'sp.test-source', quietOnSuccess: true },
] as const;

/** The slugs of the kinds whose success is quiet. */
export const QUIET_ON_SUCCESS_KINDS: ReadonlySet<string> = new Set(JOB_KINDS.filter((k) => k.quietOnSuccess).map((k) => k.slug));
