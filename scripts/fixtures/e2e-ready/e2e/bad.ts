// A read right after a bare visibility wait (line 6).
declare const expect: (x: unknown) => { toBeVisible(): Promise<void> };
declare const options: { first(): unknown; allInnerTexts(): Promise<string[]> };
export async function spec(): Promise<void> {
  await expect(options.first()).toBeVisible();
  const texts = await options.allInnerTexts();
  void texts;
}
