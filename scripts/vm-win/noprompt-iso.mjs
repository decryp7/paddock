// Makes a Windows install ISO boot without "Press any key to boot from CD or DVD", so the test VM installs headless
// (docs/testing/windows-test-vm.md). Local only, never CI.
//
//   node scripts/vm-win/noprompt-iso.mjs <iso to patch> <efisys.bin> <efisys_noprompt.bin>
//
// Microsoft's media carry two EFI boot images of the same size: efi/microsoft/boot/efisys.bin (asks for a key) and
// efisys_noprompt.bin (does not). The El Torito boot catalog points at efisys.bin. Rebuilding the ISO (xorriso, oscdimg)
// would rewrite the UDF file system Windows Setup reads; instead this overwrites the boot image's bytes in place, in a
// copy the caller made (prepare.sh clones the original with `cp -c`). It refuses unless the bytes at the catalog's EFI
// entry are exactly efisys.bin and the two images have the same size, so the UDF tree, the volume label and every other
// byte stay as they are. Already patched: no change.
import { closeSync, openSync, readFileSync, readSync, writeSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const SECTOR = 2048;
const EFI = 0xef;

/** Returns the start sectors of every bootable EFI entry in the El Torito boot catalog. `read(offset, length)` reads the image. */
export function efiBootImages(read) {
  const brvd = read(17 * SECTOR, SECTOR);
  if (brvd[0] !== 0 || brvd.toString('latin1', 1, 6) !== 'CD001' || !brvd.toString('latin1', 7, 30).startsWith('EL TORITO SPECIFICATION')) {
    throw new Error('no El Torito boot record at sector 17: not a bootable ISO');
  }
  const cat = read(brvd.readUInt32LE(0x47) * SECTOR, SECTOR);
  if (cat[0] !== 1 || cat[30] !== 0x55 || cat[31] !== 0xaa) throw new Error('the El Torito boot catalog has no valid validation entry');
  const found = [];
  const entry = (at, platform) => {
    if (platform === EFI && cat[at] === 0x88) found.push(cat.readUInt32LE(at + 8));
  };
  entry(32, cat[1]); // the default entry uses the validation entry's platform
  for (let at = 64; at + 32 <= SECTOR; ) {
    const header = cat[at];
    if (header !== 0x90 && header !== 0x91) break;
    const platform = cat[at + 1];
    const count = cat.readUInt16LE(at + 2);
    for (let k = 1; k <= count && at + 32 * k + 32 <= SECTOR; k++) entry(at + 32 * k, platform);
    at += 32 * (count + 1);
    if (header === 0x91) break; // the last section
  }
  return found;
}

/** Overwrites the catalog's EFI boot image (which must equal `prompt`) with `noprompt`. Returns 'patched' or 'already'. */
export function patchNoPrompt(fd, prompt, noprompt) {
  if (prompt.length !== noprompt.length) throw new Error(`boot images differ in size (${prompt.length} vs ${noprompt.length}); refusing`);
  const read = (offset, length) => {
    const b = Buffer.alloc(length);
    const n = readSync(fd, b, 0, length, offset);
    if (n !== length) throw new Error(`short read at ${offset}`);
    return b;
  };
  const sectors = efiBootImages(read);
  if (sectors.length !== 1) throw new Error(`expected one EFI boot entry, found ${sectors.length}`);
  const offset = sectors[0] * SECTOR;
  const current = read(offset, prompt.length);
  if (current.equals(noprompt)) return 'already';
  if (!current.equals(prompt)) throw new Error('the EFI boot image is neither efisys.bin nor efisys_noprompt.bin; refusing');
  writeSync(fd, noprompt, 0, noprompt.length, offset);
  if (!read(offset, noprompt.length).equals(noprompt)) throw new Error('verification after writing failed');
  return 'patched';
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [iso, promptPath, nopromptPath] = process.argv.slice(2);
  if (!iso || !promptPath || !nopromptPath) {
    console.error('usage: noprompt-iso.mjs <iso to patch> <efisys.bin> <efisys_noprompt.bin>');
    process.exit(2);
  }
  const fd = openSync(iso, 'r+');
  try {
    console.log(patchNoPrompt(fd, readFileSync(promptPath), readFileSync(nopromptPath)));
  } catch (e) {
    console.error(`noprompt-iso: ${e.message}`);
    process.exitCode = 1;
  } finally {
    closeSync(fd);
  }
}
