#!/usr/bin/env python3
"""Generate low-poly shadow meshes by cleaning + QEM-decimating mesh.glb.

For each sprite directory containing mesh.glb (the full Hunyuan
reconstruction), this script:
  1. Loads the mesh and welds near-duplicate vertices, removes degenerate
     and duplicate faces, drops unreferenced vertices, and reconciles
     winding. Hunyuan output is non-manifold (open edges, self-intersections)
     and aggressive QEM on raw output produces shattered geometry — the
     cleaning pass makes the input something QEM can actually handle.
  2. Runs quadric-error decimation to a target triangle count.
  3. Sanity-checks the output. If it looks shattered (too few triangles, or
     fragmented into many disconnected components), falls back to voxel-
     remeshing, which always produces a clean manifold.
  4. Writes the result to shadow.glb (overwriting whatever's there).

Dependencies:
    pip install trimesh fast-simplification numpy

Usage:
    python decimate_shadow.py [--target N] [--voxel-res R] sprite_dir...

    --target sets the QEM target triangle count (default 1500).
    --voxel-res sets the fallback voxel grid resolution (default 48).
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


def process(sprite_dir: Path, target_faces: int, voxel_res: int) -> bool:
    mesh_path = sprite_dir / "mesh.glb"
    shadow_path = sprite_dir / "shadow.glb"

    mesh = load_concatenated(mesh_path)
    if mesh is None:
        print(f"FAIL {sprite_dir.name}: no geometry in mesh.glb")
        return False

    orig_faces = len(mesh.faces)
    mesh = clean(mesh)
    cleaned_faces = len(mesh.faces)

    try:
        decimated = decimate_qem(mesh, target_faces)
        method = "qem"
    except Exception as e:
        print(f"  {sprite_dir.name}: QEM raised {e}; using voxel fallback")
        decimated = voxel_remesh(mesh, voxel_res)
        method = "voxel"

    if method == "qem" and looks_shattered(decimated, target_faces):
        print(f"  {sprite_dir.name}: QEM output looks shattered "
              f"({len(decimated.faces)} faces); using voxel fallback")
        decimated = voxel_remesh(mesh, voxel_res)
        method = "voxel"

    out_faces = len(decimated.faces)
    decimated.export(str(shadow_path), file_type="glb")
    print(f"OK   {sprite_dir.name}: {orig_faces} → cleaned {cleaned_faces} "
          f"→ {method} {out_faces} faces")
    return True


def main():
    p = argparse.ArgumentParser(
        description="Decimate Hunyuan mesh.glb into a low-poly shadow.glb.")
    p.add_argument("paths", nargs="+",
                   help="Sprite directory paths (or glob patterns).")
    p.add_argument("--target", type=int, default=1500,
                   help="QEM target triangle count (default 1500).")
    p.add_argument("--voxel-res", type=int, default=48,
                   help="Fallback voxel grid resolution (default 48).")
    args = p.parse_args()

    dirs = resolve_sprite_dirs(args.paths, required_file="mesh.glb")
    if not dirs:
        print("no sprite dirs found with mesh.glb")
        sys.exit(1)

    ok = sum(process(d, args.target, args.voxel_res) for d in dirs)
    print(f"\nDone: {ok}/{len(dirs)} sprites processed.")
    sys.exit(0 if ok == len(dirs) else 1)


if __name__ == "__main__":
    main()
