import { P, save, sequence } from './common.mjs';

/** parts: [id, name, tech, role]; script items: ['m', a, b, text, reply?] ['s', p, text] ['o', kind, cond] ['e', cond] ['c'] ['n', p, text, w?] */
function seq(file, { title, desc, parts, script, pw = 168, gap = 48 }) {
  const w = 96 + parts.length * pw + (parts.length - 1) * gap;
  const d = P({ title, kind: 'UML · Sequence diagram · planned', desc, w });
  const s = sequence(d, parts.map(([id, name, tech, role]) => ({ id, name, tech, role })), { pw, gap });
  for (const [k, ...a] of script) {
    if (k === 'm') s.msg(a[0], a[1], a[2], { reply: !!a[3] });
    else if (k === 's') s.self(a[0], a[1]);
    else if (k === 'o') s.open(a[0], a[1]);
    else if (k === 'e') s.else(a[0]);
    else if (k === 'c') s.close();
    else if (k === 'n') s.note(a[0], a[1], a[2]);
  }
  const y = s.done();
  const names = { ui: 'Studio', accent: 'Sighter', core: 'Core library', process: 'Test code · process', external: 'External' };
  d.legend([...[...new Set(parts.map((p) => p[3]))].map((k) => ({ kind: k, label: names[k] })), { kind: 'line', label: 'Call' }, { kind: 'dashline', label: 'Reply' }]);
  save(file, d, y);
}

seq('uml-seq-locate', {
  title: 'Locate and click an element', desc: 'A C# test clicks the Save button through a locator. The client waits until the target is actionable, checks that exactly one element matches, then clicks through the guarded input driver.',
  parts: [['t', 'C# test', 'NUnit', 'process'], ['cl', 'Client library', 'locators', 'accent'], ['api', 'Locator API', 'HTTP, loopback', 'accent'], ['rs', 'Resolver', 'layers', 'accent'], ['pw', 'Pitwall', 'optional', 'external'], ['dr', 'Input driver', 'guarded', 'accent'], ['ap', 'App under test', 'any UI', 'external']],
  script: [
    ['m', 't', 'cl', '`await win.GetByText("Save").ClickAsync()`'],
    ['s', 'cl', 'The locator is lazy: it resolves now, and again on every action'],
    ['o', 'loop', '[auto-wait until the target is found, visible, stable and enabled, or the timeout]'],
    ['m', 'cl', 'api', '`POST /v1/locate { query, model: "editor@3", minConfidence }`'],
    ['m', 'api', 'ap', 'Capture the window'], ['m', 'ap', 'api', 'Frame', 1],
    ['m', 'api', 'rs', '`resolve(frame, query)`'],
    ['s', 'rs', 'cache → template → OCR anchor → detector; stop at the first confident answer'],
    ['o', 'opt', '[no layer is confident and Pitwall grounding is allowed]'],
    ['m', 'rs', 'pw', 'Where is “Save” in this frame?'], ['m', 'pw', 'rs', 'Box in the pixels of the sent image', 1],
    ['c'],
    ['m', 'rs', 'api', 'Matches with confidence and `resolvedBy`', 1],
    ['m', 'api', 'cl', '`200 { matches, modelVersion }`', 1],
    ['s', 'cl', 'Visible? Box stable? Enabled? Not covered?'],
    ['c'],
    ['o', 'alt', '[exactly one match, and it is actionable]'],
    ['m', 'cl', 'api', '`POST /v1/act { click, element }`'],
    ['m', 'api', 'dr', '`click(box centre)`'],
    ['s', 'dr', 'Check act permission, window scope, deny-list and confidence'],
    ['m', 'dr', 'ap', 'Mouse click'],
    ['m', 'cl', 'api', 'Verify: the screen changed'], ['m', 'api', 'cl', 'Confirmed', 1],
    ['m', 'cl', 't', 'Returns', 1],
    ['e', '[no match, several matches, a check fails or the timeout passes]'],
    ['m', 'cl', 't', 'Throws `NeedsLabelling`, `Ambiguous` or `ActionRefused`, with screenshot and candidates', 1],
    ['c'],
  ],
});

seq('uml-seq-recovery', {
  title: 'Recover a stuck step', desc: 'A step cannot find its screen. Pitwall sees the screenshot and proposes a plan from a closed vocabulary; guarded actions run and are verified. Without Pitwall the step fails with a screenshot.',
  parts: [['rn', 'Test runner', 'sighter run', 'accent'], ['rs', 'Resolver', 'layers', 'accent'], ['pw', 'Pitwall', 'vision, optional', 'external'], ['dr', 'Input driver', 'guarded', 'accent'], ['ap', 'App under test', 'any UI', 'external'], ['st', 'Studio', 'review', 'ui']],
  script: [
    ['m', 'rn', 'rs', 'Resolve the step’s target and match the expected screen'],
    ['m', 'rs', 'rn', 'No match within the step’s wait', 1],
    ['s', 'rn', 'Stuck: the expected screen does not match'],
    ['o', 'alt', '[Pitwall is available and `act` is enabled for the run]'],
    ['m', 'rn', 'pw', 'Screenshot, step intent, expected screen, last steps'],
    ['m', 'pw', 'rn', 'Diagnosis and plan from the closed vocabulary', 1],
    ['s', 'rn', 'Parse; drop actions outside the vocabulary'],
    ['o', 'loop', '[each planned action; at most 3 attempts inside the time budget]'],
    ['m', 'rn', 'dr', '`dismiss dialog via “Sign in again”`'],
    ['s', 'dr', 'Act permission, window scope, deny-list, confidence'],
    ['m', 'dr', 'ap', 'Click the named button'],
    ['m', 'rn', 'rs', 'Verify: does the expected screen match now?'], ['m', 'rs', 'rn', 'Match', 1],
    ['c'],
    ['s', 'rn', 'Retry the original step: it passes'],
    ['s', 'rn', 'Result: passed with recovery (warning); diagnosis and before/after kept'],
    ['m', 'rn', 'st', 'Proposal: add a “handle session dialog” sub-flow'],
    ['m', 'st', 'rn', 'Accepted or rejected by the user; never silent', 1],
    ['e', '[Pitwall is not available, or recovery is off]'],
    ['s', 'rn', 'Fail the step with a screenshot, the candidates and the review-queue link'],
    ['m', 'rn', 'st', 'Flag for labelling'],
    ['c'],
  ],
});

seq('uml-seq-hybrid', {
  title: 'Alongside FlaUI: UIA first, vision where UIA cannot see', desc: 'The test keeps FlaUI for everything UIA can read. FirstOf tries the UIA query and falls back to a scoped Sighter locator for a custom-drawn control.',
  parts: [['t', 'C# test', 'FlaUI + Sighter', 'process'], ['fl', 'FlaUI', 'UIA', 'process'], ['sc', 'Sighter client', 'FirstOf, locators', 'accent'], ['api', 'Locator API', 'HTTP, loopback', 'accent'], ['rs', 'Resolver', 'OCR + detector', 'accent'], ['in', 'Shared input', 'one coordinate space', 'accent'], ['ap', 'App under test', 'custom-drawn UI', 'external']],
  script: [
    ['m', 't', 'fl', '`Application.Launch(path)`, `app.GetMainWindow(automation)`'],
    ['m', 'fl', 'ap', 'Start and attach'],
    ['m', 'fl', 't', 'Window, found through UIA', 1],
    ['m', 't', 'sc', '`await FirstOf(win.FindFirstDescendant(cf => cf.ByAutomationId("Save")), win.Sighter().GetByText("Save")).ClickAsync()`'],
    ['m', 'sc', 'fl', '`FindFirstDescendant(ByAutomationId("Save"))`'],
    ['o', 'alt', '[UIA finds it]'],
    ['m', 'fl', 'sc', 'Element', 1],
    ['m', 'sc', 'fl', '`Click()` through FlaUI; Sighter is not called'],
    ['m', 'sc', 't', 'Returns, logged “answered by UIA”', 1],
    ['e', '[UIA returns nothing: a custom-drawn control]'],
    ['m', 'fl', 'sc', 'Nothing found', 1],
    ['s', 'sc', 'Fall back to the locator, scoped to the window’s `BoundingRectangle`'],
    ['o', 'loop', '[auto-wait until found, visible, stable and enabled]'],
    ['m', 'sc', 'api', '`POST /v1/locate { query, region }`'],
    ['m', 'api', 'rs', '`resolve(frame, query, region)`'],
    ['s', 'rs', 'OCR anchor, then detector'],
    ['m', 'rs', 'api', 'Matches, confidence, `ResolvedBy`', 1],
    ['m', 'api', 'sc', '`200 { matches }`', 1],
    ['c'],
    ['s', 'sc', 'Strict check: exactly one match'],
    ['m', 'sc', 'in', '`click(centre)`; DPI conversion in one helper'],
    ['m', 'in', 'ap', 'Mouse click, same coordinates as FlaUI'],
    ['m', 'sc', 'api', 'Verify: the screen changed'], ['m', 'api', 'sc', 'Confirmed', 1],
    ['s', 'sc', 'Log: “Save: answered by vision (detector 0.96)”'],
    ['m', 'sc', 't', 'Returns', 1],
    ['c'],
    ['o', 'opt', '[the test uses a second application]'],
    ['m', 't', 'sc', '`await app2.ActivateAsync()`'],
    ['m', 'sc', 'ap', 'Bring app2 to the front; focus is verified'],
    ['m', 't', 'sc', '`app2.MainWindow.GetByText("Save")` searches with app2’s model'],
    ['c'],
    ['n', 'sc', 'The same pattern works with pywinauto, Appium and Robot Framework.', 520],
  ],
});
