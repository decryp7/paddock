<div align="center">

<img src="../../icons/alternative/scrutineer-placeholder/scrutineer-placeholder-128.svg?v=2d2ae2d2" alt="Scrutineer placeholder icon: an inspection stamp standing on a certificate, beside its tick mark, on a berry tile" width="96" height="96">

# Scrutineer

**A desktop app and command line for code-signing certificates: generate, sign, verify and install them in one place.**

In motorsport, scrutineering is the technical inspection that certifies a car before it may race. Scrutineer does that
for what you ship: it makes the certificates, signs builds and installers with them, checks signatures and their chains,
and installs certificates where the system and your builds will trust them.

</div>

![Placeholder hero for Scrutineer: certified before it ships. A concept mock-up of a certificate list with release, nightly and test-root certificates, their states, and the chain from the test root to the release certificate](docs/images/scrutineer-placeholder-hero.svg?v=1a51e73b)

> **Status: planned, not released.** There is no code yet. Scrutineer was dropped on 2026-11-05 and revived on 2026-11-09: one
> family signing identity keeps macOS privacy grants (Full Disk Access and others) and keychain access across updates, which
> ad-hoc builds lose every time. It is a family app on the shared shell, with the same functions as a command line for CI.
>
> **This is a placeholder page.** Its icon, hero and other images are placeholders (their file names say `placeholder`); the
> real icon, images and page come later. The screens are mock-ups, not product screenshots, and the features below are the
> app's purpose, not decisions: the design is the proposed
> [ADR 0079](../../docs/adr/0079-scrutineer-one-family-signing-identity.md).

## What it will do

![Placeholder feature panels for Scrutineer: generate a certificate and its key; sign builds and installers; verify a signature and its chain; install a certificate where it will be trusted](docs/images/scrutineer-placeholder-features.svg?v=65204de8)

- **Generate** a certificate and its key, with the uses code signing needs.
- **Sign** builds and installers with the certificate you choose.
- **Verify** a signature and its chain, and say why one fails.
- **Install** a certificate where the system and your builds will trust it.

![Placeholder diagram of how Scrutineer works: generate, sign, verify, install](docs/images/scrutineer-placeholder-how-it-works.svg?v=732a3682)

## Part of Paddock

Scrutineer is one of the [Paddock apps](../../docs/ecosystem.md). Its placeholder page lives with the family's
[tools](../README.md) until it has code under `apps/scrutineer`.
