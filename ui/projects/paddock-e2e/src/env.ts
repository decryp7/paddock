/**
 * The e2e port of a worktree or CI job (rule 2: each agent its own port). Read from the environment variable `name`;
 * locally `fallback` is used when it is unset, on CI (`CI` set) a missing port is an error, because a fixed default
 * could collide with another job on the same runner host.
 */
export function e2ePort(name: string, fallback: number, env: Readonly<Record<string, string | undefined>> = process.env): number {
  const raw = env[name];
  if (raw === undefined || raw === '') {
    if (env['CI']) throw new Error(`CI: set ${name} (the e2e server's port); a fixed default could collide with another job`);
    return fallback;
  }
  const port = Number(raw);
  if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error(`${name} is not a port: ${JSON.stringify(raw)}`);
  return port;
}
