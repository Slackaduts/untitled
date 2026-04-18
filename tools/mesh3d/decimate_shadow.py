#!/usr/bin/env python3
"""Generate low-poly watertight shadow meshes from mesh.glb.

For each sprite directory containing mesh.glb (the full Hunyuan
reconstruction):

  1. Load and clean the raw mesh (weld near-duplicate vertices, drop
     degenerate and duplicate faces, remove unreferenced vertices,
     reconcile winding).
  2. Sample a dense oriented point cloud from the surface.
  3. Screened Poisson surface reconstruction (open3d) — fits an implicit
     function to the points and extracts a smooth isosurface. Always
     manifold; preserves shape better than voxel remeshing because it
     doesn't grid-align.
  4. open3d QEM decimation to --target triangles. Open3d's QEM is more
     conservative about topology than fast-simplification's wrapper.
  5. PyMeshFix repair pass — finds every boundary edge and stitches it via
     constrained triangulation. Iterates up to --repair-attempts times.
     Required for high-genus shapes (trees, foliage) where Poisson alone
     can't get a closed isosurface.
  6. Write shadow.glb.

Each output line ends with `[watertight]` or `[OPEN boundary=N non_manifold=M]`
so you can see exactly why anything that's still open isn't closed.

Dependencies:
    pip install trimesh fast-simplification open3d pymeshfix numpy

Usage:
    python decimate_shadow.py [--target N] [--depth D] [--samples S] sprite_dir...
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
    """Best-effort topology cleanup before Poisson sampling."""
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


def poisson_and_decimate(mesh, depth: int, samples: int, target_faces: int):
    """Screened Poisson reconstruction + open3d QEM. Both inside open3d to
    avoid the trimesh round-trip and fast-simplification's looser topology.
    No AABB crop — the crop cuts triangles and produces boundary edges; we
    accept slight silhouette inflation for strict manifoldness."""
    import open3d as o3d
    import trimesh

    points, face_idx = trimesh.sample.sample_surface(mesh, count=samples)
    normals = mesh.face_normals[face_idx]

    pcd = o3d.geometry.PointCloud()
    pcd.points = o3d.utility.Vector3dVector(np.asarray(points))
    pcd.normals = o3d.utility.Vector3dVector(np.asarray(normals))

    with o3d.utility.VerbosityContextManager(o3d.utility.VerbosityLevel.Error):
        o3d_mesh, _densities = (
            o3d.geometry.TriangleMesh.create_from_point_cloud_poisson(
                pcd, depth=depth, width=0, scale=1.1, linear_fit=False
            )
        )

    poisson_faces = len(o3d_mesh.triangles)

    o3d_mesh = o3d_mesh.simplify_quadric_decimation(
        target_number_of_triangles=target_faces
    )
    o3d_mesh.remove_duplicated_vertices()
    o3d_mesh.remove_duplicated_triangles()
    o3d_mesh.remove_degenerate_triangles()
    o3d_mesh.remove_unreferenced_vertices()

    out = trimesh.Trimesh(
        vertices=np.asarray(o3d_mesh.vertices),
        faces=np.asarray(o3d_mesh.triangles),
        process=False,
    )
    return out, poisson_faces


def force_watertight(mesh, max_attempts: int = 3):
    """MeshFix-based repair using the low-level PyTMesh API.

    `fill_small_boundaries(nbe=0)` patches every boundary regardless of
    vertex count. We deliberately do NOT call `tin.clean()` — that step
    iteratively deletes self-intersecting triangles, and on Poisson output
    of high-genus shapes (trees) the cascade can wipe out most of the mesh.
    Self-intersections don't leak light through the shadow caster; holes do.
    So we tolerate intersections, fix only the holes.

    `process=True` on the trimesh constructor merges spatially-duplicate
    vertices that pymeshfix leaves behind — without it, trimesh's
    `is_watertight` edge-counting reports false boundaries.

    If a repair attempt collapses the mesh below 10% of its input face
    count, we keep the pre-repair version (better OPEN than empty).
    """
    import pymeshfix
    import trimesh

    pre_faces = len(mesh.faces)
    best = mesh
    for _ in range(max_attempts):
        tin = pymeshfix.PyTMesh()
        tin.load_array(
            np.ascontiguousarray(best.vertices, dtype=np.float64),
            np.ascontiguousarray(best.faces, dtype=np.int32),
        )
        tin.fill_small_boundaries(nbe=0, refine=True)
        v, f = tin.return_arrays()

        candidate = trimesh.Trimesh(
            vertices=np.asarray(v),
            faces=np.asarray(f),
            process=True,
        )
        # Bail if repair destroyed the mesh.
        if len(candidate.faces) < max(4, pre_faces * 0.1):
            return best

        best = candidate
        if best.is_watertight:
            return best

    return best


def open_edge_diagnostic(mesh):
    """For an OPEN mesh, count boundary and non-manifold edges to see which
    kind of failure we're hitting."""
    edges_sorted = mesh.edges_sorted
    _unique, counts = np.unique(edges_sorted, axis=0, return_counts=True)
    boundary = int((counts == 1).sum())
    non_manifold = int((counts > 2).sum())
    return f"boundary={boundary} non_manifold={non_manifold}"


def process(sprite_dir: Path, target_faces: int,
            depth: int, samples: int, repair_attempts: int) -> bool:
    mesh_path = sprite_dir / "mesh.glb"
    shadow_path = sprite_dir / "shadow.glb"

    raw = load_concatenated(mesh_path)
    if raw is None:
        print(f"FAIL {sprite_dir.name}: no geometry in mesh.glb")
        return False

    orig_faces = len(raw.faces)
    raw = clean(raw)
    cleaned_faces = len(raw.faces)
    if cleaned_faces < 4:
        print(f"FAIL {sprite_dir.name}: nothing left after cleaning")
        return False

    try:
        decimated, poisson_faces = poisson_and_decimate(
            raw, depth=depth, samples=samples, target_faces=target_faces
        )
    except Exception as e:
        print(f"FAIL {sprite_dir.name}: {e}")
        return False

    if len(decimated.faces) < 4:
        print(f"FAIL {sprite_dir.name}: empty after decimation")
        return False

    # Always run pymeshfix as the final pass — even when decimation produces
    # a closed mesh, the repair is a no-op and cheap; when it doesn't, this
    # is the step that actually closes high-genus shapes.
    try:
        decimated = force_watertight(decimated, max_attempts=repair_attempts)
    except Exception as e:
        print(f"  {sprite_dir.name}: pymeshfix raised {e}; saving as-is")

    if decimated.is_watertight:
        tag = "[watertight]"
    else:
        tag = f"[OPEN {open_edge_diagnostic(decimated)}]"

    out_faces = len(decimated.faces)
    decimated.export(str(shadow_path), file_type="glb")
    print(f"OK   {sprite_dir.name}: {orig_faces} → cleaned {cleaned_faces} "
          f"→ poisson {poisson_faces} → fix {out_faces} faces {tag}")
    return True


def main():
    p = argparse.ArgumentParser(
        description="Decimate Hunyuan mesh.glb into a watertight low-poly shadow.glb "
                    "via Poisson reconstruction + QEM + PyMeshFix repair.")
    p.add_argument("paths", nargs="+",
                   help="Sprite directory paths (or glob patterns).")
    p.add_argument("--target", type=int, default=1500,
                   help="QEM target triangle count (default 1500).")
    p.add_argument("--depth", type=int, default=8,
                   help="Poisson octree depth (default 8). 6=blobby, 10=fine.")
    p.add_argument("--samples", type=int, default=60000,
                   help="Surface points sampled before Poisson (default 60000).")
    p.add_argument("--repair-attempts", type=int, default=3,
                   help="Max iterations of pymeshfix repair (default 3).")
    args = p.parse_args()

    dirs = resolve_sprite_dirs(args.paths, required_file="mesh.glb")
    if not dirs:
        print("no sprite dirs found with mesh.glb")
        sys.exit(1)

    ok = sum(process(d, args.target, args.depth, args.samples,
                     args.repair_attempts)
             for d in dirs)
    print(f"\nDone: {ok}/{len(dirs)} sprites processed.")
    sys.exit(0 if ok == len(dirs) else 1)


if __name__ == "__main__":
    main()
