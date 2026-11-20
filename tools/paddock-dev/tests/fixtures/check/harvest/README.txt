How tests/fixtures/check/units/*.json and recorded/frozen-real-tree.json were made (pair P3, 2026-11-19). The JavaScript modules of the 12
checks exist only in commit 984fa453b and before, so these scripts run from a checkout (or `git archive`) of that commit, with this
repository's node_modules linked in:

  1. HARVEST_OUT=out.jsonl NODE_OPTIONS="--import file://<here>/register.mjs" \
       node --test conformance/test/c39-*.test.mjs ... (the 12 test files)       register.mjs + hooks.mjs wrap the detection
                                                                                 functions and runChecks and log every input
  2. node convert.mjs <checkout> units.json                                      one-file trees, the JS answer for each
  3. node freeze.mjs <current repo> <checkout at 984fa453b> list.json out.json   the frozen real-tree copy and its JS answer
  4. node redact.mjs real-tree.json                                              token-shaped test strings out of the frozen tree
                                                                                 (freeze.mjs applies the same redaction; added 2026-11-20
                                                                                 after gitleaks flagged heavy's invented tokens in CI run 8346)

The answers in `result` can be re-recorded from the Rust host with PADDOCK_GOLDEN_BLESS=1 (refused in CI): a reviewed edit, never by hand.
These scripts are kept so the inputs can be re-harvested; they are not run by any gate.
