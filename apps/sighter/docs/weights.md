# Model weights mirror

Work package SW9 (plan item P1 of [ADR 0001](adr/0001-object-detector.md)). Written 2026-10-30.

## What is mirrored

The four Apache-2.0 RF-DETR COCO checkpoints named by ADR 0001 decision 6. The D-FINE fallback is not mirrored: the ADR
catalogues only these four, and D-FINE checkpoints are fetched only if that fallback is ever built. RT-DETRv4 is not
mirrored (licence review P4 pending). Upstream URLs were read on 2026-10-29 from `rfdetr/assets/model_weights.py` in the
`rfdetr` 1.11.2 wheel (PyPI, released 2026-10-04; wheel SHA-256 `2f2358956712...b103b2a`).

| File | Size (bytes) | SHA-256 (ours) | Upstream MD5 (verified) | Upstream URL |
|---|---|---|---|---|
| `rf-detr-nano.pth` | 366287238 | `d8d6b9ee57d4d0ed2b1f305163624712a0532cb7bce0c747317984fc5457440d` | `fb6504cce7fbdc783f7a46991f07639f` | `https://storage.googleapis.com/rfdetr/nano_coco/checkpoint_best_regular.pth` |
| `rf-detr-small.pth` | 386045550 | `d81979a9213a2109345158ce9232668df4c1ae52e9b8db3f2ec0a8cbad959b33` | `fb37061c1af7bace359c91b723a8d5c1` | `https://storage.googleapis.com/rfdetr/small_coco/checkpoint_best_regular.pth` |
| `rf-detr-medium.pth` | 404992918 | `749ff6071828aaffac63e204c4f4135ed3d6cdae4d702e086c360edc3b5768c8` | `7223f764a87b863f02eb8d52bf0ce2ee` | `https://storage.googleapis.com/rfdetr/medium_coco/checkpoint_best_regular.pth` |
| `rf-detr-large-2026.pth` | 135954129 | `0f4e20e19a99c0f8a62b5685f57f6c8b5c371c59081feda6752a0561a79ccf38` | `5cb72153541cbcb9aa6efa26222acc75` | `https://storage.googleapis.com/rfdetr/rf-detr-large-2026.pth` |

Size note: the checkpoints are the training `.pth` files, not exported models, and their sizes do not follow model size.
Nano (366 MB), Small (386 MB) and Medium (405 MB) lie within 11 % of each other, and Large (136 MB) is a different, newer
upstream file (`rf-detr-large-2026.pth`) and smaller than all three. The sizes are the bytes of the mirrored files, read back
after upload; the reason for the pattern was not investigated and is not assumed. The catalogue records them so a truncated
download is refused.

Total about 1.29 GB (the ADR estimated 1.5 GB; Large is smaller than estimated). Upstream publishes MD5 only; the
SHA-256s are ours, computed on 2026-10-29 after the MD5s matched.

## Licences

Apache-2.0 for code and weights: the repository `LICENSE` is Apache-2.0 and the README (checked 2026-10-29) lists
Nano, Small, Medium and Large as Apache 2.0 (XL, 2XL and the smaller A/F/P sizes are PML 1.0 and are not used).
Redistribution is permitted by Apache-2.0 section 4 given the licence text and notice, which ship in the package as
`LICENSE` and `NOTICE`. The Objects365 pre-training lineage states no licence; the user accepted it on 2026-10-29
(ADR decision 1). Notices for models built from these: [NOTICE-weights.md](../NOTICE-weights.md).

## Where they are

Gitea generic package `sighter-weights`, version `1.11.2` (the pinned `rfdetr` version; the ADR defines no other
model-set version), owner `decryp7`:
`https://dev.decryptology.net/api/packages/decryp7/generic/sighter-weights/1.11.2/<file>`, plus `LICENSE`, `NOTICE` and
`SHA256SUMS` in the same package. The catalogue is [`../catalogue/model-weights.toml`](../catalogue/model-weights.toml),
one entry per file with URL, size, SHA-256, licence and upstream URL. This is the single catalogue location: SW11's parser
reads this file in place and no second copy is made in `crates/`; its parser is the shared single-file asset kind
of SW11, so `dev fetch` and the app read it once that lands. Uploaded files were read back and re-hashed.

## How to refresh

A new checkpoint is a new package version, never an overwrite (ADR consequences): bump `rfdetr`, read the URLs and MD5s
from its `model_weights.py`, download only from `storage.googleapis.com/rfdetr` into a new folder, check the MD5s,
compute SHA-256s, treat the files as data (never load or execute them outside the training sandbox), re-check the
README licence column, then `curl --upload-file` each file with the Gitea token to
`.../generic/sighter-weights/<new version>/<file>` and add a new catalogue version. Never print the token.

## Checks

`scripts/weights/check-catalogue.mjs` (run by Sighter's CI leg through `paddock.ci.json`, tested by
`check-catalogue.test.mjs`) refuses the whole catalogue unless it parses, every entry has a 64-hex SHA-256 and a size above
0, every URL is `.../generic/sighter-weights/<version>/<file>` on the Gitea host, and every licence (and the file's own) is
Apache-2.0 or MIT. SW11's shared parser replaces it and keeps the same rules.

## Loading

A `.pth` file is a pickle container. Weights are loaded only in the training sandbox, with `torch.load(..., weights_only=True)`
(a restricted unpickler), after the SHA-256 matched the catalogue; an inference build never loads a `.pth` at all (it
runs exported ONNX). Recorded in [ADR 0001](adr/0001-object-detector.md) under the training worker.
