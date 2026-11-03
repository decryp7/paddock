import { dialogContracts, expectSizeToContent, SIZE_SCALES, test } from '@decryp7/paddock-e2e';

// ADR 0043 check 4: one fixture dialog per kind whose content arrives after the first measure; the page-side loop follows it
// at every scale (the page only: a browser's deviceScaleFactor does not reach Rust).
const CONTRACTS = dialogContracts('projects/shell-demo/src/app/generated/dialog-contracts.ts');

for (const id of ['sd.fixture-decision', 'sd.fixture-form', 'sd.fixture-viewer', 'sd.fixture-tool']) {
  for (const scale of SIZE_SCALES) {
    test(`${id} follows content that arrives late at ${scale}x: the last measure is the current one, at most two sends, a new epoch on reload`, async ({ browser, baseURL }) => {
      await expectSizeToContent(browser, baseURL as string, { id, contract: CONTRACTS[id] as never }, scale, process.env['DIALOG_SHOTS']);
    });
  }
}
