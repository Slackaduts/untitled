#!/usr/bin/env python3
"""
tileconv — Convert RPG Maker VX Ace tilesets to Tiled-compatible format.

Handles:
  A1 (animated water)    → blob-expanded tileset with Wang terrain definitions
  A2 (ground autotiles)  → blob-expanded tileset with Wang terrain definitions
  A3 (wall tops)         → upscaled simple tileset
  A4 (wall + top)        → upscaled simple tileset
  A5 (simple tiles)      → upscaled simple tileset
  B–E (object tiles)     → upscaled simple tileset

Upscaling: 32px → 48px via 2x nearest neighbor (64px) then Lanczos downsample.

Usage:
  python3 tileconv.py <input.png> -o <output_dir> [--type A1|A2|A3|A4|A5|B]
  python3 tileconv.py --batch ~/Downloads/celianna/ -o assets/tilesets/
"""

import argparse
import os
import re
import sys
from pathlib import Path

from PIL import Image, ImageFilter

# ── Constants ──────────────────────────────────────────────────────────────────

SRC_TILE = 32
DST_TILE = 48
HALF_SRC = SRC_TILE // 2
HALF_DST = DST_TILE // 2


# ── Upscaling ──────────────────────────────────────────────────────────────────

def edge_aa(img: Image.Image, radius: float = 0.75) -> Image.Image:
    """Anti-alias silhouette edges by feathering only the alpha channel."""
    if img.mode != "RGBA":
        return img
    r, g, b, a = img.split()
    a = a.filter(ImageFilter.GaussianBlur(radius=radius))
    return Image.merge("RGBA", (r, g, b, a))


def pad_edges(img: Image.Image, iterations: int = 4) -> Image.Image:
    """
    Dilate RGB colors into transparent pixels so Lanczos resampling
    never interpolates toward black. Each iteration spreads opaque
    colors one pixel outward into transparent neighbors.
    """
    if img.mode != "RGBA":
        return img
    import numpy as np

    data = np.array(img)
    rgb = data[:, :, :3].astype(np.float32)
    alpha = data[:, :, 3]

    for _ in range(iterations):
        # Find transparent pixels that border opaque ones
        opaque = alpha > 0
        if opaque.all():
            break

        # Accumulate neighbor colors (shifted in 4 cardinal directions)
        accum = np.zeros_like(rgb)
        count = np.zeros(alpha.shape, dtype=np.float32)

        for dy, dx in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
            shifted_rgb = np.roll(np.roll(rgb, dy, axis=0), dx, axis=1)
            shifted_opaque = np.roll(np.roll(opaque, dy, axis=0), dx, axis=1).astype(np.float32)
            accum += shifted_rgb * shifted_opaque[:, :, np.newaxis]
            count += shifted_opaque

        # Fill transparent pixels that have opaque neighbors
        fill_mask = (~opaque) & (count > 0)
        safe_count = np.where(count > 0, count, 1.0)
        avg = accum / safe_count[:, :, np.newaxis]

        rgb[fill_mask] = avg[fill_mask]
        alpha[fill_mask] = 1  # mark as "has color" for next iteration but keep alpha low

    # Restore original alpha (we only wanted to extend RGB)
    result = np.zeros_like(data)
    result[:, :, :3] = rgb.clip(0, 255).astype(np.uint8)
    result[:, :, 3] = data[:, :, 3]  # original alpha
    return Image.fromarray(result, "RGBA")


def upscale_image(img: Image.Image, aa: bool = False) -> Image.Image:
    """Upscale pixel art: 2x nearest neighbor then Lanczos downsample to 1.5x.
    Pads RGB into transparent areas first to prevent dark fringing."""
    w2, h2 = img.width * 2, img.height * 2
    big = img.resize((w2, h2), Image.NEAREST)
    big = pad_edges(big)
    ratio = DST_TILE / (SRC_TILE * 2)
    out = big.resize((int(w2 * ratio), int(h2 * ratio)), Image.LANCZOS)
    return edge_aa(out) if aa else out


def upscale_minitile(img: Image.Image) -> Image.Image:
    """Upscale a single 16x16 mini-tile to 24x24."""
    big = img.resize((32, 32), Image.NEAREST)
    return big.resize((HALF_DST, HALF_DST), Image.LANCZOS)


# ── Mini-tile composition ─────────────────────────────────────────────────────

def compose_tile(minitiles: list[Image.Image]) -> Image.Image:
    """Compose 4 upscaled mini-tiles (24x24 each) into a 48x48 tile."""
    tile = Image.new("RGBA", (DST_TILE, DST_TILE))
    tile.paste(minitiles[0], (0, 0))
    tile.paste(minitiles[1], (HALF_DST, 0))
    tile.paste(minitiles[2], (0, HALF_DST))
    tile.paste(minitiles[3], (HALF_DST, HALF_DST))
    return tile


# ── 47-blob autotile logic ───────────────────────────────────────────────────
#
# Neighbor bitmask (8-way):
#   128  1   2
#    64  X   4
#    32 16   8
#
# For each quadrant, which cardinal + diagonal neighbors matter:
#   TL: top(1), left(64), top-left(128)
#   TR: top(1), right(4), top-right(2)
#   BL: bottom(16), left(64), bottom-left(32)
#   BR: bottom(16), right(4), bottom-right(8)

QUADRANT_BITS = [
    (1, 64, 128),  # TL: top, left, top-left
    (1, 4, 2),     # TR: top, right, top-right
    (16, 64, 32),  # BL: bottom, left, bottom-left
    (16, 4, 8),    # BR: bottom, right, bottom-right
]

# RPG Maker VX Ace A2 autotile block layout (2×3 tiles = 64×96px at 32px):
#
#   [0,0] [1,0]   ← inner corners (concave)
#   [0,1] [1,1]   ← full interior / edge pieces
#   [0,2] [1,2]   ← outer corners (convex)
#
# For each quadrant, based on neighbor presence, we source from a specific
# cell in the block. The quadrant index determines which sub-quadrant of
# that cell to extract.
#
# Cases per quadrant:
#   both cardinals + diagonal present → full interior: cell (1,1)
#   both cardinals, no diagonal       → inner corner:  cell (0,0)
#   cardinal_a only (no cardinal_b)   → horizontal edge
#   cardinal_b only (no cardinal_a)   → vertical edge
#   neither cardinal                  → outer corner:  cell (0,2)

def get_minitile(sheet: Image.Image, bx: int, by: int, qi: int, mask: int) -> Image.Image:
    """
    Get the correct 16x16 mini-tile for quadrant qi given neighbor bitmask.
    bx, by: tile position of the 2x3 autotile block's top-left corner.
    qi: quadrant index (0=TL, 1=TR, 2=BL, 3=BR).
    """
    ca_bit, cb_bit, diag_bit = QUADRANT_BITS[qi]
    ca = bool(mask & ca_bit)
    cb = bool(mask & cb_bit)
    diag = bool(mask & diag_bit)

    # Cell selection derived from mkxp-z autotilesvx.cpp lookup table.
    # Each case uses a formula over qi to pick the correct cell for that quadrant.
    if ca and cb:
        if diag:
            # Interior: mirrored diagonal pattern across quadrants
            cx = (qi + 1) % 2
            cy = 2 - (qi // 2)
        else:
            # Inner corner: always cell(1,0)
            cx, cy = 1, 0
    elif ca and not cb:
        # Vertical cardinal present, horizontal missing
        cx = qi % 2
        cy = 2 - (qi // 2)
    elif not ca and cb:
        # Horizontal cardinal present, vertical missing
        cx = (qi + 1) % 2
        cy = 1 + (qi // 2)
    else:
        # Neither cardinal: outer corner
        cx = qi % 2
        cy = 1 + (qi // 2)

    px = (bx + cx) * SRC_TILE + (qi % 2) * HALF_SRC
    py = (by + cy) * SRC_TILE + (qi // 2) * HALF_SRC
    return sheet.crop((px, py, px + HALF_SRC, py + HALF_SRC))


MASK_FILL_WATER = (255, 0, 0)    # R channel = water
MASK_FILL_GRASS = (0, 255, 0)    # G channel = grass
MASK_EDGE       = (0, 0, 255)    # B channel = edge

# Color distance threshold for per-pixel fill classification.
# Pixels within this distance from the interior mean are "fill".
FILL_THRESHOLD = 45


def build_autotile_masks(sheet: Image.Image, bx: int, by: int,
                         masks: list[int],
                         fill_color: tuple[int, int, int]) -> list[Image.Image]:
    """
    Generate per-pixel mask tiles by comparing each pixel to the interior
    fill color. This correctly handles edge and corner minitiles — only pixels
    that look like the fill terrain get replaced by the shader; rocky borders
    and other terrain retain the original artwork.
    """
    from PIL import ImageChops

    # ── Build interior reference: mean color of the mask=255 tile ────────
    interior_mts = [
        upscale_minitile(get_minitile(sheet, bx, by, qi, 255))
        for qi in range(4)
    ]
    interior_tile = compose_tile(interior_mts).convert("RGBA")

    # Collect opaque pixel RGB values
    pixels = list(interior_tile.convert("RGBA").tobytes())
    pixels = [(pixels[i], pixels[i+1], pixels[i+2], pixels[i+3])
              for i in range(0, len(pixels), 4)]
    opaque = [(r, g, b) for r, g, b, a in pixels if a > 128]
    if not opaque:
        # Fully transparent interior — skip mask generation for this block
        return [Image.new("RGB", (DST_TILE, DST_TILE)) for _ in masks]

    n = len(opaque)
    mean_color = (
        sum(p[0] for p in opaque) // n,
        sum(p[1] for p in opaque) // n,
        sum(p[2] for p in opaque) // n,
    )

    # ── For each autotile variant, build a per-pixel mask ────────────────
    result = []
    for mask in masks:
        mask_tile = Image.new("RGB", (DST_TILE, DST_TILE), (0, 0, 0))

        for qi in range(4):
            ca_bit, cb_bit, diag_bit = QUADRANT_BITS[qi]
            ca = bool(mask & ca_bit)
            cb = bool(mask & cb_bit)
            diag = bool(mask & diag_bit)
            is_interior = ca and cb and diag

            mt = get_minitile(sheet, bx, by, qi, mask)
            mt_up = upscale_minitile(mt).convert("RGBA")
            rgb = mt_up.convert("RGB")

            if is_interior:
                # Interior minitile: force 100% fill (no color comparison).
                # This prevents Lanczos edge artifacts from creating seams.
                mask_mt = Image.new("RGB", rgb.size, fill_color)
            else:
                # Edge/corner minitile: per-pixel color comparison.
                ref = Image.new("RGB", rgb.size, mean_color)
                diff_gray = ImageChops.difference(rgb, ref).convert("L")

                fill_mask = diff_gray.point(
                    lambda p: 255 if p < FILL_THRESHOLD else 0
                )

                # Transparent pixels should never be fill
                alpha = mt_up.split()[3]
                alpha_mask = alpha.point(lambda p: 255 if p > 128 else 0)
                fill_mask = ImageChops.multiply(fill_mask.convert("L"),
                                                alpha_mask.convert("L"))

                # Opaque but not fill → edge
                edge_mask = ImageChops.subtract(alpha_mask.convert("L"),
                                                fill_mask.convert("L"))

                mask_mt = Image.new("RGB", rgb.size, (0, 0, 0))
                mask_mt.paste(Image.new("RGB", rgb.size, MASK_EDGE),
                              mask=edge_mask)
                mask_mt.paste(Image.new("RGB", rgb.size, fill_color),
                              mask=fill_mask)

            x = (qi % 2) * HALF_DST
            y = (qi // 2) * HALF_DST
            mask_tile.paste(mask_mt, (x, y))

        result.append(mask_tile)
    return result


def generate_canonical_masks() -> list[int]:
    """Generate the 47 canonical neighbor masks (+ isolated = 48 total)."""
    seen = set()
    canonical = []
    for mask in range(256):
        n = mask
        # Diagonals only matter when both adjacent cardinals are present
        if not (n & 1 and n & 64):
            n &= ~128
        if not (n & 1 and n & 4):
            n &= ~2
        if not (n & 16 and n & 64):
            n &= ~32
        if not (n & 16 and n & 4):
            n &= ~8
        if n not in seen:
            seen.add(n)
            canonical.append(n)
    canonical.sort()
    return canonical


def mask_to_wangid(mask: int) -> str:
    """
    Convert an 8-bit neighbor bitmask to a Tiled wangid string.

    Wangid order: top, top-right, right, bottom-right, bottom, bottom-left, left, top-left
    For mixed Wang sets, each position is:
      0 = "empty/other terrain"
      1 = "this terrain" (the autotile ground type)

    The wangid describes what terrain is on each edge/corner of THIS tile.
    If a neighbor is present in the mask, it means that edge/corner is
    the same terrain as the center → color 1. Otherwise → color 2 (other).
    """
    # Map bitmask bits to wangid positions
    # Bit: 1=top, 2=top-right, 4=right, 8=bottom-right,
    #       16=bottom, 32=bottom-left, 64=left, 128=top-left
    # Wangid positions: top, top-right, right, bottom-right, bottom, bottom-left, left, top-left
    # Color 1 = this terrain, color 0 = empty/other
    bit_order = [1, 2, 4, 8, 16, 32, 64, 128]

    parts = []
    for bit in bit_order:
        parts.append("1" if mask & bit else "0")
    return ",".join(parts)


def build_autotile_block(sheet: Image.Image, bx: int, by: int,
                         masks: list[int]) -> list[Image.Image]:
    """Expand one 2×3 autotile block into 48 composed output tiles."""
    tiles = []
    for mask in masks:
        mts = [upscale_minitile(get_minitile(sheet, bx, by, qi, mask)) for qi in range(4)]
        tiles.append(compose_tile(mts))
    return tiles


# ── A2 Processing ─────────────────────────────────────────────────────────────
# A2 sheet: 512×384 = 16×12 tiles at 32px
# 8 block columns × 4 block rows (each block 2×3 tiles) = 32 autotile types

def process_a2(sheet: Image.Image, output_dir: Path, name: str,
               gen_mask: bool = False):
    masks = generate_canonical_masks()
    n_per_block = len(masks)

    block_cols = sheet.width // (SRC_TILE * 2)
    block_rows = sheet.height // (SRC_TILE * 3)

    all_tiles = []
    all_mask_tiles = []
    block_names = []

    for by_idx in range(block_rows):
        for bx_idx in range(block_cols):
            bx = bx_idx * 2
            by = by_idx * 3
            label = f"terrain_{by_idx * block_cols + bx_idx}"
            block_names.append(label)
            all_tiles.extend(build_autotile_block(sheet, bx, by, masks))
            if gen_mask:
                all_mask_tiles.extend(build_autotile_masks(sheet, bx, by, masks, MASK_FILL_GRASS))

    # Output: 16 tiles wide
    cols = 16
    rows = (len(all_tiles) + cols - 1) // cols
    out = Image.new("RGBA", (cols * DST_TILE, rows * DST_TILE))
    for i, tile in enumerate(all_tiles):
        out.paste(tile, ((i % cols) * DST_TILE, (i // cols) * DST_TILE))

    out_path = output_dir / f"{name}.png"
    out.save(out_path)
    print(f"  → {out_path} ({out.width}×{out.height}, {len(all_tiles)} tiles)")

    if gen_mask:
        save_mask_atlas(all_mask_tiles, cols, output_dir, name)

    # Generate .tsx with Wang terrain sets
    generate_wang_tsx(output_dir, name, out.width, out.height, len(all_tiles),
                      cols, block_names, masks, n_per_block)


# ── A1 Processing ─────────────────────────────────────────────────────────────
# A1: animated water. Extract frame 0 of each autotile block.

def process_a1(sheet: Image.Image, output_dir: Path, name: str,
               gen_mask: bool = False):
    masks = generate_canonical_masks()
    n_per_block = len(masks)

    # Frame 0 block positions for A1 (tile coordinates)
    a1_blocks = [(0, 0), (6, 0), (0, 4), (6, 4)]
    block_names = [f"water_{i}" for i in range(len(a1_blocks))]

    all_tiles = []
    all_mask_tiles = []
    for bx, by in a1_blocks:
        if bx * SRC_TILE < sheet.width and (by + 2) * SRC_TILE <= sheet.height:
            all_tiles.extend(build_autotile_block(sheet, bx, by, masks))
            if gen_mask:
                all_mask_tiles.extend(build_autotile_masks(sheet, bx, by, masks, MASK_FILL_WATER))
        else:
            block_names.pop()

    # Also waterfall tiles (simple, non-autotile) from rows 8+
    for col in range(sheet.width // SRC_TILE):
        for row in range(8, sheet.height // SRC_TILE):
            src = sheet.crop((col * SRC_TILE, row * SRC_TILE,
                              (col + 1) * SRC_TILE, (row + 1) * SRC_TILE))
            all_tiles.append(upscale_image(src))
            if gen_mask:
                # Waterfall tiles: full water fill
                all_mask_tiles.append(
                    Image.new("RGB", (DST_TILE, DST_TILE), MASK_FILL_WATER))

    cols = 16
    rows_img = (len(all_tiles) + cols - 1) // cols
    out = Image.new("RGBA", (cols * DST_TILE, rows_img * DST_TILE))
    for i, tile in enumerate(all_tiles):
        out.paste(tile, ((i % cols) * DST_TILE, (i // cols) * DST_TILE))

    out_path = output_dir / f"{name}.png"
    out.save(out_path)
    print(f"  → {out_path} ({out.width}×{out.height}, {len(all_tiles)} tiles)")

    if gen_mask:
        save_mask_atlas(all_mask_tiles, cols, output_dir, name)

    generate_wang_tsx(output_dir, name, out.width, out.height, len(all_tiles),
                      cols, block_names, masks, n_per_block)


# ── Simple Processing (A3, A4, A5, B–E) ──────────────────────────────────────

def find_tile_objects(sheet: Image.Image) -> list[list[tuple[int, int]]]:
    """
    Flood fill connected components using border transparency analysis.
    Two adjacent tiles are connected only if they share opaque pixels at
    their mutual border — i.e., the object visually crosses the tile seam.
    """
    cols = sheet.width // SRC_TILE
    rows = sheet.height // SRC_TILE

    # Which tiles have any content
    occupied = [[False] * cols for _ in range(rows)]
    for ty in range(rows):
        for tx in range(cols):
            crop = sheet.crop((tx * SRC_TILE, ty * SRC_TILE,
                               (tx + 1) * SRC_TILE, (ty + 1) * SRC_TILE))
            alphas = crop.getchannel("A").getdata()
            occupied[ty][tx] = any(a > 0 for a in alphas)

    # Check if two adjacent tiles share content at their border.
    # Sample a strip along the shared edge (2px on each side) and check
    # if both sides have opaque pixels at overlapping positions.
    BORDER_PX = 3  # pixels from edge to check

    def shares_border_h(tx1: int, ty: int, tx2: int) -> bool:
        """Check if tile (tx1,ty) and (tx2,ty) share content at their vertical border."""
        # Right edge of tx1, left edge of tx2
        for py in range(SRC_TILE):
            right_opaque = False
            left_opaque = False
            for dx in range(BORDER_PX):
                px_r = (tx1 * SRC_TILE + SRC_TILE - 1 - dx, ty * SRC_TILE + py)
                px_l = (tx2 * SRC_TILE + dx, ty * SRC_TILE + py)
                if sheet.getpixel(px_r)[3] > 32:
                    right_opaque = True
                if sheet.getpixel(px_l)[3] > 32:
                    left_opaque = True
                if right_opaque and left_opaque:
                    return True
        return False

    def shares_border_v(tx: int, ty1: int, ty2: int) -> bool:
        """Check if tile (tx,ty1) and (tx,ty2) share content at their horizontal border."""
        for px in range(SRC_TILE):
            bottom_opaque = False
            top_opaque = False
            for dy in range(BORDER_PX):
                px_b = (tx * SRC_TILE + px, ty1 * SRC_TILE + SRC_TILE - 1 - dy)
                px_t = (tx * SRC_TILE + px, ty2 * SRC_TILE + dy)
                if sheet.getpixel(px_b)[3] > 32:
                    bottom_opaque = True
                if sheet.getpixel(px_t)[3] > 32:
                    top_opaque = True
                if bottom_opaque and top_opaque:
                    return True
        return False

    visited = [[False] * cols for _ in range(rows)]
    objects = []

    for sy in range(rows):
        for sx in range(cols):
            if visited[sy][sx] or not occupied[sy][sx]:
                continue
            stack = [(sx, sy)]
            component = []
            while stack:
                cx, cy = stack.pop()
                if cx < 0 or cx >= cols or cy < 0 or cy >= rows:
                    continue
                if visited[cy][cx] or not occupied[cy][cx]:
                    continue
                visited[cy][cx] = True
                component.append((cx, cy))
                # Only connect to neighbors that share border content
                if cx + 1 < cols and not visited[cy][cx+1] and occupied[cy][cx+1]:
                    if shares_border_h(cx, cy, cx + 1):
                        stack.append((cx + 1, cy))
                if cx - 1 >= 0 and not visited[cy][cx-1] and occupied[cy][cx-1]:
                    if shares_border_h(cx - 1, cy, cx):
                        stack.append((cx - 1, cy))
                if cy + 1 < rows and not visited[cy+1][cx] and occupied[cy+1][cx]:
                    if shares_border_v(cx, cy, cy + 1):
                        stack.append((cx, cy + 1))
                if cy - 1 >= 0 and not visited[cy-1][cx] and occupied[cy-1][cx]:
                    if shares_border_v(cx, cy - 1, cy):
                        stack.append((cx, cy - 1))
            if component:
                objects.append(component)

    return objects


def upscale_object(sheet: Image.Image, tiles: list[tuple[int, int]]) -> tuple[Image.Image, int, int, int, int]:
    """
    Extract an object's bounding box from the sheet, upscale as one image,
    return (upscaled_image, min_col, min_row, width_tiles, height_tiles).
    """
    min_x = min(t[0] for t in tiles)
    max_x = max(t[0] for t in tiles)
    min_y = min(t[1] for t in tiles)
    max_y = max(t[1] for t in tiles)
    tw = max_x - min_x + 1
    th = max_y - min_y + 1

    # Extract the bounding box region
    src_x = min_x * SRC_TILE
    src_y = min_y * SRC_TILE
    src_w = tw * SRC_TILE
    src_h = th * SRC_TILE
    region = sheet.crop((src_x, src_y, src_x + src_w, src_y + src_h))

    # Clear any tiles within the bounding box that aren't part of this object
    tile_set = set(tiles)
    cleared = region.copy()
    for ty in range(th):
        for tx in range(tw):
            if (min_x + tx, min_y + ty) not in tile_set:
                # Clear this tile to transparent
                blank = Image.new("RGBA", (SRC_TILE, SRC_TILE), (0, 0, 0, 0))
                cleared.paste(blank, (tx * SRC_TILE, ty * SRC_TILE))

    # Upscale: 2x nearest then Lanczos to 1.5x, with edge padding
    w2, h2 = cleared.width * 2, cleared.height * 2
    big = cleared.resize((w2, h2), Image.NEAREST)
    big = pad_edges(big)
    ratio = DST_TILE / (SRC_TILE * 2)
    upscaled = big.resize((int(w2 * ratio), int(h2 * ratio)), Image.LANCZOS)

    return upscaled, min_x, min_y, tw, th


def process_simple(sheet: Image.Image, output_dir: Path, name: str, aa: bool = True):
    cols = sheet.width // SRC_TILE
    rows = sheet.height // SRC_TILE
    out = Image.new("RGBA", (cols * DST_TILE, rows * DST_TILE), (0, 0, 0, 0))

    # Find connected objects and upscale each independently
    objects = find_tile_objects(sheet)
    print(f"  Found {len(objects)} objects ({sum(len(o) for o in objects)} tiles)")

    for obj_tiles in objects:
        upscaled, ox, oy, ow, oh = upscale_object(sheet, obj_tiles)
        out.paste(upscaled, (ox * DST_TILE, oy * DST_TILE), upscaled)

    if aa:
        out = edge_aa(out)

    out_path = output_dir / f"{name}.png"
    out.save(out_path)
    n_tiles = cols * rows
    print(f"  → {out_path} ({out.width}×{out.height}, {n_tiles} tiles)")
    generate_simple_tsx(output_dir, name, out.width, out.height, n_tiles, cols)


# ── Mask Atlas Output ────────────────────────────────────────────────────

def save_mask_atlas(mask_tiles: list[Image.Image], cols: int,
                    output_dir: Path, name: str):
    """Save mask tiles as a flat 2D atlas PNG, same grid as the tile atlas."""
    rows = (len(mask_tiles) + cols - 1) // cols
    out = Image.new("RGB", (cols * DST_TILE, rows * DST_TILE), (0, 0, 0))
    for i, tile in enumerate(mask_tiles):
        out.paste(tile, ((i % cols) * DST_TILE, (i // cols) * DST_TILE))

    mask_path = output_dir / f"{name}_mask.png"
    out.save(mask_path)
    print(f"  → {mask_path} (mask atlas, {out.width}×{out.height})")


# ── TSX Generation ────────────────────────────────────────────────────────────

def generate_simple_tsx(output_dir: Path, name: str, img_w: int, img_h: int,
                        tile_count: int, columns: int):
    """Generate a plain .tsx with no Wang sets."""
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<tileset version="1.10" tiledversion="1.11.2" name="{name}"'
        f' tilewidth="{DST_TILE}" tileheight="{DST_TILE}"'
        f' tilecount="{tile_count}" columns="{columns}">',
        f' <image source="{name}.png" width="{img_w}" height="{img_h}"/>',
        '</tileset>',
    ]
    tsx_path = output_dir / f"{name}.tsx"
    tsx_path.write_text("\n".join(lines) + "\n")
    print(f"  → {tsx_path}")


def generate_wang_tsx(output_dir: Path, name: str, img_w: int, img_h: int,
                      tile_count: int, columns: int, block_names: list[str],
                      masks: list[int], tiles_per_block: int):
    """Generate a .tsx with mixed Wang terrain sets for autotile blocks."""
    colors = [
        ("#00aa00", "#aa5500"), ("#0055aa", "#555555"),
        ("#aa0000", "#00aa55"), ("#aa00aa", "#aaaa00"),
        ("#55aaff", "#ff5500"), ("#00ff00", "#ff00ff"),
        ("#ffaa00", "#0000ff"), ("#ff5555", "#55ff55"),
    ]

    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<tileset version="1.10" tiledversion="1.11.2" name="{name}"'
        f' tilewidth="{DST_TILE}" tileheight="{DST_TILE}"'
        f' tilecount="{tile_count}" columns="{columns}">',
        f' <image source="{name}.png" width="{img_w}" height="{img_h}"/>',
        ' <wangsets>',
    ]

    for block_idx, block_name in enumerate(block_names):
        base_tile = block_idx * tiles_per_block
        cp = colors[block_idx % len(colors)]

        # Find the "all neighbors" tile for representative
        rep_tile = base_tile
        for i, m in enumerate(masks):
            if m == 255:
                rep_tile = base_tile + i
                break

        lines.append(f'  <wangset name="{block_name}" type="mixed" tile="{rep_tile}">')
        lines.append(f'   <wangcolor name="{block_name}" color="{cp[0]}" tile="{rep_tile}" probability="1"/>')

        for i, mask in enumerate(masks):
            wangid = mask_to_wangid(mask)
            lines.append(f'   <wangtile tileid="{base_tile + i}" wangid="{wangid}"/>')

        lines.append('  </wangset>')

    lines.append(' </wangsets>')
    lines.append('</tileset>')

    tsx_path = output_dir / f"{name}.tsx"
    tsx_path.write_text("\n".join(lines) + "\n")
    print(f"  → {tsx_path}")


# ── Type Detection ────────────────────────────────────────────────────────────

def detect_type(filename: str) -> str | None:
    name = filename.upper()
    if "TILEA1" in name:
        return "A1"
    elif "TILEA2" in name:
        return "A2"
    elif "TILEA3" in name:
        return "A3"
    elif "TILEA4" in name:
        return "A4"
    elif "TILEA5" in name:
        return "A5"
    elif re.match(r".*TILE[B-E]", name):
        return "B"
    return None


def process_file(input_path: Path, output_dir: Path, tile_type: str | None = None,
                 aa: bool = True, gen_mask: bool = False):
    name = input_path.stem
    sheet = Image.open(input_path).convert("RGBA")

    if tile_type is None:
        tile_type = detect_type(input_path.name)
    if tile_type is None:
        print(f"  ⚠ Cannot detect type for {input_path.name}, skipping.")
        return

    print(f"Processing {input_path.name} as {tile_type}:")

    if tile_type == "A1":
        process_a1(sheet, output_dir, name, gen_mask=gen_mask)
    elif tile_type == "A2":
        process_a2(sheet, output_dir, name, gen_mask=gen_mask)
    elif tile_type in ("A3", "A4", "A5", "B"):
        process_simple(sheet, output_dir, name, aa=aa)
    else:
        process_simple(sheet, output_dir, name, aa=aa)


# ── CLI ───────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Convert RPG Maker VX Ace tilesets to Tiled format with Wang terrains.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s TileA2_exterior.png -o assets/tilesets/ --type A2
  %(prog)s --batch ~/Downloads/celianna/ -o assets/tilesets/
""")
    parser.add_argument("input", nargs="?", help="Input PNG file")
    parser.add_argument("-o", "--output", required=True, help="Output directory")
    parser.add_argument("-t", "--type", choices=["A1", "A2", "A3", "A4", "A5", "B"],
                        help="Tile type (auto-detected from filename if omitted)")
    parser.add_argument("--batch", help="Process all matching PNGs in a directory")
    parser.add_argument("--filter", default="*Tile*.png",
                        help="Glob pattern for batch mode (default: *Tile*.png)")
    parser.add_argument("--src-tile", type=int, default=32, help="Source tile size")
    parser.add_argument("--dst-tile", type=int, default=48, help="Output tile size")
    parser.add_argument("--no-aa", action="store_true",
                        help="Disable edge anti-aliasing on simple tilesets")
    parser.add_argument("--mask", action="store_true",
                        help="Generate terrain fill mask atlases for A1/A2 tilesets")

    args = parser.parse_args()

    global SRC_TILE, DST_TILE, HALF_SRC, HALF_DST
    SRC_TILE = args.src_tile
    DST_TILE = args.dst_tile
    HALF_SRC = SRC_TILE // 2
    HALF_DST = DST_TILE // 2

    output_dir = Path(args.output)
    output_dir.mkdir(parents=True, exist_ok=True)

    if args.batch:
        batch_dir = Path(args.batch)
        import fnmatch
        files = sorted([f for f in batch_dir.iterdir()
                        if f.is_file() and fnmatch.fnmatch(f.name, args.filter)])
        if not files:
            print(f"No files matching '{args.filter}' in {batch_dir}")
            sys.exit(1)
        print(f"Batch processing {len(files)} files from {batch_dir}\n")
        aa = not args.no_aa
        for f in files:
            process_file(f, output_dir, args.type, aa=aa, gen_mask=args.mask)
            print()
    elif args.input:
        process_file(Path(args.input), output_dir, args.type, aa=not args.no_aa,
                     gen_mask=args.mask)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
