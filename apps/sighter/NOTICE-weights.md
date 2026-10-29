# Notices for model weights

Sighter's detector starts from the RF-DETR COCO checkpoints Nano, Small, Medium and Large (`rf-detr-large-2026.pth`),
mirrored unchanged on this project's Gitea (package `sighter-weights` 1.11.2).

- Copyright (c) 2025 Roboflow. Licensed under the Apache License, Version 2.0 (text: [LICENSE-APACHE](LICENSE-APACHE)).
- Source: https://github.com/roboflow/rf-detr (rfdetr 1.11.2). Roboflow's README states that RF-DETR Nano, Small,
  Medium and Large are released under Apache 2.0.
- Lineage: DINOv2 (Apache-2.0), then Objects365 pre-training (no licence stated; accepted by the user on 2026-10-29,
  ADR 0001 decision 1), then COCO (Roboflow checkpoint, Apache-2.0).
- The XL and 2XL sizes (PML 1.0) are not used and not mirrored.

Every model fine-tuned from one of these checkpoints carries this notice and lineage in its model card.
