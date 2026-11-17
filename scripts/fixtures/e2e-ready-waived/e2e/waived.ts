// A read right after a bare visibility wait that carries a waiver (one waiver in this file).
declare const expect: (x: unknown) => { toBeVisible(): Promise<void> };
declare const options: { first(): unknown; allInnerTexts(): Promise<string[]> };
export async function spec(): Promise<void> {
  await expect(options.first()).toBeVisible();
  // conformance: e2e-ready — the fixture reads the list as it is on purpose
  const texts = await options.allInnerTexts();
  void texts;
}
