#!/usr/bin/env python3
"""Extract individual objects from tileset atlases.

Uses pixel-level boundary analysis: within a tile, all opaque content is one
unit. Between adjacent tiles, connection requires opaque pixels to physically
touch across the tile seam (same position on both sides of the boundary).

Usage:
    python extract_sprites.py [--debug] [--min-touch N] [path.tsx ...]

If no paths given, processes all .tsx files in assets/tilesets/.
Output: assets/objects/<tileset_name>/<tileset_name>_obj<N>.png

Requires: pillow, numpy
"""

import argparse
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

import numpy as np
from PIL import Image

ALPHA_THRESH = 32     # pixels below this alpha are "transparent"
MIN_TOUCH = 1         # minimum pixels that must touch across a seam to connect
MIN_OPAQUE_FRAC = 0.05  # skip tiles with less than this fraction opaque


def parse_tsx(tsx_path: Path):
    """Parse a TSX file and return tileset info."""
    tree = ET.parse(tsx_path)
    root = tree.getroot()
    name = root.get("name")
    tile_w = int(root.get("tilewidth"))
    tile_h = int(root.get("tileheight"))
    columns = int(root.get("columns"))
    tilecount = int(root.get("tilecount"))
    img_elem = root.find("image")
    img_source = img_elem.get("source")
    img_path = (tsx_path.parent / img_source).resolve()
    return name, tile_w, tile_h, columns, tilecount, img_path


def tile_has_content(atlas, tile_w, tile_h, cols, tile_idx):
    """Check if a tile has meaningful opaque content."""
    ac = tile_idx % cols
    ar = tile_idx // cols
    x0 = ac * tile_w
    y0 = ar * tile_h
    if y0 + tile_h > atlas.shape[0] or x0 + tile_w > atlas.shape[1]:
        return False
    tile_alpha = atlas[y0:y0 + tile_h, x0:x0 + tile_w, 3]
    opaque_count = np.sum(tile_alpha > ALPHA_THRESH)
    return opaque_count > tile_w * tile_h * MIN_OPAQUE_FRAC


def seam_touches_horizontal(atlas, tile_w, tile_h, cols, idx_left, idx_right):
    """Count how many pixels touch across a vertical seam between two tiles.

    Checks the rightmost column of the left tile against the leftmost column
    of the right tile. A pixel position "touches" if BOTH sides are opaque.
    """
    lc = idx_left % cols
    lr = idx_left // cols
    rc = idx_right % cols
    rr = idx_right // cols

    # Right column of left tile
    lx = lc * tile_w + tile_w - 1
    ly = lr * tile_h
    # Left column of right tile
    rx = rc * tile_w
    ry = rr * tile_h

    if (ly + tile_h > atlas.shape[0] or lx >= atlas.shape[1] or
            ry + tile_h > atlas.shape[0] or rx >= atlas.shape[1]):
        return 0

    left_col = atlas[ly:ly + tile_h, lx, 3] > ALPHA_THRESH
    right_col = atlas[ry:ry + tile_h, rx, 3] > ALPHA_THRESH
    return int(np.sum(left_col & right_col))


def seam_touches_vertical(atlas, tile_w, tile_h, cols, idx_top, idx_bottom):
    """Count how many pixels touch across a horizontal seam between two tiles.

    Checks the bottom row of the top tile against the top row of the bottom tile.
    """
    tc = idx_top % cols
    tr = idx_top // cols
    bc = idx_bottom % cols
    br = idx_bottom // cols

    # Bottom row of top tile
    tx = tc * tile_w
    ty = tr * tile_h + tile_h - 1
    # Top row of bottom tile
    bx = bc * tile_w
    by = br * tile_h

    if (ty >= atlas.shape[0] or tx + tile_w > atlas.shape[1] or
            by >= atlas.shape[0] or bx + tile_w > atlas.shape[1]):
        return 0

    top_row = atlas[ty, tx:tx + tile_w, 3] > ALPHA_THRESH
    bot_row = atlas[by, bx:bx + tile_w, 3] > ALPHA_THRESH
    return int(np.sum(top_row & bot_row))


# ── Debug visualization ──────────────────────────────────────────────────

REASON_STYLE = {
    "touch":      (0, 200, 0, 200),     # green  — pixels touch across seam
    "no_touch":   (255, 0, 0, 200),     # red    — no pixels touch
    "no_nb":      (50, 50, 50, 100),    # dark gray — no neighbor / empty
    "no_content": (30, 30, 30, 80),     # near-invisible — tile has no content
}


def draw_marker(img, x, y, w, h, color):
    """Draw a filled rectangle."""
    y1, y2 = max(0, y), min(img.shape[0], y + h)
    x1, x2 = max(0, x), min(img.shape[1], x + w)
    if y2 > y1 and x2 > x1:
        for c in range(min(4, len(color))):
            img[y1:y2, x1:x2, c] = color[c]


# ── Main extraction ─────────────────────────────────────────────────────

def extract_objects_from_atlas(atlas, tile_w, tile_h, cols, tilecount,
                                min_touch=MIN_TOUCH, debug=False):
    """Find connected tile groups and composite each into a sprite."""
    rows = (tilecount + cols - 1) // cols

    has_content = [False] * tilecount
    for i in range(tilecount):
        has_content[i] = tile_has_content(atlas, tile_w, tile_h, cols, i)

    # Pre-compute seam touch counts for all adjacent pairs
    h_touch = {}  # (left_idx, right_idx) → touch count
    v_touch = {}  # (top_idx, bottom_idx) → touch count

    for idx in range(tilecount):
        if not has_content[idx]:
            continue
        cx = idx % cols
        cy = idx // cols

        # Right neighbor
        if cx + 1 < cols:
            nb = cy * cols + cx + 1
            if nb < tilecount and has_content[nb]:
                h_touch[(idx, nb)] = seam_touches_horizontal(
                    atlas, tile_w, tile_h, cols, idx, nb)

        # Bottom neighbor
        if cy + 1 < rows:
            nb = (cy + 1) * cols + cx
            if nb < tilecount and has_content[nb]:
                v_touch[(idx, nb)] = seam_touches_vertical(
                    atlas, tile_w, tile_h, cols, idx, nb)

    # Flood-fill connected tiles
    visited = [False] * tilecount
    objects = []

    for start in range(tilecount):
        if visited[start] or not has_content[start]:
            continue

        stack = [start]
        component = []

        while stack:
            idx = stack.pop()
            if idx < 0 or idx >= tilecount:
                continue
            if visited[idx] or not has_content[idx]:
                continue
            visited[idx] = True
            component.append(idx)

            cx = idx % cols
            cy = idx // cols

            # Left
            if cx > 0:
                nb = cy * cols + cx - 1
                if nb < tilecount and not visited[nb] and has_content[nb]:
                    if h_touch.get((nb, idx), 0) >= min_touch:
                        stack.append(nb)
            # Right
            if cx + 1 < cols:
                nb = cy * cols + cx + 1
                if nb < tilecount and not visited[nb] and has_content[nb]:
                    if h_touch.get((idx, nb), 0) >= min_touch:
                        stack.append(nb)
            # Up
            if cy > 0:
                nb = (cy - 1) * cols + cx
                if nb < tilecount and not visited[nb] and has_content[nb]:
                    if v_touch.get((nb, idx), 0) >= min_touch:
                        stack.append(nb)
            # Down
            if cy + 1 < rows:
                nb = (cy + 1) * cols + cx
                if nb < tilecount and not visited[nb] and has_content[nb]:
                    if v_touch.get((idx, nb), 0) >= min_touch:
                        stack.append(nb)

        if not component:
            continue

        # Bounding box in atlas grid
        coords = [(i % cols, i // cols) for i in component]
        min_x = min(c[0] for c in coords)
        max_x = max(c[0] for c in coords)
        min_y = min(c[1] for c in coords)
        max_y = max(c[1] for c in coords)
        rect_w = max_x - min_x + 1
        rect_h = max_y - min_y + 1

        # Composite
        comp_w = rect_w * tile_w
        comp_h = rect_h * tile_h
        comp = np.zeros((comp_h, comp_w, 4), dtype=np.uint8)

        for idx in component:
            cx = idx % cols
            cy = idx // cols
            src_x0 = (idx % cols) * tile_w
            src_y0 = (idx // cols) * tile_h
            dst_x0 = (cx - min_x) * tile_w
            dst_y0 = (cy - min_y) * tile_h

            src = atlas[src_y0:src_y0 + tile_h, src_x0:src_x0 + tile_w]
            if src.shape[0] == tile_h and src.shape[1] == tile_w:
                comp[dst_y0:dst_y0 + tile_h, dst_x0:dst_x0 + tile_w] = src

        # Debug markers
        if debug:
            ms = 10  # marker size
            pad = 1
            for idx in component:
                cx = idx % cols
                cy = idx // cols
                tx = (cx - min_x) * tile_w
                ty = (cy - min_y) * tile_h

                # Right edge
                nb = cy * cols + cx + 1 if cx + 1 < cols else -1
                if nb >= 0 and nb < tilecount and has_content[nb]:
                    touches = h_touch.get((idx, nb), 0)
                    color = REASON_STYLE["touch"] if touches >= min_touch else REASON_STYLE["no_touch"]
                else:
                    color = REASON_STYLE["no_nb"]
                draw_marker(comp, tx + tile_w - ms - pad, ty + (tile_h - ms) // 2, ms, ms, color)

                # Left edge
                nb = cy * cols + cx - 1 if cx > 0 else -1
                if nb >= 0 and nb < tilecount and has_content[nb]:
                    touches = h_touch.get((nb, idx), 0)
                    color = REASON_STYLE["touch"] if touches >= min_touch else REASON_STYLE["no_touch"]
                else:
                    color = REASON_STYLE["no_nb"]
                draw_marker(comp, tx + pad, ty + (tile_h - ms) // 2, ms, ms, color)

                # Bottom edge
                nb = (cy + 1) * cols + cx if cy + 1 < rows else -1
                if nb >= 0 and nb < tilecount and has_content[nb]:
                    touches = v_touch.get((idx, nb), 0)
                    color = REASON_STYLE["touch"] if touches >= min_touch else REASON_STYLE["no_touch"]
                else:
                    color = REASON_STYLE["no_nb"]
                draw_marker(comp, tx + (tile_w - ms) // 2, ty + tile_h - ms - pad, ms, ms, color)

                # Top edge
                nb = (cy - 1) * cols + cx if cy > 0 else -1
                if nb >= 0 and nb < tilecount and has_content[nb]:
                    touches = v_touch.get((nb, idx), 0)
                    color = REASON_STYLE["touch"] if touches >= min_touch else REASON_STYLE["no_touch"]
                else:
                    color = REASON_STYLE["no_nb"]
                draw_marker(comp, tx + (tile_w - ms) // 2, ty + pad, ms, ms, color)

        # Trim (skip in debug mode to preserve tile grid alignment)
        if not debug:
            # Find opaque bounding box
            opaque_mask = comp[:, :, 3] > 0
            if not np.any(opaque_mask):
                continue
            rows_with_content = np.any(opaque_mask, axis=1)
            cols_with_content = np.any(opaque_mask, axis=0)
            y1 = np.argmax(rows_with_content)
            y2 = comp.shape[0] - np.argmax(rows_with_content[::-1])
            x1 = np.argmax(cols_with_content)
            x2 = comp.shape[1] - np.argmax(cols_with_content[::-1])
            comp = comp[y1:y2, x1:x2]

        if comp.size == 0:
            continue

        # Skip tiny objects (not in debug)
        if not debug:
            opaque_pixels = np.sum(comp[:, :, 3] > ALPHA_THRESH)
            if opaque_pixels < tile_w * tile_h * 0.15:
                continue

        objects.append((Image.fromarray(comp), len(component)))

    return objects


def process_tileset(tsx_path: Path, output_dir: Path, min_touch: int,
                    debug: bool = False):
    """Process a single tileset and extract all objects."""
    try:
        name, tile_w, tile_h, columns, tilecount, img_path = parse_tsx(tsx_path)
    except Exception as e:
        print(f"  WARNING: Failed to parse {tsx_path.name}: {e}")
        return 0

    if not img_path.exists():
        print(f"  WARNING: Atlas not found: {img_path}")
        return 0

    if "terrain_surfaces" in name.lower():
        return 0

    atlas = np.array(Image.open(img_path).convert("RGBA"))

    objects = extract_objects_from_atlas(atlas, tile_w, tile_h, columns, tilecount,
                                         min_touch=min_touch, debug=debug)
    if not objects:
        return 0

    ts_dir = output_dir / name
    ts_dir.mkdir(parents=True, exist_ok=True)

    saved = 0
    for obj_img, tile_count in objects:
        obj_path = ts_dir / f"{name}_obj{saved:04d}.png"
        obj_img.save(obj_path)
        w, h = obj_img.size
        print(f"  {obj_path.name}: {w}x{h} ({tile_count} tiles)")
        saved += 1

    return saved


def main():
    parser = argparse.ArgumentParser(description="Extract objects from tileset atlases")
    parser.add_argument("paths", nargs="*", help="Specific .tsx files to process")
    parser.add_argument(
        "--min-touch", type=int, default=1,
        help="Min pixels that must touch across a seam to connect (default: 1)",
    )
    parser.add_argument(
        "--debug", action="store_true",
        help="Draw edge markers on exported sprites (green=touch, red=no touch)",
    )
    args = parser.parse_args()

    script_dir = Path(__file__).resolve().parent
    project_root = script_dir.parent.parent
    tileset_dir = project_root / "assets" / "tilesets"
    output_dir = project_root / "assets" / "objects"

    if args.paths:
        tsx_files = [Path(p).resolve() for p in args.paths]
    else:
        tsx_files = sorted(tileset_dir.glob("*.tsx"))

    if not tsx_files:
        print("No TSX files found.")
        return

    output_dir.mkdir(parents=True, exist_ok=True)

    total = 0
    for tsx in tsx_files:
        print(f"Tileset: {tsx.stem}")
        count = process_tileset(tsx, output_dir, args.min_touch, debug=args.debug)
        if count > 0:
            print(f"  -> {count} objects")
        else:
            print(f"  -> no objects found")
        total += count

    print(f"\nDone. {total} total objects saved to {output_dir}")
    if args.debug:
        print("\nDebug legend:")
        print("  Green  = pixels touch across seam (connected)")
        print("  Red    = no pixels touch (separated)")
        print("  Gray   = no neighbor or neighbor empty")


if __name__ == "__main__":
    main()
