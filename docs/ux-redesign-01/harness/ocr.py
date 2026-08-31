#!/usr/bin/env python3
import subprocess
import sys
from pathlib import Path

EVIDENCE = Path(__file__).resolve().parent.parent


def run_tesseract(png: Path) -> Path:
    txt = png.with_suffix(".ocr.txt")
    # If a non-empty .ocr.txt already exists (e.g. QR dialog fallback), keep it.
    if txt.exists() and txt.stat().st_size > 0:
        print(f"ocr {png.name} -> {txt.name} (existing)")
        return txt
    # Spanish tessdata may not be installed; fall back to English (Latin script still works).
    for lang in ("spa", "eng"):
        result = subprocess.run(
            ["tesseract", str(png), "stdout", "-l", lang],
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            txt.write_text(result.stdout, encoding="utf-8")
            print(f"ocr {png.name} -> {txt.name} (lang={lang})")
            return txt
    print(f"tesseract failed for {png}: {result.stderr}", file=sys.stderr)
    txt.write_text("", encoding="utf-8")
    return txt


def main():
    pngs = sorted(EVIDENCE.glob("*.png"))
    if not pngs:
        print("no PNG files found in", EVIDENCE)
        sys.exit(1)
    empty = []
    for png in pngs:
        txt = run_tesseract(png)
        if not txt.exists() or txt.stat().st_size == 0:
            empty.append(png.name)
    if empty:
        print("OCR produced empty output for:", ", ".join(empty), file=sys.stderr)
        sys.exit(1)
    print(f"OCR complete for {len(pngs)} images")


if __name__ == "__main__":
    main()
