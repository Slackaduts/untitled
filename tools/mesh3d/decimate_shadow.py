#!/usr/bin/env python3
"""Generate low-poly watertight shadow meshes from mesh.glb.

For each sprite directory containing mesh.glb (the full Hunyuan
reconstruction):
  1. Load and clean the mesh topology (weld vertices, drop degenerate and
     duplicate faces, reconcile winding).
  2. Voxel-remesh at --prepass-res to force a watertight manifold. Hunyuan
     output is non-manifold (open edges, self-intersections); running QEM
     directly on it leaves the output with holes — fine for a silhouette
     from one angle, but the shadow pass sees through the holes and the
     cast shadow looks "unfolded" / hollowed-out. Marching cubes on a
     filled voxel grid is always manifold, so the subsequent QEM operates
     on a clean input and its output stays closed.
  3. QEM-decimate the voxel remesh to --target triangles. Fine-grained
     surface detail lost, but silhouette and enclosure are preserved.
  4. Fallback: if QEM output still looks shattered, use the raw voxel
     remesh at --fallback-res (fewer triangles, blockier) which is
     guaranteed manifold.
  5. Write to shadow.glb.

Dependencies:
    pip install trimesh fast-simplification numpy

Usage:
    python decimate_shadow.py [--target N] sprite_dir...

    --target         QEM target triangle count (default 1500).
    --prepass-res    Voxel resolution for the watertightening pass (96).
    --fallback-res   Voxel resolution for the shatter-fallback (48).
"""

import argparse
import glob as _glob
import sys
from pathlib import Path

import numpy as np


def resolve_sprite_dirs(arg_paths, required_file="mesh.glb"):
    """Expand globs and filter to sprite directories that contain the file
    we need. PowerShell doesn't expand globs for us; do it here."""
    results = []
    seen = set()
    for arg in arg_paths:
        matches = _glob.glob(arg) if any(c in arg for c in "*?[") else [arg]
        if not matches:
            print(f"no match: {arg}")
            continue
        for m in matches:
            p = Path(m)
            if p.is_file():
                if p.name == required_file or p.name == "sprite.png":
                    p = p.parent
                else:
                    continue
            if not p.is_dir() or not (p / required_file).exists():
                continue
            key = str(p.resolve())
            if key in seen:
                continue
            seen.add(key)
            results.append(p)
    return results


def load_concatenated(path):
    """Load a glb/gltf as a single Trimesh, concatenating multi-mesh scenes."""
    import trimesh
    loaded = trimesh.load(str(path), force="mesh", process=False)
    if isinstance(loaded, trimesh.Scene):
        geoms = [g for g in loaded.geometry.values()
                 if isinstance(g, trimesh.Trimesh)]
        if not geoms:
            return None
        return trimesh.util.concatenate(geoms) if len(geoms) > 1 else geoms[0]
    return loaded if isinstance(loaded, trimesh.Trimesh) else None


def clean(mesh):
    """Best-effort topology cleanup before decimation."""
    import trimesh
    mesh.merge_vertices(merge_norm=True, merge_tex=False)
    mesh.update_faces(mesh.nondegenerate_faces())
    mesh.update_faces(mesh.unique_faces())
    mesh.remove_unreferenced_vertices()
    try:
        trimesh.repair.fix_normals(mesh, multibody=True)
    except Exception:
        pass
    return mesh


def decimate_qem(mesh, target_faces):
    """QEM via fast-simplification (called through trimesh)."""
    return mesh.simplify_quadric_decimation(face_count=target_faces)


def voxel_remesh(mesh, resolution):
    """Re-voxelize at low resolution and march cubes. Always manifold,
    predictable triangle count, but loses fine surface detail."""
    pitch = float(max(mesh.extents)) / resolution
    if pitch <= 0:
        return mesh
    return mesh.voxelized(pitch=pitch).fill().marching_cubes


def looks_shattered(mesh, target_faces):
    """Heuristic for QEM output that fragmented instead of decimating."""
    if len(mesh.faces) < 50:
        return True
    if len(mesh.faces) < target_faces * 0.1:
        return True
    try:
        components = mesh.split(only_watertight=False)
    except Exception:
        return False
    if len(components) > 10:
        return True
    # If most faces ended up in tiny islands, the mesh fragmented.
    component_face_counts = sorted((len(c.faces) for c in components),
                                    reverse=True)
    if component_face_counts and component_face_counts[0] < 0.5 * len(mesh.faces):
        return True
    return False


def process(sprite_dir: Path, target_faces: int,
            prepass_res: int, fallback_res: int) -> bool:
    mesh_path = sprite_dir / "mesh.glb"
    shadow_path = sprite_dir / "shadow.glb"

    raw = load_concatenated(mesh_path)
    if raw is None:
        print(f"FAIL {sprite_dir.name}: no geometry in mesh.glb")
        return False

    orig_faces = len(raw.faces)
    raw = clean(raw)
    cleaned_faces = len(raw.faces)

    # Watertighten: voxel remesh + marching cubes → guaranteed manifold.
    try:
        watertight = voxel_remesh(raw, prepass_res)
    except Exception as e:
        print(f"FAIL {sprite_dir.name}: voxel prepass raised {e}")
        return False

    prepass_faces = len(watertight.faces)

    # Now decimate the manifold. QEM on manifold input produces manifold
    # output, so the result stays closed.
    method = "qem"
    try:
        decimated = decimate_qem(watertight, target_faces)
    except Exception as e:
        print(f"  {sprite_dir.name}: QEM raised {e}; using voxel fallback")
        decimated = voxel_remesh(raw, fallback_res)
        method = "voxel-fallback"

    if method == "qem" and looks_shattered(decimated, target_faces):
        print(f"  {sprite_dir.name}: QEM output looks shattered "
              f"({len(decimated.faces)} faces); using voxel fallback")
        decimated = voxel_remesh(raw, fallback_res)
        method = "voxel-fallback"

    out_faces = len(decimated.faces)
    decimated.export(str(shadow_path), file_type="glb")
    print(f"OK   {sprite_dir.name}: {orig_faces} → cleaned {cleaned_faces} "
          f"→ watertight {prepass_faces} → {method} {out_faces} faces")
    return True


def main():
    p = argparse.ArgumentParser(
        description="Decimate Hunyuan mesh.glb into a watertight low-poly shadow.glb.")
    p.add_argument("paths", nargs="+",
                   help="Sprite directory paths (or glob patterns).")
    p.add_argument("--target", type=int, default=1500,
                   help="QEM target triangle count (default 1500).")
    p.add_argument("--prepass-res", type=int, default=96,
                   help="Voxel resolution for the watertightening pass "
                        "(default 96). Higher = smoother input to QEM but "
                        "slower and more memory.")
    p.add_argument("--fallback-res", type=int, default=48,
                   help="Voxel resolution for the shatter-fallback "
                        "(default 48).")
    args = p.parse_args()

    dirs = resolve_sprite_dirs(args.paths, required_file="mesh.glb")
    if not dirs:
        print("no sprite dirs found with mesh.glb")
        sys.exit(1)

    ok = sum(process(d, args.target, args.prepass_res, args.fallback_res)
             for d in dirs)
    print(f"\nDone: {ok}/{len(dirs)} sprites processed.")
    sys.exit(0 if ok == len(dirs) else 1)


if __name__ == "__main__":
    main()
