"""Export RF-DETR Nano and Small (COCO weights) to ONNX at native size and at 640 px, for bench.py.

Research harness for docs/research/2026-10-29-rf-detr.md (rule 48), not product code. Needs a throwaway venv with
`rfdetr[onnx]==1.11.2`. Usage: `RF_HOME=<weights dir> nice -n 19 python -I export.py <out dir>`.
"""
import os
import sys
from pathlib import Path
import rfdetr
out = Path(sys.argv[1])
variants = {"nano": rfdetr.RFDETRNano, "small": rfdetr.RFDETRSmall}
for name, cls in variants.items():
    for shape in (None, (640, 640)):
        tag = f"{name}-{'native' if shape is None else '640'}"
        d = out / tag
        if any(d.glob("*.onnx")):
            continue
        m = cls()
        p = m.export(output_dir=str(d), shape=shape, verbose=False)
        print(tag, p, os.path.getsize(p))
