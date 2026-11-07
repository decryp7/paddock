// Release row (Dyno 1.1.0): the disk figures Dyno shows are the OS's own, in decimal units, and follow a write within 60 s.
//  - free space on the system volume: `dyno status --json` freeBytes against the OS's figure (macOS: the Finder's "available" capacity, df as the second reading;
//    Windows: Win32_LogicalDisk, what Explorer shows; Linux: df) within 1 %
//  - decimal units: `dyno status` prints "<n> GB" for 10^9 bytes (never GiB), checked against the JSON bytes
//  - refresh: a 1 GB file is written, Dyno's free figure must drop by about 1 GB within 60 s, and rise again within 60 s after the file is deleted.
// Pure judge + a driver over an OS adapter (`exec`). A reading that cannot be taken is a broken check (ER-33), never a pass.
import { makeResult } from './verdict.mjs';

export const TITLE_DISK = 'Release row: disk space matches the OS within 1 %, uses decimal units and refreshes within 60 s of a 1 GB write';
export const REFRESH_WAIT_MS = 60_000;
export const WRITE_BYTES = 1_000_000_000;
const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** Parse `dyno status --json` into [{name, freeBytes, totalBytes}] (null when it is not that JSON). */
export function parseDisks(text) {
  try { const j = JSON.parse(String(text).slice(String(text).indexOf('{'))); return Array.isArray(j.disks) ? j.disks.filter((d) => Number.isFinite(d.freeBytes) && Number.isFinite(d.totalBytes)) : null; } catch { return null; }
}

/** The decimal text Dyno prints for a byte count: "12.3 GB". Returns {value, unit} of the first "<n> <unit> free of" on a Disk line for this name, or null. */
export function printedFree(statusText, name) {
  for (const l of String(statusText).split('\n')) {
    if (!/^\s*Disk\b/.test(l) || !l.includes(name)) continue;
    const m = /([\d.,]+)\s*([KMGTP]?i?B)\s+free of\s+([\d.,]+)\s*([KMGTP]?i?B)/.exec(l);
    if (m) return { free: Number(m[1].replace(',', '.')), freeUnit: m[2], total: Number(m[3].replace(',', '.')), totalUnit: m[4], line: l.trim() };
  }
  return null;
}
const POW = { B: 0, KB: 1, MB: 2, GB: 3, TB: 4, PB: 5 };

/** Pure judge. f: { disks (Dyno JSON), volumeName, osFreeBytes, osSource, osAltFreeBytes?, osAltSource?, statusText, baselineFree, afterWriteFree, writeSecs, afterDeleteFree, deleteSecs, writeBytes } */
export function judgeDisk(f) {
  const checks = [];
  const d = (f.disks ?? []).find((x) => x.name === f.volumeName) ?? null;
  checks.push(chk(Boolean(d), d ? `Dyno lists the system volume "${f.volumeName}" (free ${d.freeBytes} of ${d.totalBytes} bytes)` : `Dyno's disk list has no volume "${f.volumeName}": ${(f.disks ?? []).map((x) => x.name).join(', ') || '(none)'}`));
  if (!d) return checks;
  checks.push(chk(Number.isFinite(f.osFreeBytes) && f.osFreeBytes > 0, Number.isFinite(f.osFreeBytes) ? `the OS figure (${f.osSource}) is ${f.osFreeBytes} bytes` : `the OS figure could not be read (${f.osSource})`, 'check'));
  if (Number.isFinite(f.osFreeBytes) && f.osFreeBytes > 0) {
    const rel = Math.abs(d.freeBytes - f.osFreeBytes) / f.osFreeBytes;
    checks.push(chk(rel <= 0.01, `Dyno free ${d.freeBytes} vs ${f.osSource} ${f.osFreeBytes}: ${(rel * 100).toFixed(2)} % apart (limit 1 %)`));
    if (Number.isFinite(f.osAltFreeBytes)) checks.push(chk(true, `for information, ${f.osAltSource} reads ${f.osAltFreeBytes} (${(Math.abs(d.freeBytes - f.osAltFreeBytes) / f.osAltFreeBytes * 100).toFixed(2)} % from Dyno)`));
  }
  const p = printedFree(f.statusText, f.volumeName);
  if (!p) checks.push(chk(false, `no "Disk ... free of ..." line for "${f.volumeName}" in dyno status text`, 'check'));
  else {
    const dec = p.freeUnit in POW && !p.freeUnit.includes('i');
    checks.push(chk(dec, dec ? `dyno status prints decimal units: "${p.line}"` : `dyno status prints binary units (${p.freeUnit}): "${p.line}"`));
    if (dec) { const bytes = p.free * 1000 ** POW[p.freeUnit]; const rel = Math.abs(bytes - d.freeBytes) / d.freeBytes; checks.push(chk(rel <= 0.06, `the printed figure ${p.free} ${p.freeUnit} is the JSON ${d.freeBytes} bytes in 10^3 steps (${(rel * 100).toFixed(1)} % off, rounding allowed)`)); }
  }
  const w = f.writeBytes ?? WRITE_BYTES;
  // Control: the OS's own figure must have fallen by the write, else the write did not take space and a Dyno that does not move is not at fault.
  if (Number.isFinite(f.osAfterWrite) && Number.isFinite(f.osBaseline)) checks.push(chk(f.osBaseline - f.osAfterWrite >= w * 0.9, `control: the OS figure fell by ${f.osBaseline - f.osAfterWrite} bytes right after the ${w}-byte write`, 'check'));
  if (Number.isFinite(f.afterWriteFree)) {
    const drop = f.baselineFree - f.afterWriteFree;
    checks.push(chk(drop >= w * 0.9 && f.writeSecs <= 60, drop >= w * 0.9 ? `after writing ${w} bytes Dyno's free figure fell by ${drop} bytes after ${f.writeSecs} s (limit 60 s)` : `after ${f.writeSecs} s Dyno's free figure fell by only ${drop} bytes for a ${w}-byte write (needs at least ${Math.round(w * 0.9)})`));
  } else checks.push(chk(false, 'Dyno\'s free figure was not read after the write', 'check'));
  if (Number.isFinite(f.afterDeleteFree)) {
    const rise = f.afterDeleteFree - (f.afterWriteFree ?? f.baselineFree);
    checks.push(chk(rise >= w * 0.9 && f.deleteSecs <= 60, rise >= w * 0.9 ? `after deleting the file the figure rose by ${rise} bytes after ${f.deleteSecs} s` : `after deleting the file the figure rose by only ${rise} bytes in ${f.deleteSecs} s`));
  }
  return checks;
}

const num = (s) => { const n = Number(String(s ?? '').trim().split(/\s+/)[0]); return Number.isFinite(n) ? n : NaN; };

/**
 * Per-OS adapters: { osFree(), osAlt?(), volumeName, writeCmd, deleteCmd, statusCmd, tmpPath }. Driver: `exec(cmd) -> {code, stdout}`, `sleep(ms)`, `now()`.
 * The status commands run as the session user (they need agent.json).
 */
export async function diskSpaceStep({ exec, sleep, now, adapter }) {
  const read = async () => { const r = await exec(adapter.statusJsonCmd); const disks = parseDisks(r.stdout); return disks; };
  let volumeName = adapter.volumeName;
  const freeOf = (disks) => disks?.find((x) => x.name === volumeName)?.freeBytes;
  try {
    const disks = await read();
    if (!disks) throw new Error('dyno status --json gave no disk list (is Dyno running?)');
    // The system volume: a preferred name when Dyno lists it, else the largest volume it lists.
    if (!volumeName && adapter.resolveVolume) volumeName = await adapter.resolveVolume(exec, disks).catch(() => undefined);
    if (!volumeName) volumeName = disks.find((x) => (adapter.preferNames ?? []).includes(x.name))?.name ?? [...disks].sort((a, b) => b.totalBytes - a.totalBytes)[0]?.name;
    const statusText = (await exec(adapter.statusTextCmd)).stdout;
    const os = await adapter.osFree(exec);
    const alt = adapter.osAlt ? await adapter.osAlt(exec).catch(() => null) : null;
    // the OS figure is read right after Dyno's, so background churn is small; re-read Dyno's value as the baseline for the write test
    const baselineFree = freeOf((await read())) ?? freeOf(disks);
    const w0 = await exec(adapter.writeCmd);
    if (w0.code !== 0 || (adapter.writeOkPattern && !adapter.writeOkPattern.test(w0.stdout))) throw new Error(`writing the 1 GB file failed: ${String(w0.stdout).trim().slice(-160)}`);
    const osAfterWrite = (await adapter.osFree(exec)).bytes;
    const t0 = now(); let afterWriteFree = NaN, writeSecs = NaN;
    for (;;) { afterWriteFree = freeOf(await read()); writeSecs = Math.round((now() - t0) / 1000); if (baselineFree - afterWriteFree >= WRITE_BYTES * 0.9 || now() - t0 >= REFRESH_WAIT_MS) break; await sleep(3000); }
    const osAfterWait = (await adapter.osFree(exec)).bytes;
    await exec(adapter.deleteCmd);
    const t1 = now(); let afterDeleteFree = NaN, deleteSecs = NaN;
    for (;;) { afterDeleteFree = freeOf(await read()); deleteSecs = Math.round((now() - t1) / 1000); if (afterDeleteFree - afterWriteFree >= WRITE_BYTES * 0.9 || now() - t1 >= REFRESH_WAIT_MS) break; await sleep(3000); }
    const checks = judgeDisk({ disks, volumeName, osFreeBytes: os.bytes, osSource: os.source, osAltFreeBytes: alt?.bytes, osAltSource: alt?.source, statusText, baselineFree, afterWriteFree, writeSecs, afterDeleteFree, deleteSecs, osBaseline: os.bytes, osAfterWrite: Math.min(osAfterWrite, Number.isFinite(osAfterWait) ? osAfterWait : osAfterWrite) });
    return makeResult(TITLE_DISK, checks, [`OS: ${os.source} ${os.bytes}`, `status: ${statusText.split('\n').filter((l) => /^\s*Disk/.test(l)).join(' | ')}`], 'the disk figures match the OS, use decimal units and follow a 1 GB write and delete within 60 s');
  } catch (e) {
    await exec(adapter.deleteCmd).catch(() => {});
    return { title: TITLE_DISK, status: 'not-measured', cls: 'check', summary: `not measured: ${String(e.message ?? e).split('\n')[0]}`, checks: [{ ok: false, kind: 'check', detail: String(e.message ?? e) }], evidence: [] };
  }
}

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
/** Linux: the volume name Dyno gives "/" is read from the JSON (the entry whose name is the root mount or the largest); df -B1 avail is the OS figure. */
export function unixAdapter({ os, dyno, volumeName = undefined, tmp = '/var/tmp/vmsmoke-1gb.bin' }) {
  return {
    volumeName, preferNames: ['/', 'Macintosh HD', '/System/Volumes/Data'],
    statusJsonCmd: `${dyno} status --json 2>/dev/null`, statusTextCmd: `${dyno} status 2>/dev/null`,
    writeCmd: `head -c ${WRITE_BYTES} /dev/zero > ${sq(tmp)} && sync`, deleteCmd: `rm -f ${sq(tmp)}; sync`,
    async osFree(exec) {
      if (os === 'macos') {
        const r = await exec(`osascript -l JavaScript -e 'ObjC.import("Foundation"); var u=$.NSURL.fileURLWithPath("/"); var k=$.NSURLVolumeAvailableCapacityForImportantUsageKey; var o=Ref(); u.getResourceValueForKeyError(o,k,null); ObjC.unwrap(o[0])'`);
        return { bytes: num(r.stdout), source: 'Finder (NSURLVolumeAvailableCapacityForImportantUsage)' };
      }
      return { bytes: num((await exec(`df -B1 --output=avail / | tail -1`)).stdout), source: 'df -B1 (Files app)' };
    },
    // Linux: Dyno names a volume by its device (/dev/vda1); the system volume is the source of / (the largest one can be a host share such as `rosetta`).
    async resolveVolume(exec, disks) { if (os !== 'linux') return undefined; const src = (await exec('findmnt -no SOURCE /')).stdout.trim().split('\n').at(0); return disks.find((d) => d.name === src)?.name; },
    async osAlt(exec) { return os === 'macos' ? { bytes: num((await exec(`df -k / | tail -1 | awk '{print $4*1024}'`)).stdout), source: 'df' } : { bytes: num((await exec(`stat -f -c '%a*%S' / | bc`)).stdout), source: 'stat -f' }; },
  };
}

/** Windows (PowerShell over ssh as the session user): `ssh(cmd) -> string`. Explorer's figure is Win32_LogicalDisk.FreeSpace. */
export function windowsAdapter({ ssh, dyno = 'C:\\Program Files\\Dyno\\dyno.exe', tmp = 'C:\\Users\\paddock-test\\AppData\\Local\\Temp\\vmsmoke-1gb.bin' }) {
  const ps = (c) => ({ code: 0, stdout: ssh(c, { ms: 280_000 }) });
  return {
    volumeName: undefined, preferNames: ['C:\\', 'C:', 'C:/'],
    writeOkPattern: /WROTE/,
    statusJsonCmd: `& '${dyno}' status --json 2>$null`, statusTextCmd: `& '${dyno}' status 2>$null`,
    writeCmd: `$f=[IO.File]::Create('${tmp}'); $b=New-Object byte[] 1000000; (New-Object Random).NextBytes($b); for($i=0;$i -lt 1000;$i++){$f.Write($b,0,$b.Length)}; $f.Flush($true); $f.Close(); 'WROTE'`,
    deleteCmd: `Remove-Item '${tmp}' -Force -ErrorAction SilentlyContinue`,
    // Explorer's figure is the volume's free bytes (GetDiskFreeSpaceEx = DriveInfo). CIM, Get-Volume and Get-PSDrive answer a standard user with Access denied or nothing on this guest.
    async osFree(exec) { return { bytes: num((await exec(`([IO.DriveInfo]'C').AvailableFreeSpace`)).stdout), source: 'Explorer (GetDiskFreeSpaceEx C:)' }; },
    async osAlt(exec) { return { bytes: num((await exec(`([IO.DriveInfo]'C').TotalFreeSpace`)).stdout), source: 'DriveInfo.TotalFreeSpace C:' }; },
  };
}
