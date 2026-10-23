<div align="center">

# Sighter

**Vision-based UI automation for apps with or without accessibility: record, label, train, and let your tests find any element.**

Sighter finds a button, a field or a piece of text on screen by sight, so legacy, custom-drawn and remote UIs can be
tested like any other. Everything runs on your machine.

</div>

![Sighter: Find any UI element by sight](docs/images/sighter-hero.svg?v=c99b3b41)


> **Status: in design; not usable yet.** There is no product code in this repository. The design is in
> [docs/proposal.md](docs/proposal.md) and open questions are pending. Building starts after Debrief ships.

## What it will do

![Sighter features](docs/images/sighter-features.svg?v=254b7ef8)

![How Sighter works](docs/images/sighter-how-it-works.svg?v=b8cce25d)

- **Find elements from pixels alone.** Send a screenshot (or ask for a window capture) and a query ("the Save button",
  text "Export"); get boxes, confidence and which layer answered. Accessibility trees are an optional label source,
  never required at run time.
- **Record** windows or screens plus input events, in the studio or headlessly on test machines.
- **Label and review** in a studio: suggestions from OCR, template matching, a detector and a vision model, with a
  review queue for everything not identified automatically.
- **Train** a small detector per app from your own reviewed labels, versioned and immutable (ONNX, permissive
  licences only).
- **Serve** a loopback-only API (`/v1/locate` and friends) with a layered resolver (cache, template, OCR, detector,
  vision model, "needs labelling") that your tests call from scripts, generated clients or the CLI.
- **Stay safe.** It never clicks blindly, redacts recordings before they are stored, and sends nothing off the machine.

## How it fits the family

Sighter is the sixth app of the Paddock family. [Pitwall](https://dev.decryptology.net/decryp7/pitwall) supplies the
local vision model, [Debrief](https://dev.decryptology.net/decryp7/debrief) opens the run bundles (screenshots, overlays,
results) for analysis, and [Pitcrew](https://dev.decryptology.net/decryp7/pitcrew) runs the tests on CI runners that have
Sighter as a service. See the
[family overview](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/ecosystem.md).

## Documentation

- [Design proposal](docs/proposal.md) (status: proposal, open questions pending)
- [Lessons from the suite](docs/suite-lessons.md)
- [Engineering rules](AGENTS.md)

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
