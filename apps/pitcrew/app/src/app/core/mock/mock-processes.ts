import type { KeepFor, ProcessEntry, ProcessList, ProcessPolicy, RunnerSnapshot, TimeUnit } from '../api.types';

/*
 * Invented process trees for the demo. The Rust core decides what is a leftover and what happens to it (ADR 0012);
 * the mock only looks plausible. Times are relative to the `now` the backend was built with.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
/** Process memory is a memory size: binary steps under the KB/MB/GB labels, as Activity Monitor. */
const MB = 1024 ** 2;

export const DEFAULT_PROCESS_POLICY: ProcessPolicy = {
  cleanup: true,
  keep: { amount: 10, unit: 'minutes' },
  stop_timeout_secs: 15,
  rules: [
    { pattern: '*GradleDaemon*', keep: { amount: 3, unit: 'hours' } },
    { pattern: '*KotlinCompileDaemon*', keep: { amount: 3, unit: 'hours' } },
    { pattern: 'qemu-system*', keep: { amount: 30, unit: 'minutes' } },
    { pattern: '*/emulator/emulator*', keep: { amount: 30, unit: 'minutes' } },
    { pattern: 'adb', keep: 'never' },
  ],
};

/** The time a keep amount stands for. */
export function keepMs(k: KeepFor): number {
  const unit: Record<TimeUnit, number> = { minutes: MINUTE, hours: HOUR, days: 24 * HOUR };
  return k.amount * unit[k.unit];
}

const JAVA = String.raw`"C:\Program Files\Eclipse Adoptium\jdk-21.0.4.7-hotspot\bin\java.exe" --add-opens=java.base/java.util=ALL-UNNAMED --add-opens=java.base/java.lang=ALL-UNNAMED --add-opens=java.base/java.lang.invoke=ALL-UNNAMED -Xmx2048m -Dfile.encoding=UTF-8 -Duser.country=US -Duser.language=en -Duser.variant -cp C:\Users\ci\.gradle\wrapper\dists\gradle-8.10.2-bin\gradle-launcher-8.10.2.jar -javaagent:C:\Users\ci\.gradle\caches\8.10.2\agents\gradle-instrumentation-agent-8.10.2.jar org.gradle.launcher.daemon.bootstrap.GradleDaemon 8.10.2`;

/** The process list of a running runner. A stopped one has none. */
export function demoProcesses(r: RunnerSnapshot, now: number): ProcessList {
  const base = {
    runnerPid: null,
    idle: r.currentJob === null,
    cleanup: true,
    keep: { amount: 10, unit: 'minutes' } as KeepFor,
    entries: [] as ProcessEntry[],
    truncated: false,
    takenAt: now,
  };
  if (r.runningSince === null || r.usage === null) return base;
  const runnerPid = 4120 + r.name.length * 8;
  const runner: ProcessEntry = {
    pid: runnerPid,
    ppid: 812,
    name: r.kind === 'forgejo-runner' ? 'forgejo-runner.exe' : 'gitea-runner.exe',
    command: `${r.dir}\\bin\\gitea-runner.exe daemon --config ${r.dir}\\config.yaml`,
    cpuPermille: 4,
    memBytes: 62 * MB,
    startedAt: r.runningSince,
    isRunner: true,
    job: null,
    leftover: null,
  };
  const out: ProcessList = { ...base, runnerPid, entries: [runner] };
  const job = r.currentJob;
  if (!job) return out;
  const ref = { repository: job.info.repository, job: job.info.job, runNumber: job.info.runNumber, running: true };
  const t = job.startedAt;
  const child = (pid: number, ppid: number, name: string, command: string, cpu: number, mb: number, age: number): ProcessEntry => ({
    pid,
    ppid,
    name,
    command,
    cpuPermille: cpu,
    memBytes: mb * MB,
    startedAt: t + age,
    isRunner: false,
    job: ref,
    leftover: null,
  });
  out.entries.push(
    child(7716, runnerPid, 'node.exe', String.raw`"C:\hostedtoolcache\windows\node\22.11.0\x64\node.exe" D:\a\_temp\_runner_file_commands\action.js --token [redacted]`, 12, 184, 4_000),
    child(9304, 7716, 'MSBuild.exe', String.raw`"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe" storefront.sln /nologo /m /p:Configuration=Release /p:Platform=x64 /v:minimal /p:PackageRegistryToken=[redacted]`, 128, 612, 21_000),
    child(10212, 9304, 'cl.exe', String.raw`"C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.41.34120\bin\HostX64\x64\CL.exe" /c /I..\include /Zi /nologo /W3 /O2 /D NDEBUG /MD /Fo"x64\Release\\" checkout.cpp`, 96, 312, 51_000),
    child(11036, runnerPid, 'conhost.exe', String.raw`\??\C:\Windows\system32\conhost.exe 0x4`, 0, 6, 4_100),
  );
  const left = (pid: number, name: string, command: string, cpu: number, mb: number, ageMin: number, since: number, rule: string | null, stopsIn: number | null) => {
    const e: ProcessEntry = {
      pid,
      ppid: null,
      name,
      command,
      cpuPermille: cpu,
      memBytes: mb * MB,
      startedAt: now - ageMin * MINUTE,
      isRunner: false,
      job: { repository: 'acme/payments-api', job: 'integration (postgres)', runNumber: 338, running: false },
      leftover: { since, rule, fate: stopsIn === null ? { kind: 'never' } : { kind: 'stopsAt', at: now + stopsIn } },
    };
    out.entries.unshift(e);
  };
  left(13512, 'adb.exe', String.raw`adb -L tcp:5037 fork-server server --reply-fd 704 --token [redacted]`, 0, 14, 240, now - 3 * HOUR, 'adb', null);
  left(13370, 'sleep.exe', 'sleep 3600', 0, 2, 31, now - 20 * MINUTE, null, 7 * MINUTE);
  left(
    12408,
    'java.exe',
    JAVA,
    2,
    518,
    182,
    now - 46 * MINUTE,
    '*GradleDaemon*',
    2 * HOUR + 14 * MINUTE,
  );
  return out;
}
