# The family plugin signing key

Built-in plugins of every Paddock app are `.paddockplugin` packages signed with **one family plugin key** (ADR 0001,
"Signing, trust and packaging"). This page says how the owner creates it and stores it as a CI secret. Nobody else
creates it, and agents never do.

## Why it is not the updater key

The family updater key (`PADDOCK_TAURI_SIGNING_PRIVATE_KEY`, ADR 0014's amendment, plan decision D2) is not used for
plugins:

- **The format differs.** The updater key is a password-protected minisign key that Tauri's updater reads.
  `paddock-plugin-package` signs and verifies plain Ed25519 keys in its own text form,
  `paddock-plugin-secret-key ed25519 <64 hex digits>` (`crates/paddock-plugin-package/src/keys.rs`).
- **One key per purpose.** If either key leaks, it must not be able to sign the other kind of artifact. The two keys also
  rotate differently: installed apps carry the updater key, while the plugin key is checked whenever a plugin is installed
  or updated.

## Create it (owner, once)

1. On your own machine (not a test VM, not CI), in a `paddock` checkout, build the tool and write the key **outside any
   repository**:

   ```sh
   cd crates
   cargo run --release -p cargo-paddock-plugin -- keygen --out ~/paddock-plugin-family.key
   ```

   The tool writes the secret half to a file only you can read and never overwrites an existing file. It prints the
   public key (`paddock-plugin-public-key ed25519 <64 hex digits>`) and the key id (16 hex digits). Write down both.
2. Back up the key file the same way as the updater key: in the password manager, plus an offline copy.
3. In `decryp7/paddock`, open **Settings > Actions > Secrets** and add a repository secret:

   | Secret | Value |
   | --- | --- |
   | `PADDOCK_PLUGIN_SIGNING_KEY` | the whole one-line content of `~/paddock-plugin-family.key` (`paddock-plugin-secret-key ed25519 …`) |

4. Delete `~/paddock-plugin-family.key` from the working machine once the backup is checked. The key now exists only in
   the backup and the CI secret.
5. Give the **public key** and key id to whoever wires the release signing (packages PH4 and R of the 1.2 plan). The
   public key is not secret: it is committed as each app's built-in key list (`TrustSettings` built-in keys).

## How CI uses it (wired by the release work, not yet)

The release workflow writes the secret to a temporary owner-only file, signs each built-in package with
`cargo-paddock-plugin sign <package> --key <file>`, deletes the file, and checks the result with
`cargo-paddock-plugin verify <package> --trust <public key>`. Branch and fork builds have no secret, so their packages
are unsigned and only run in developer mode, the same rule as the updater key.

## Lost or leaked

- **Lost:** create a new key and release every app with the new public key in its built-in list. Plugins signed with the
  old key still verify while the old public key stays in the list.
- **Leaked:** create a new key, remove the old public key from every app's built-in list in the same release, and re-sign
  all built-in packages. Until users update, the leaked key could sign a package that an older app treats as built-in.
