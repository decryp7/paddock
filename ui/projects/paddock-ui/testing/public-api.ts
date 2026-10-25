/**
 * `@decryp7/paddock-ui/testing`: the DOM half of the component test kit (plan X38): `installTestKit()` for the app's
 * `test-setup.ts` (the jsdom gaps), DOM helpers (`must`, `at`, `text`, `one`, `all`, `byText`, `buttonByText`, `typeInto`,
 * `press`, `manualFrames`) and a fake ECharts. The TestBed half (`settle`, `render`, the shell fakes) is
 * `@decryp7/paddock-ui/shell/testing`.
 */
export { installTestKit, type TestKitOptions } from './src/setup';
export { all, at, buttonByText, byText, manualFrames, must, one, press, text, typeInto } from './src/dom';
export { FakeChart, FakeECharts } from './src/fake-chart';
export { assertStatusGlyphs, statusGlyphRenderProblems } from './src/status-glyphs';
