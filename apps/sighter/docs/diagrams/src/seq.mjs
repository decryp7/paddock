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

seq('uml-seq-agent-step', {
  title: 'One agent-mode step', desc: 'The agent runner observes through the MCP server, Pitwall picks one tool call, the safety guard checks it, the target performs it and the runner verifies the result.',
  parts: [['rn', 'Agent runner', 'sighter do', 'accent'], ['ms', 'MCP server', 'tools, resolver', 'accent'], ['pw', 'Pitwall', 'optional', 'external'], ['sf', 'Safety guard', 'in the server', 'accent'], ['tg', 'Target adapter', 'Local, RDP', 'accent'], ['pc', 'PC under test', 'any app', 'external']],
  script: [
    ['o', 'loop', '[until the goal is met, a budget runs out or the user stops the run]'],
    ['m', 'rn', 'ms', '`observe()`'],
    ['m', 'ms', 'tg', '`frames()`'], ['m', 'tg', 'ms', 'Screenshot', 1],
    ['s', 'ms', 'Detect and label elements with boxes'],
    ['m', 'ms', 'rn', 'Screenshot and elements `#1 Save`, `#2 Cancel`…', 1],
    ['m', 'rn', 'pw', 'Goal, history, tool definitions, observation'],
    ['m', 'pw', 'rn', 'One tool call, e.g. `click(element: 1)`', 1],
    ['s', 'rn', 'Parse; drop calls outside the tool set'],
    ['m', 'rn', 'sf', '`click(element: 1)`'],
    ['s', 'sf', 'Allow-listed window? Budget left? Stop key not pressed?'],
    ['o', 'opt', '[destructive: send, delete, pay, install]'],
    ['m', 'sf', 'rn', 'Confirmation required: the user approves or refuses', 1],
    ['c'],
    ['o', 'alt', '[allowed]'],
    ['m', 'sf', 'tg', '`input(click at box centre)`'],
    ['m', 'tg', 'pc', 'Mouse click'],
    ['m', 'rn', 'ms', 'Verify: `observe()` and compare'], ['m', 'ms', 'rn', 'Screen changed as expected', 1],
    ['e', '[refused]'],
    ['m', 'sf', 'rn', 'Refused with a reason; Pitwall chooses again', 1],
    ['c'],
    ['s', 'sf', 'Every observation and action is written to the replay log'],
    ['c'],
  ],
});

seq('uml-seq-repro-from-video', {
  title: 'Reproduce a bug from a Debrief video', desc: 'Debrief, acting as an MCP host, turns a recording into candidate steps, drives them on a remote PC through Sighter with Pitwall choosing tools, watches the logs for the error and saves a plan that later runs without an LLM.',
  parts: [['db', 'Debrief', 'MCP host', 'external'], ['pw', 'Pitwall', 'local model', 'external'], ['ms', 'Sighter MCP server', 'tools, safety', 'accent'], ['tg', 'RDP target', 'embedded client', 'accent'], ['pc', 'PC under test', 'remote, logs', 'external']],
  script: [
    ['s', 'db', 'Pick key frames from the video; align them to the logs'],
    ['m', 'db', 'ms', '`observe_image(frame)` on each key frame (no input)'],
    ['m', 'ms', 'db', 'Labelled elements: what was clicked and typed into', 1],
    ['m', 'db', 'pw', 'Describe the frames; propose candidate steps'],
    ['m', 'pw', 'db', 'Ordered candidate steps', 1],
    ['s', 'db', 'The user confirms the run on this remote PC'],
    ['m', 'db', 'ms', 'Select the RDP target of the PC under test'],
    ['m', 'ms', 'tg', 'Connect; fixed resolution'], ['m', 'tg', 'pc', 'RDP session'],
    ['o', 'loop', '[each candidate step, inside the step and time budget]'],
    ['m', 'db', 'ms', '`observe()`'], ['m', 'ms', 'db', 'Screenshot and labelled elements', 1],
    ['m', 'db', 'pw', 'Step, observation, tool definitions'], ['m', 'pw', 'db', 'One tool call', 1],
    ['m', 'db', 'ms', 'Tool call; the safety guard checks it'],
    ['m', 'ms', 'tg', '`input(…)`'], ['m', 'tg', 'pc', 'Mouse or keyboard'],
    ['m', 'pc', 'db', 'New log lines (live or watched folder)', 1],
    ['s', 'db', 'Same error signature or known issue?'],
    ['c'],
    ['o', 'alt', '[the failure reproduces]'],
    ['m', 'db', 'ms', 'Save the executed steps as `.sighterplan`'],
    ['n', 'ms', 'The saved plan later runs with no LLM.', 300],
    ['e', '[the budget runs out]'],
    ['s', 'db', 'Report: not reproduced, with the replay log'],
    ['c'],
  ],
});
