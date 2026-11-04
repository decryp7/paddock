// Edits of the Windows test VM's .vmx that touch only what they own: the RemoteDisplay.vnc.* lines (add or strip) and, when the vmx names an
// NVMe redo log that no longer exists (a snapshot revert renumbers them), the name of the newest one that does. The current file is always the
// base: an old copy is never restored (it names a redo log that is gone and every revert then fails with "A file was not found").
const VNC = /^RemoteDisplay\.vnc\./;
const DISK = /^(nvme0:0\.fileName = ")(.*)(")$/m;
const REDO = /^paddock-win11-test-\d{6}\.vmdk$/;

export const stripVnc = (text) => text.split('\n').filter((l) => !VNC.test(l)).join('\n');

export const withVnc = (text, port, password) => `${stripVnc(text).trimEnd()}\nRemoteDisplay.vnc.enabled = "TRUE"\nRemoteDisplay.vnc.port = "${port}"\nRemoteDisplay.vnc.password = "${password}"\n`;

/** If the vmx names a disk file that is not in `existing` (file names), point it at the newest redo log that is (highest number). */
export function fixDiskName(text, existing) {
  const m = DISK.exec(text);
  if (!m || existing.includes(m[2])) return text;
  const newest = existing.filter((f) => REDO.test(f)).sort().at(-1);
  return newest ? text.replace(DISK, `$1${newest}$3`) : text;
}
