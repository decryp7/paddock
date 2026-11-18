import { expect, test } from './test';
import { pauseLive } from '@decryp7/paddock-e2e';
import { formatBytes, formatGhz, formatRate, formatStorage } from '../src/app/shared/format';
import {
  MAC_BUILD_CPU,
  MAC_BUILD_DISK_BPS,
  MAC_CARGO_CPU,
  MAC_CPU_CLOCK_MHZ,
  MAC_CPU_MAX_MHZ,
  MAC_CPU_TEMP_C,
  MAC_CPU_TOTAL,
  MAC_DISK_FREE,
  MAC_DISK_FREE_TEXT,
  MAC_DISK_IO_BPS,
  MAC_DISK_TOTAL,
  MAC_DISK_USED_PERCENT,
  MAC_DISK_WRITE_BPS,
  MAC_MEMORY_PERCENT,
  MAC_MEMORY_TOTAL,
  MAC_MEMORY_USED,
  MAC_WRITE_SHARE,
} from '../src/app/core/mock/mac-machine';
import { FROZEN, SHOT_AFTER_MS } from './doc-instant';

/**
 * Every fact the documentation pictures show more than once reads the same on every page, read from the rendered text the way a person reads it
 * (the class "mock facts drift across pictures": the startup disk, the CPU load, the build, the memory and the disk traffic differed from page to
 * page, found by reading the pictures). The mock's Mac declares each fact once (core/mock/mac-machine.ts); this spec opens each page at the
 * pictures' instant (doc-instant.ts), reads its text and asks it for the fact as the app's own formatters print it from the sheet. The unit spec
 * (core/mock-consistency.spec.ts) asks the replies; this one asks what the person sees.
 */
test.use({ pinnedClock: false, timezoneId: 'UTC', locale: 'en-GB', viewport: { width: 1440, height: 1500 } });

const MIB = 1024 ** 2;
const CARGO_DISK = 0.4 * MIB;
const rustcCpu = `${Math.round(MAC_BUILD_CPU - MAC_CARGO_CPU)}%`;
const rustcWrite = formatRate((MAC_BUILD_DISK_BPS - CARGO_DISK) * MAC_WRITE_SHARE);
const diskFull = `Macintosh HD is ${MAC_DISK_USED_PERCENT}% full`;

/** A page, how to reach it, and the sentences or figures its text has to contain. */
const PAGES: { name: string; hash: string; facts: Record<string, string> }[] = [
  {
    name: 'dashboard',
    hash: '/',
    facts: {
      'disk free and total (Health card)': MAC_DISK_FREE_TEXT,
      'disk used share': diskFull,
      'CPU total (tile)': `CPU\n${MAC_CPU_TOTAL}%`,
      'memory share (tile)': `Memory\n${MAC_MEMORY_PERCENT}%`,
      'disk traffic (tile)': `Disk\n${formatRate(MAC_DISK_IO_BPS)}`,
      'CPU total ("Is this normal?")': `CPU is ${MAC_CPU_TOTAL}% now`,
      'rustc CPU ("Is this normal?")': `CPU is ${rustcCpu} now`,
    },
  },
  { name: 'health', hash: '/health', facts: { 'disk free and total': MAC_DISK_FREE_TEXT, 'disk used share': diskFull } },
  {
    name: 'tray',
    hash: '/tray',
    facts: {
      'disk free and total': `${formatStorage(MAC_DISK_FREE)} free of ${formatStorage(MAC_DISK_TOTAL)}`,
      'CPU total': `${MAC_CPU_TOTAL}%`,
      'memory used and total': `Memory ${formatBytes(MAC_MEMORY_USED).replace(/ GB$/, '')} of ${formatBytes(MAC_MEMORY_TOTAL)}`,
      'memory share': `${MAC_MEMORY_PERCENT}%`,
    },
  },
  {
    name: 'why-slow',
    hash: '/why-slow',
    facts: {
      'rustc CPU': `rustc is using ${rustcCpu} CPU`,
      'CPU total': `CPU total\n${MAC_CPU_TOTAL}%`,
      'rustc writes': `${rustcWrite} by rustc`,
      'disk free and total': `${formatStorage(MAC_DISK_FREE)} of ${formatStorage(MAC_DISK_TOTAL)}`,
      'disk used share': diskFull,
    },
  },
  {
    name: 'processes',
    hash: '/processes',
    facts: {
      'CPU total and memory in use': `${MAC_CPU_TOTAL}% CPU · ${formatBytes(MAC_MEMORY_USED)} memory in use`,
      'build disk traffic': formatRate(MAC_BUILD_DISK_BPS),
      'build CPU': `\n${MAC_BUILD_CPU}%\n`,
    },
  },
  {
    name: 'inventory',
    hash: '/inventory',
    facts: { 'memory total': `${formatBytes(MAC_MEMORY_TOTAL)} LPDDR5`, 'disk size': formatStorage(MAC_DISK_TOTAL) },
  },
  {
    name: 'recorder',
    hash: '/recorder',
    facts: {
      'CPU total': `CPU ${MAC_CPU_TOTAL}.0%`,
      'memory used and total': `${formatBytes(MAC_MEMORY_USED).replace(/ GB$/, '')} of ${formatBytes(MAC_MEMORY_TOTAL)}`,
      'disk writes': `write ${formatRate(MAC_DISK_WRITE_BPS)}`,
    },
  },
  {
    name: 'throttling',
    hash: '/throttling',
    facts: {
      'CPU total (Load)': `Load\n${MAC_CPU_TOTAL}%`,
      'clock': `Clock\n${formatGhz(MAC_CPU_CLOCK_MHZ)} of ${formatGhz(MAC_CPU_MAX_MHZ)}`,
      'temperature': `Temperature\n${MAC_CPU_TEMP_C} °C`,
    },
  },
  { name: 'cpu', hash: '/cpu', facts: { 'CPU total and clock': `${MAC_CPU_TOTAL}% overall · ${formatGhz(MAC_CPU_CLOCK_MHZ)}` } },
  { name: 'disk-map', hash: '/disk-map', facts: { 'disk free and total': `of ${formatStorage(MAC_DISK_TOTAL)} · ${formatStorage(MAC_DISK_FREE)} free` } },
];

for (const { name, hash, facts } of PAGES) {
  test(`${name} says the declared figures`, async ({ page }) => {
    await page.clock.install({ time: FROZEN });
    await page.goto(`/?latency=0&stable=60000&chrome=macos&menu=native#${hash}`);
    await page.clock.runFor(SHOT_AFTER_MS - 10_000);
    await expect(page.locator('pd-shell-frame, dy-tray').first()).toBeVisible();
    // conformance: e2e-ready — reads the faked clock (Date.now), not the page
    const now = await page.evaluate(() => Date.now());
    await page.clock.runFor(FROZEN + SHOT_AFTER_MS - now);
    // The page does not draw while the clock is paused (pauseLive): wait for every figure to be drawn first, so the pause freezes a finished page
    // (the read right after the pause missed a figure in about 1 of 40 runs, with the shell drawn and the page body not yet).
    for (const [what, expected] of Object.entries(facts)) {
      await expect.poll(() => page.locator('body').innerText(), { message: `${name}: ${what} is drawn` }).toContain(expected);
    }
    await pauseLive(page);
    const text = await page.locator('body').innerText();
    for (const [what, expected] of Object.entries(facts)) {
      expect(text, `${name}: ${what}`).toContain(expected);
    }
  });
}
