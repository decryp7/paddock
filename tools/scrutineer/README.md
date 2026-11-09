<div align="center">

<img src="../../icons/alternative/scrutineer-placeholder/scrutineer-placeholder-128.svg?v=2d2ae2d2" alt="Scrutineer placeholder icon: an inspection stamp standing on a certificate, beside its tick mark, on a berry tile" width="96" height="96">

# Scrutineer

**A desktop app and command line for code-signing certificates: generate, sign, verify and install them in one place.**

In motorsport, scrutineering is the technical inspection that certifies a car before it may race. Scrutineer does that
for what you ship: it makes the certificates, signs builds and installers with them, checks signatures and their chains,
and installs certificates where the system and your builds will trust them.

</div>

![Placeholder hero for Scrutineer: certified before it ships. A concept mock-up of the certificate authority screen: the family root, intermediate and code-signing leaf with their expiry, the other keys, the chain, and a reminder to renew the leaf](docs/images/scrutineer-placeholder-hero.svg?v=814cbf00)

> **Status: planned, not released.** There is no code yet. Scrutineer was dropped on 2026-11-05 and revived on 2026-11-09: one
> family signing identity keeps macOS privacy grants (Full Disk Access and others) and keychain access across updates, which
> ad-hoc builds lose every time. It is a family app on the shared shell, with the same functions as a command line for CI.
>
> **This is a placeholder page.** Its icon, hero and other images are placeholders (their file names say `placeholder`); the
> real icon, images and page come later. The screens are mock-ups, not product screenshots, and the features below are the
> app's purpose, not decisions: the design is the proposed
> [ADR 0079](../../docs/adr/0079-scrutineer-one-family-signing-identity.md).

## What it will do

![Placeholder feature panels for Scrutineer: rotate the intermediate step by step with what is kept; drop a file to see its verify report; install the family root after typing the first 8 hex digits of its fingerprint; the doctor checklist with a leaf to renew](docs/images/scrutineer-placeholder-features.svg?v=ea5a473e)

- **Certificate authority:** the family root, intermediate and code-signing leaf, the package and update keys, and when
  each expires.
- **Generate and rotate** them, with each step and its cost shown before it acts.
- **Sign and verify** builds and installers on macOS, Windows and Linux; drop any file to see who signed it, its chain and
  why a check fails.
- **Trust, per OS:** install or remove the family root for code signing only, after you type the first 8 hex digits of its
  fingerprint from your own record.
- **Doctor, expiry reminders and key backup:** what is installed where, what expires next, and a paper backup of the keys.
- **The same functions as a command line** for CI, where it also gates every release.

![Placeholder diagram of how Scrutineer works: generate, sign, verify, install](docs/images/scrutineer-placeholder-how-it-works.svg?v=ce92b1e3)

## Part of Paddock

Scrutineer is one of the [Paddock apps](../../docs/ecosystem.md). Its placeholder page lives with the family's
[tools](../README.md) until it has code under `apps/scrutineer`.
