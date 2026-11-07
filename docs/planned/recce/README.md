<div align="center">

<img src="../../../icons/alternative/recce-placeholder/recce-placeholder-128.svg?v=a3c6f9f7" alt="Recce placeholder icon: a magnifier on a folded stage map, on a jade tile" width="96" height="96">

# Recce

**One search box for your whole PC: files, documents, email and SharePoint, by name or by meaning, with answers that cite their sources.**

In rallying, the recce is the reconnaissance run over a stage before the event, so you know where everything is. Recce
does that for your PC: it indexes what you keep, finds it as you type, and answers questions from your own documents
with a local model. Everything runs on your machine.

</div>

![Placeholder hero for Recce: know where everything is. A concept mock-up of the search window with mixed results from a PDF, a mail, SharePoint and a Word file, and an answer that cites two numbered sources](docs/images/recce-placeholder-hero.svg?v=89051053)

> **Status: planned, not released.** There is no product code yet.
>
> **This is a placeholder page.** Its icon, hero and other images are placeholders (their file names say `placeholder`); the
> real icon, images and page come later. The screens are mock-ups, not product screenshots.
>
> The page lives in `docs/planned/recce/` until the app gets its folder under `apps/`. The draft plan and its research are on
> the branch `docs/pc-search-app-plan` (`docs/plans/2026-11-04-pc-search-app.md`); its decisions are open. The app starts after
> the releases ahead of it in the family's order.

## What it will do

![Placeholder feature panels for Recce: one box for everything, search by meaning, answers with sources and a light launcher; works with Pitwall, which runs the local model, and Pacenotes, which shares the document readers](docs/images/recce-placeholder-features.svg?v=d73e6b82)

- **One search box** over file names, document content, email and attachments, and SharePoint and OneDrive.
- **Search by meaning:** finds the passage you mean even when it uses other words.
- **Answers with sources:** a local model (through Pitwall) answers from your own documents, and every claim links its source.
- **A light launcher:** apps, files, a calculator and actions from one hotkey.
- **MCP, later:** local AI tools get the same read-only view.

![Placeholder diagram of how Recce works: index names, content and meaning on your PC; search from one box as you type; answer with a local model and cited sources; later, serve the same view to local AI over MCP](docs/images/recce-placeholder-how-it-works.svg?v=e7ec2bd4)

## The plan in stages

Each stage is a useful release on its own; sizes and decisions are in the draft plan.

1. **Launcher and file name search:** the hotkey, apps, files by name, a calculator and actions.
2. **Content and meaning search:** document text and search by meaning, built on the family's shared readers and embeddings.
3. **Email and SharePoint/OneDrive:** mail with attachments, and Microsoft 365 files.
4. **Answers with sources, MCP, plugins and workflows.**

## Privacy

The index, the models and the answers stay on your PC. Logs record phases, counts and outcomes, never names, paths,
queries or text.

## Part of Paddock

Recce is one of the [Paddock apps](../../ecosystem.md), built on the shared shell and crates.
