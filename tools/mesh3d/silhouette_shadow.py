#!/usr/bin/env python3
"""Generate low-poly shadow meshes from sprite alpha + depth maps.

Bypasses Hunyuan mesh topology entirely. For each input sprite directory:
  1. Extract the 2D silhouette from the alpha channel.
  2. Simplify the contour with Douglas-Peucker.
  3. Triangulate the front face (ear clipping via mapbox_earcut).
  4. Duplicate vertices along ±Z using the depth map for per-vertex protrusion,
     creating a symmetric relief mesh (front and back surfaces).
  5. Stitch side walls between the two contour rings.

The output is:
  • Always manifold (construction is deterministic; no chance of "shattering").
  • Silhouette-exact to the simplification tolerance (~1 px by default).
  • ~100-300 triangles per sprite regardless of the source mesh's complexity.

Overwrites shadow.glb in each sprite directory. The original Hunyuan mesh at
mesh.glb (if present) is left untouched.

Dependencies:
    pip install opencv-python mapbox_earcut trimesh numpy pillow

Usage:
    python silhouette_shadow.py assets/objects/TILESET/SPRITE/
    python silhouette_shadow.py --tolerance 2.0 assets/objects/**/
"""

import argparse
import glob as _glob
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def resolve_sprite_dirs(arg_paths):
    """Expand glob patterns (PowerShell doesn't do this for us) and filter
    to directories that contain a sprite.png. Silently ignores auxiliary
    files (sprite_normal.png, sprite_depth.png, sprite_roughness.png, GLBs,
    txts) so globs like '*/*/*.png' don't try to re-process them as sprites.
    """
    results = []
    seen = set()
    for arg in arg_paths:
        # Only expand if the arg actually contains glob metacharacters;
        # otherwise treat it as a literal path so paths with brackets etc.
        # still work.
        matches = _glob.glob(arg) if any(c in arg for c in "*?[") else [arg]
        if not matches:
            print(f"no match: {arg}")
            continue
        for m in matches:
            p = Path(m)
            # If the user pointed at a specific file, only accept sprite.png.
            # Everything else (sprite_normal.png, sprite_depth.png,
            # sprite_roughness.png, mesh.glb, shadow.glb, type.txt, ...) is
            # skipped without a warning — the glob probably picked them up
            # incidentally.
            if p.is_file():
                if p.name == "sprite.png":
                    p = p.parent
                else:
                    continue
            if not p.is_dir():
                continue
            if not (p / "sprite.png").exists():
                continue
            key = str(p.resolve())
            if key in seen:
                continue
            seen.add(key)
            results.append(p)
    return results


def polygon_area_signed(pts: np.ndarray) -> float:
    """Shoelace signed area — positive when CCW in standard math axes."""
    x, y = pts[:, 0], pts[:, 1]
    return 0.5 * float(np.sum(x * np.roll(y, -1) - np.roll(x, -1) * y))


def build_shadow_mesh(sprite_path: Path, depth_path: Path, max_depth: float,
                      tolerance_px: float):
    """Return a trimesh.Trimesh with the silhouette-extruded shadow mesh.

    Mesh convention matches Hunyuan output so the existing Rust loader needs
    no changes: Y-up, height normalized to 2 units, X scaled by aspect ratio,
    Z (depth) in the same units as max_depth.
    """
    import cv2
    import mapbox_earcut as earcut
    import trimesh

    sprite = np.array(Image.open(sprite_path).convert("RGBA"))
    alpha = sprite[:, :, 3]
    depth_img = np.array(Image.open(depth_path).convert("L"))
    depth = depth_img.astype(np.float32) / 255.0
    h, w = alpha.shape

    # --- Silhouette contour -------------------------------------------------
    mask = (alpha >= 128).astype(np.uint8)
    # Erode so the contour sits a few pixels inside the alpha edge. The
    # depth map is 0 at the silhouette boundary (depth extraction only
    # writes values for fully-opaque pixels); using the original contour
    # gives every ring vertex depth=0 and collapses the mesh into a
    # zero-volume sheet that casts no shadow.
    eroded = cv2.erode(mask, np.ones((3, 3), np.uint8), iterations=2)
    # If erosion wiped the whole sprite (very thin shapes), fall back to
    # the raw mask rather than failing.
    mask_for_contour = eroded if eroded.any() else mask

    contours, _ = cv2.findContours(mask_for_contour, cv2.RETR_EXTERNAL,
                                   cv2.CHAIN_APPROX_NONE)
    if not contours:
        raise ValueError(f"no opaque pixels in {sprite_path}")
    largest = max(contours, key=cv2.contourArea)

    # Douglas-Peucker simplification.
    simplified = cv2.approxPolyDP(largest, tolerance_px, closed=True)
    simplified = simplified.reshape(-1, 2).astype(np.float32)

    # Image pixel coords are Y-down; flipping Y gives standard math axes so
    # shoelace signed-area lines up with "CCW looking from +Z".
    math_pts = np.stack([simplified[:, 0], h - simplified[:, 1]], axis=1)
    if polygon_area_signed(math_pts) < 0:
        simplified = simplified[::-1]
        math_pts = math_pts[::-1]

    n = len(simplified)
    if n < 3:
        raise ValueError(f"simplified contour has <3 vertices in {sprite_path}")

    # --- Triangulate the front face ----------------------------------------
    # mapbox_earcut wants the 2D points as shape (n, 2) float32, and a
    # uint32 ndarray of ring-end indices.
    tri_indices = earcut.triangulate_float32(
        math_pts.astype(np.float32),
        np.array([n], dtype=np.uint32),
    ).reshape(-1, 3)

    # --- Per-vertex depth sample ------------------------------------------
    def sample_depth(px: float, py: float) -> float:
        xi = int(np.clip(round(px), 0, w - 1))
        yi = int(np.clip(round(py), 0, h - 1))
        return float(depth[yi, xi])

    # --- Build 3D vertices -------------------------------------------------
    # Normalize so height spans 2.0 units (matches Hunyuan convention so the
    # Rust loader's `scale = height/2` still produces sprite-sized output).
    s = 2.0 / h

    # Minimum per-vertex depth fraction. Keeps the mesh from collapsing to
    # zero thickness when the depth map reads near-zero at the contour (e.g.,
    # for sprites where the depth extraction only covered the central body
    # of the shape). Pure silhouette extrusion at this floor.
    MIN_DEPTH_FRAC = 0.25

    raw_depths = np.array([sample_depth(px, py) for (px, py) in simplified])
    print(f"    depth samples: "
          f"min={raw_depths.min():.3f}, max={raw_depths.max():.3f}, "
          f"mean={raw_depths.mean():.3f}")

    front_verts = np.empty((n, 3), dtype=np.float32)
    back_verts = np.empty((n, 3), dtype=np.float32)
    for i, (px, py) in enumerate(simplified):
        mx = (px - w / 2) * s              # centered X
        my = (h / 2 - py) * s              # centered Y, flipped to Y-up
        raw_d = sample_depth(px, py)
        d = max(raw_d, MIN_DEPTH_FRAC) * max_depth
        front_verts[i] = [mx, my, d]
        back_verts[i] = [mx, my, -d]

    verts = np.vstack([front_verts, back_verts])

    # --- Faces --------------------------------------------------------------
    faces = []
    # Front face (CCW from +Z).
    for tri in tri_indices:
        faces.append([tri[0], tri[1], tri[2]])
    # Back face, vertex indices offset by n, winding reversed (CCW from -Z).
    for tri in tri_indices:
        faces.append([tri[0] + n, tri[2] + n, tri[1] + n])
    # Side walls: each contour edge becomes a quad (two tris), CCW from
    # outside the mesh.
    for i in range(n):
        a = i
        b = (i + 1) % n
        faces.append([a, b, b + n])
        faces.append([a, b + n, a + n])

    faces = np.asarray(faces, dtype=np.int32)
    return trimesh.Trimesh(vertices=verts, faces=faces, process=False)


def process_sprite_dir(sprite_dir: Path, tolerance: float) -> bool:
    sprite_path = sprite_dir / "sprite.png"
    depth_path = sprite_dir / "sprite_depth.png"
    max_depth_path = sprite_dir / "max_depth.txt"
    shadow_path = sprite_dir / "shadow.glb"

    for p in (sprite_path, depth_path, max_depth_path):
        if not p.exists():
            print(f"SKIP {sprite_dir.name}: missing {p.name}")
            return False

    try:
        max_depth = float(max_depth_path.read_text().strip())
    except Exception as e:
        print(f"SKIP {sprite_dir.name}: bad max_depth.txt ({e})")
        return False

    try:
        mesh = build_shadow_mesh(sprite_path, depth_path, max_depth, tolerance)
    except Exception as e:
        print(f"FAIL {sprite_dir.name}: {e}")
        return False

    mesh.export(str(shadow_path), file_type="glb")
    print(f"OK   {sprite_dir.name}: "
          f"{len(mesh.vertices):>4d} verts, {len(mesh.faces):>4d} tris → "
          f"{shadow_path.name}")
    return True


def main():
    p = argparse.ArgumentParser(
        description="Generate low-poly shadow meshes from sprite alpha + depth.")
    p.add_argument("paths", nargs="+",
                   help="Sprite directory paths (or sprite.png paths).")
    p.add_argument("--tolerance", type=float, default=1.5,
                   help="Douglas-Peucker tolerance in pixels (default 1.5). "
                        "Larger = fewer triangles, coarser silhouette.")
    args = p.parse_args()

    sprite_dirs = resolve_sprite_dirs(args.paths)

    ok = 0
    for sprite_dir in sprite_dirs:
        if process_sprite_dir(sprite_dir, args.tolerance):
            ok += 1

    print(f"\nDone: {ok}/{len(sprite_dirs)} sprites processed.")
    sys.exit(0 if ok == len(sprite_dirs) and sprite_dirs else 1)


if __name__ == "__main__":
    main()
