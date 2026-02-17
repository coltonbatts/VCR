#!/usr/bin/env python3
"""Generate a labeled contact sheet for a VCR pack of image assets."""

from __future__ import annotations

import argparse
import json
import math
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    from PIL import Image, ImageDraw, ImageFont, ImageOps
except ImportError as exc:  # pragma: no cover - handled at runtime
    print(
        "Missing dependency: Pillow. Run scripts/pack_contact_sheet.sh or install via pip.",
        file=sys.stderr,
    )
    raise SystemExit(2) from exc


@dataclass
class PackItem:
    item_id: str
    path: Path
    source_path: str
    width: int
    height: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build a contact sheet from pack.json image items so artists/agents can pick "
            "asset IDs quickly."
        )
    )
    parser.add_argument(
        "--pack",
        required=True,
        help="Path to the pack directory (must contain pack.json).",
    )
    parser.add_argument(
        "--out",
        help="Output PNG path. Defaults to <pack>/contact_sheet.png.",
    )
    parser.add_argument(
        "--index-out",
        help=(
            "Optional TSV index output path. Defaults to "
            "<out>.index.tsv (or <pack>/contact_sheet.index.tsv)."
        ),
    )
    parser.add_argument("--cols", type=int, default=6, help="Grid columns (default: 6).")
    parser.add_argument(
        "--thumb",
        type=int,
        default=260,
        help="Thumbnail size per tile in pixels (default: 260).",
    )
    parser.add_argument(
        "--tile-padding",
        type=int,
        default=16,
        help="Padding between tiles (default: 16).",
    )
    parser.add_argument(
        "--inner-padding",
        type=int,
        default=12,
        help="Padding inside each thumbnail around the fitted image (default: 12).",
    )
    parser.add_argument(
        "--checker",
        type=int,
        default=20,
        help="Checker square size in px for alpha visualization (default: 20).",
    )
    return parser.parse_args()


def load_items(pack_dir: Path) -> tuple[str, list[PackItem]]:
    pack_json = pack_dir / "pack.json"
    if not pack_json.exists():
        raise FileNotFoundError(f"pack.json not found at {pack_json}")

    payload = json.loads(pack_json.read_text())
    pack_id = payload.get("pack_id", "<unknown-pack>")
    raw_items = payload.get("items", [])
    image_items: list[PackItem] = []

    for raw in raw_items:
        if raw.get("type") != "image":
            continue
        item_id = raw["id"]
        rel_path = Path(raw["path"])
        src = pack_dir / rel_path
        spec = raw.get("spec", {})
        width = int(spec.get("width", 0) or 0)
        height = int(spec.get("height", 0) or 0)
        image_items.append(
            PackItem(
                item_id=item_id,
                path=src,
                source_path=str(rel_path.as_posix()),
                width=width,
                height=height,
            )
        )

    image_items.sort(key=lambda item: item.item_id)
    if not image_items:
        raise ValueError(f"No image items found in {pack_json}")

    return pack_id, image_items


def draw_checkerboard(
    draw: ImageDraw.ImageDraw,
    x: int,
    y: int,
    w: int,
    h: int,
    square: int,
) -> None:
    c1 = (58, 58, 58, 255)
    c2 = (40, 40, 40, 255)
    for yy in range(y, y + h, square):
        for xx in range(x, x + w, square):
            use_c1 = ((xx - x) // square + (yy - y) // square) % 2 == 0
            draw.rectangle(
                [xx, yy, min(xx + square - 1, x + w), min(yy + square - 1, y + h)],
                fill=(c1 if use_c1 else c2),
            )


def choose_output_paths(
    pack_dir: Path, out: str | None, index_out: str | None
) -> tuple[Path, Path]:
    contact_path = Path(out) if out else pack_dir / "contact_sheet.png"
    if index_out:
        index_path = Path(index_out)
    else:
        if out:
            index_path = contact_path.with_suffix(contact_path.suffix + ".index.tsv")
        else:
            index_path = pack_dir / "contact_sheet.index.tsv"
    return contact_path, index_path


def render_contact_sheet(
    items: list[PackItem],
    out_path: Path,
    cols: int,
    thumb: int,
    tile_padding: int,
    inner_padding: int,
    checker: int,
) -> None:
    if cols <= 0:
        raise ValueError("--cols must be greater than 0")
    if thumb <= 0:
        raise ValueError("--thumb must be greater than 0")

    rows = math.ceil(len(items) / cols)
    label_h = 40
    sheet_w = tile_padding + cols * (thumb + tile_padding)
    sheet_h = tile_padding + rows * (thumb + label_h + tile_padding)
    sheet = Image.new("RGBA", (sheet_w, sheet_h), (20, 20, 20, 255))
    draw = ImageDraw.Draw(sheet)
    font = ImageFont.load_default()

    for i, item in enumerate(items):
        row = i // cols
        col = i % cols
        x = tile_padding + col * (thumb + tile_padding)
        y = tile_padding + row * (thumb + label_h + tile_padding)

        draw_checkerboard(draw, x, y, thumb, thumb, checker)

        if not item.path.exists():
            raise FileNotFoundError(f"Missing source image for '{item.item_id}': {item.path}")

        with Image.open(item.path).convert("RGBA") as img:
            actual_w, actual_h = img.size
            item.width = item.width or actual_w
            item.height = item.height or actual_h
            fitted = ImageOps.contain(
                img,
                (thumb - (inner_padding * 2), thumb - (inner_padding * 2)),
                Image.Resampling.LANCZOS,
            )

        px = x + (thumb - fitted.width) // 2
        py = y + (thumb - fitted.height) // 2
        sheet.alpha_composite(fitted, (px, py))
        draw.rectangle([x, y, x + thumb, y + thumb], outline=(130, 130, 130, 255), width=1)

        label = f"{item.item_id} {item.width}x{item.height}"
        draw.text((x + 4, y + thumb + 9), label, font=font, fill=(235, 235, 235, 255))

    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out_path)


def write_index(pack_id: str, items: list[PackItem], path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        f"# pack_id\t{pack_id}",
        "id\twidth\theight\tsource_path",
    ]
    for item in items:
        lines.append(
            f"{item.item_id}\t{item.width}\t{item.height}\t{item.source_path}"
        )
    path.write_text("\n".join(lines) + "\n")


def print_summary(pack_id: str, items: list[PackItem], out: Path, index_out: Path) -> None:
    print(f"Pack: {pack_id}")
    print(f"Items: {len(items)}")
    print(f"Contact sheet: {out}")
    print(f"Index TSV: {index_out}")
    print("")
    for item in items:
        print(f"{item.item_id:10s} {item.width:4d}x{item.height:<4d}")


def main() -> int:
    args = parse_args()
    pack_dir = Path(args.pack).resolve()
    out_path, index_path = choose_output_paths(pack_dir, args.out, args.index_out)
    pack_id, items = load_items(pack_dir)
    render_contact_sheet(
        items=items,
        out_path=out_path,
        cols=args.cols,
        thumb=args.thumb,
        tile_padding=args.tile_padding,
        inner_padding=args.inner_padding,
        checker=args.checker,
    )
    write_index(pack_id, items, index_path)
    print_summary(pack_id, items, out_path, index_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
