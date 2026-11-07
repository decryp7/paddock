<div align="center">

<img src="../../icons/alternative/scrutineer-placeholder/scrutineer-placeholder-128.svg?v=2d2ae2d2" alt="Scrutineer placeholder icon: an inspection stamp standing on a certificate, beside its tick mark, on a berry tile" width="96" height="96">

# Scrutineer

**A tool for code-signing certificates that was dropped on 2026-11-05: it was to generate, sign, verify and install them in one place.**

In motorsport, scrutineering is the technical inspection that certifies a car before it may race. Scrutineer was to do that
for what the family ships: make the certificates, sign builds and installers with them, check signatures and their chains,
and install certificates where the system and the builds would trust them.

</div>

![Placeholder hero for Scrutineer: certified before it ships. A concept mock-up of a certificate list with release, nightly and test-root certificates, their states, and the chain from the test root to the release certificate](docs/images/scrutineer-placeholder-hero.svg?v=ddee5b1a)

> **Status: dropped on 2026-11-05.** There is no code, and the apps stay unsigned. Scrutineer was to be a tool of the family,
> not an app. This page and its placeholder images (their file names say `placeholder`) are kept as a record; the screens are
> mock-ups, not product screenshots.

## What it would have done

![Placeholder feature panels for Scrutineer: generate a certificate and its key; sign builds and installers; verify a signature and its chain; install a certificate where it will be trusted](docs/images/scrutineer-placeholder-features.svg?v=d5bebd19)

- **Generate** a certificate and its key, with the uses code signing needs.
- **Sign** builds and installers with the chosen certificate.
- **Verify** a signature and its chain, and say why one fails.
- **Install** a certificate where the system and the builds would trust it.

![Placeholder diagram of how Scrutineer works: generate, sign, verify, install](docs/images/scrutineer-placeholder-how-it-works.svg?v=732a3682)

## Part of Paddock

Scrutineer was planned as one of the family's [tools](../README.md), beside the [Paddock apps](../../docs/ecosystem.md), until it was dropped.
