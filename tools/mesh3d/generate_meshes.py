#!/usr/bin/env python3
"""Generate voxelized 3D meshes from 2D billboard sprites using Hunyuan3D-2.

Pipeline:
  1. Shape generation (Hunyuan3D DiT) → high-poly mesh
  2. Voxelize at sprite pixel resolution → voxel grid
  3. Color each voxel from the original sprite (XY projection)
  4. Greedy mesh → merge coplanar same-color faces
  5. Export as GLB

Usage:
    python generate_meshes.py [--force] [--steps N] [path...]

Output: <input_stem>_mesh.glb alongside each input sprite.
"""

import argparse
import sys
import os
from pathlib import Path

import numpy as np
from PIL import Image


def load_shape_model(device="cuda"):
    """Load the Hunyuan3D-2 shape generation model."""
    try:
        from hy3dgen.shapegen.pipelines import Hunyuan3DDiTFlowMatchingPipeline
    except ImportError:
        print("ERROR: Hunyuan3D-2 not installed.")
        sys.exit(1)

    print(f"Loading shape model on {device}...")
    pipeline = Hunyuan3DDiTFlowMatchingPipeline.from_pretrained(
        "tencent/Hunyuan3D-2",
        subfolder="hunyuan3d-dit-v2-0",
        use_safetensors=True,
        device=device,
    )
    print("Shape model loaded.")
    return pipeline


def remove_background(image: Image.Image) -> Image.Image:
    """Remove background from image using rembg."""
    try:
        from rembg import remove
        return remove(image)
    except ImportError:
        print("WARNING: rembg not installed, using image as-is")
        return image


def upscale_for_model(image: Image.Image, target_size=768) -> Image.Image:
    """Upscale small pixel art sprites for better model input."""
    w, h = image.size
    if max(w, h) >= target_size:
        return image
    scale = target_size // max(w, h)
    if scale < 2:
        scale = 2
    new_w, new_h = w * scale, h * scale
    upscaled = image.resize((new_w, new_h), Image.NEAREST)
    print(f"  Upscaled {w}x{h} -> {new_w}x{new_h}")
    return upscaled


def voxelize_and_color(mesh, sprite: Image.Image, voxel_scale=1.0, flip_uv=False,
                       blend_trim_px=0.0):
    """Convert a mesh to colored voxels at sprite pixel resolution.

    1. Determine voxel pitch so that the front face spans sprite_width × sprite_height voxels
    2. Voxelize the mesh
    3. Color each voxel by sampling the sprite at the corresponding XY position
    4. Convert to a greedy-meshed triangle mesh with vertex colors

    Args:
        mesh: trimesh.Trimesh or Scene from Hunyuan3D
        sprite: Original sprite image (RGBA)
        voxel_scale: Multiplier for voxel resolution (1.0 = 1 voxel per sprite pixel)
    """
    import trimesh

    # Extract single trimesh
    if isinstance(mesh, trimesh.Scene):
        geoms = [g for g in mesh.geometry.values() if isinstance(g, trimesh.Trimesh)]
        if not geoms:
            print("  WARNING: No geometry found for voxelization")
            return mesh, 2
        tm = trimesh.util.concatenate(geoms) if len(geoms) > 1 else geoms[0]
    elif isinstance(mesh, trimesh.Trimesh):
        tm = mesh
    elif isinstance(mesh, (list, tuple)):
        tms = [m for m in mesh if isinstance(m, trimesh.Trimesh)]
        if not tms:
            print("  WARNING: No trimesh found")
            return mesh, 2
        tm = trimesh.util.concatenate(tms) if len(tms) > 1 else tms[0]
    else:
        print(f"  WARNING: Unknown mesh type {type(mesh)}")
        return mesh, 2

    # Auto-detect axes
    # Hunyuan3D consistently outputs Y-up meshes. The thinnest axis is depth (Z typically).
    # U = width (X), V = height (Y).
    verts = tm.vertices
    ranges = verts.max(axis=0) - verts.min(axis=0)
    depth_axis = int(np.argmin(ranges))
    axes = [i for i in range(3) if i != depth_axis]

    # Y (axis 1) is always height/V in Hunyuan3D output
    if 1 in axes:
        v_axis = 1
        u_axis = axes[0] if axes[0] != 1 else axes[1]
    else:
        # Depth is Y — fall back to aspect ratio matching
        sprite_aspect = sprite.width / sprite.height
        r0, r1 = ranges[axes[0]], ranges[axes[1]]
        mesh_aspect = r0 / r1 if r1 > 0 else 1.0
        if abs(mesh_aspect - sprite_aspect) > abs((r1 / r0 if r0 > 0 else 1.0) - sprite_aspect):
            u_axis, v_axis = axes[1], axes[0]
        else:
            u_axis, v_axis = axes[0], axes[1]

    if flip_uv:
        u_axis, v_axis = v_axis, u_axis

    axis_names = ['X', 'Y', 'Z']
    print(f"  Axes: depth={axis_names[depth_axis]}, "
          f"U={axis_names[u_axis]}, V={axis_names[v_axis]}"
          f"{' (flipped)' if flip_uv else ''}")

    # Calculate voxel pitch: map the sprite's pixel dimensions to the mesh's world dimensions
    u_range = ranges[u_axis]
    v_range = ranges[v_axis]
    # Use the larger dimension to set pitch, ensuring sprite pixels map 1:1 to voxels
    pitch_u = u_range / (sprite.width * voxel_scale) if sprite.width > 0 else 0.01
    pitch_v = v_range / (sprite.height * voxel_scale) if sprite.height > 0 else 0.01
    pitch = min(pitch_u, pitch_v)  # use finer resolution
    if pitch < 1e-6:
        pitch = 0.01

    print(f"  Voxelizing at pitch={pitch:.5f} "
          f"(~{int(u_range/pitch)}x{int(v_range/pitch)}x{int(ranges[depth_axis]/pitch)} voxels)")

    # Voxelize surface shell
    voxel_grid = tm.voxelized(pitch)
    filled = voxel_grid.matrix.copy()
    origin = voxel_grid.transform[:3, 3]
    grid_shape = filled.shape

    # Fill interior: for each column along the depth axis, fill between
    # the first and last filled voxel. This handles non-watertight meshes.
    depth_grid_axis = depth_axis  # axis index in the voxel grid
    # Iterate over the two non-depth axes
    other_axes = [i for i in range(3) if i != depth_grid_axis]
    a0, a1 = other_axes

    filled_before = np.sum(filled)
    for i in range(grid_shape[a0]):
        for j in range(grid_shape[a1]):
            # Extract the column along depth axis
            slicing = [slice(None)] * 3
            slicing[a0] = i
            slicing[a1] = j
            col = filled[tuple(slicing)]

            # Find first and last filled voxel in this column
            indices = np.where(col)[0]
            if len(indices) >= 2:
                col[indices[0]:indices[-1] + 1] = True
                filled[tuple(slicing)] = col

    print(f"  Voxel grid: {grid_shape[0]}x{grid_shape[1]}x{grid_shape[2]} "
          f"({filled_before} shell -> {np.sum(filled)} solid)")

    if np.sum(filled) == 0:
        print("  WARNING: No voxels filled")
        return tm, depth_axis

    # Prepare sprite for sampling
    sprite_arr = np.array(sprite.convert("RGBA"))
    u_min = verts[:, u_axis].min()
    v_min = verts[:, v_axis].min()

    # ── Enforce pixel-perfect front silhouette ──
    # Iterate over voxel grid positions, sample sprite to decide which columns exist.
    # For missing columns, ray-cast into the ORIGINAL mesh to find the correct depth.
    grid_pitch = voxel_grid.transform[0, 0]
    grid_origin = voxel_grid.transform[:3, 3]
    sprite_alpha = sprite_arr[:, :, 3]
    min_depth_voxels = max(1, int(grid_shape[depth_axis] * 0.1))

    # Pre-compute ray direction along depth axis
    ray_dir = np.zeros(3)
    ray_dir[depth_axis] = 1.0
    depth_min = verts[:, depth_axis].min()
    depth_max = verts[:, depth_axis].max()

    for gi_u in range(grid_shape[u_axis]):
        for gi_v in range(grid_shape[v_axis]):
            world_u = grid_origin[u_axis] + (gi_u + 0.5) * grid_pitch
            world_v = grid_origin[v_axis] + (gi_v + 0.5) * grid_pitch

            nu = (world_u - u_min) / u_range if u_range > 0 else 0.5
            nv = (world_v - v_min) / v_range if v_range > 0 else 0.5
            sx = int(np.clip(nu * (sprite.width - 1), 0, sprite.width - 1))
            sy = int(np.clip((1.0 - nv) * (sprite.height - 1), 0, sprite.height - 1))

            slicing = [slice(None)] * 3
            slicing[u_axis] = gi_u
            slicing[v_axis] = gi_v
            col = filled[tuple(slicing)]

            if sprite_alpha[sy, sx] >= 32:
                if not np.any(col):
                    # Ray-cast through original mesh at this UV position
                    ray_origin = np.zeros(3)
                    ray_origin[u_axis] = world_u
                    ray_origin[v_axis] = world_v
                    ray_origin[depth_axis] = depth_min - grid_pitch

                    hits, _, _ = tm.ray.intersects_location(
                        [ray_origin], [ray_dir]
                    )

                    if len(hits) > 0:
                        # Fill between first and last hit
                        hit_depths = hits[:, depth_axis]
                        d_front = hit_depths.min()
                        d_back = hit_depths.max()
                        gi_front = int((d_front - grid_origin[depth_axis]) / grid_pitch)
                        gi_back = int((d_back - grid_origin[depth_axis]) / grid_pitch)
                        gi_front = max(0, gi_front)
                        gi_back = min(len(col) - 1, gi_back)
                        if gi_back < gi_front:
                            gi_back = gi_front
                        col[gi_front:gi_back + 1] = True
                    else:
                        # No ray hits — use neighbor average as fallback
                        nb_fronts = []
                        nb_backs = []
                        for du, dv in [(-1,0),(1,0),(0,-1),(0,1)]:
                            niu, niv = gi_u + du, gi_v + dv
                            if 0 <= niu < grid_shape[u_axis] and 0 <= niv < grid_shape[v_axis]:
                                nb_s = [slice(None)] * 3
                                nb_s[u_axis] = niu
                                nb_s[v_axis] = niv
                                nb_col = filled[tuple(nb_s)]
                                nb_idx = np.where(nb_col)[0]
                                if len(nb_idx) > 0:
                                    nb_fronts.append(nb_idx[0])
                                    nb_backs.append(nb_idx[-1])
                        if nb_fronts:
                            f = int(np.mean(nb_fronts))
                            b = int(np.mean(nb_backs))
                            col[f:b + 1] = True
                        else:
                            mid = len(col) // 2
                            half = min_depth_voxels // 2
                            col[max(0, mid - half):min(len(col), mid + half + 1)] = True
                    filled[tuple(slicing)] = col
            else:
                col[:] = False
                filled[tuple(slicing)] = col

    print(f"  After silhouette enforcement: {np.sum(filled)} filled voxels")

    # Pre-compute nearest opaque pixel lookup for coloring voxels
    # that map to transparent sprite areas
    from scipy.ndimage import distance_transform_edt
    opaque_mask = sprite_alpha >= 32
    if not np.all(opaque_mask):
        _, nearest_idx = distance_transform_edt(~opaque_mask, return_indices=True)
        nearest_color_map = sprite_arr[nearest_idx[0], nearest_idx[1]]
    else:
        nearest_color_map = sprite_arr

    # Build voxel mesh with per-face coloring:
    # - Front/back (depth axis): sprite color at XY
    # - Sides (u/v axis): sprite color darkened 25% for natural shading
    all_verts = []
    all_faces = []
    all_colors = []
    vert_offset = 0

    directions = [
        (1, 0, 0), (-1, 0, 0),
        (0, 1, 0), (0, -1, 0),
        (0, 0, 1), (0, 0, -1),
    ]
    face_quads = {
        (1, 0, 0): [(1,0,0), (1,1,0), (1,1,1), (1,0,1)],
        (-1, 0, 0): [(0,0,1), (0,1,1), (0,1,0), (0,0,0)],
        (0, 1, 0): [(0,1,0), (0,1,1), (1,1,1), (1,1,0)],
        (0, -1, 0): [(1,0,0), (1,0,1), (0,0,1), (0,0,0)],
        (0, 0, 1): [(0,0,1), (0,1,1), (1,1,1), (1,0,1)],
        (0, 0, -1): [(1,0,0), (0,0,0), (0,1,0), (1,1,0)],
    }

    def darken(color, factor=0.75):
        """Darken RGB by factor, preserve alpha."""
        return np.array([
            int(color[0] * factor),
            int(color[1] * factor),
            int(color[2] * factor),
            color[3]
        ], dtype=np.uint8)

    for ix in range(grid_shape[0]):
        for iy in range(grid_shape[1]):
            for iz in range(grid_shape[2]):
                if not filled[ix, iy, iz]:
                    continue

                world_pos = origin + np.array([ix + 0.5, iy + 0.5, iz + 0.5]) * pitch

                su = (world_pos[u_axis] - u_min) / (u_range if u_range > 0 else 1.0)
                sv = 1.0 - (world_pos[v_axis] - v_min) / (v_range if v_range > 0 else 1.0)
                sx = int(np.clip(su * (sprite.width - 1), 0, sprite.width - 1))
                sy = int(np.clip(sv * (sprite.height - 1), 0, sprite.height - 1))

                # Skip bottom rows corresponding to blend_height (ground blend zone)
                if blend_trim_px > 0:
                    pixels_from_bottom = sprite.height - 1 - sy
                    if pixels_from_bottom < blend_trim_px:
                        continue

                front_color = sprite_arr[sy, sx]

                if front_color[3] < 32:
                    front_color = nearest_color_map[sy, sx]
                    if front_color[3] < 32:
                        continue

                side_color = darken(front_color, 0.75)
                top_bottom_color = darken(front_color, 0.85)

                for dx, dy, dz in directions:
                    nx, ny, nz = ix + dx, iy + dy, iz + dz
                    if (nx < 0 or nx >= grid_shape[0] or
                        ny < 0 or ny >= grid_shape[1] or
                        nz < 0 or nz >= grid_shape[2] or
                        not filled[nx, ny, nz]):

                        face_dir = [dx, dy, dz]
                        grid_dir_axis = next(i for i in range(3) if face_dir[i] != 0)

                        if grid_dir_axis == depth_axis:
                            color = front_color  # front AND back get sprite color
                        elif grid_dir_axis == v_axis:
                            color = top_bottom_color
                        else:
                            color = side_color

                        quad = face_quads[(dx, dy, dz)]
                        face_verts = [
                            origin + np.array([ix + qx, iy + qy, iz + qz]) * pitch
                            for qx, qy, qz in quad
                        ]

                        vi = vert_offset
                        all_verts.extend(face_verts)
                        all_faces.append([vi, vi + 1, vi + 2])
                        all_faces.append([vi, vi + 2, vi + 3])
                        all_colors.extend([color] * 4)
                        vert_offset += 4

    if not all_verts:
        print("  WARNING: No visible voxel faces")
        return tm, depth_axis

    verts_arr = np.array(all_verts, dtype=np.float64)
    faces_arr = np.array(all_faces, dtype=np.int64)
    colors_arr = np.array(all_colors, dtype=np.uint8)

    result = trimesh.Trimesh(
        vertices=verts_arr,
        faces=faces_arr,
        vertex_colors=colors_arr,
        process=False,
    )

    print(f"  Voxel mesh: {len(result.vertices)} verts, {len(result.faces)} faces")
    return result, depth_axis


_clip_model = None
_clip_processor = None

def load_clip():
    """Load CLIP model for sprite classification."""
    global _clip_model, _clip_processor
    if _clip_model is not None:
        return _clip_model, _clip_processor

    import torch
    from transformers import CLIPProcessor, CLIPModel

    print("Loading CLIP for sprite classification...")
    model_name = "openai/clip-vit-base-patch32"
    _clip_processor = CLIPProcessor.from_pretrained(model_name)
    _clip_model = CLIPModel.from_pretrained(model_name)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    _clip_model = _clip_model.to(device)
    print(f"CLIP loaded on {device}")
    return _clip_model, _clip_processor


def classify_sprite(sprite_path: Path) -> str:
    """Classify a sprite as 'structural' or 'organic' using CLIP.

    Compares the sprite against semantic labels to determine if it's
    a natural/organic object (tree, bush, flower) or a structural one
    (building, wall, fence, furniture).
    """
    import torch

    model, processor = load_clip()
    device = next(model.parameters()).device

    sprite = Image.open(sprite_path).convert("RGB")

    organic_labels = [
        "a tree", "a bush", "foliage", "a plant", "leaves",
        "grass", "flowers", "vines", "a forest",
    ]
    structural_labels = [
        "a building", "a house", "a wall", "a fence", "a door",
        "a roof", "furniture", "a bridge", "a tower", "a sign",
        "a barrel", "a crate", "a chest", "a lamp post",
    ]

    all_labels = organic_labels + structural_labels
    inputs = processor(
        text=all_labels,
        images=sprite,
        return_tensors="pt",
        padding=True,
    )
    inputs = {k: v.to(device) for k, v in inputs.items()}

    with torch.no_grad():
        outputs = model(**inputs)
        logits = outputs.logits_per_image[0]
        probs = logits.softmax(dim=0).cpu().numpy()

    # Sum probabilities for each category
    organic_score = sum(probs[i] for i in range(len(organic_labels)))
    structural_score = sum(probs[i] for i in range(len(organic_labels), len(all_labels)))

    # Find top label for logging
    top_idx = probs.argmax()
    top_label = all_labels[top_idx]
    top_prob = probs[top_idx]

    result = "structural" if structural_score > organic_score else "organic"
    print(f"  CLIP: {result} (organic={organic_score:.2f}, structural={structural_score:.2f}, "
          f"top='{top_label}' {top_prob:.2f})")
    return result


def trim_mesh_bottom(mesh, blend_height_px: float, sprite_height_px: float):
    """Remove the bottom portion of a mesh corresponding to blend_height.

    blend_height_px is in sprite pixels. We convert to a fraction of the
    mesh's total height and slice off vertices below that threshold.
    """
    import trimesh

    if not isinstance(mesh, trimesh.Trimesh) or len(mesh.vertices) == 0:
        return mesh

    # Y axis is height in Hunyuan3D meshes
    y_min = mesh.vertices[:, 1].min()
    y_max = mesh.vertices[:, 1].max()
    y_range = y_max - y_min
    if y_range <= 0:
        return mesh

    # Convert blend_height from sprite pixels to mesh units
    blend_fraction = blend_height_px / sprite_height_px
    cut_y = y_min + blend_fraction * y_range

    # Remove faces where ALL vertices are below the cut
    keep_faces = []
    for fi, face in enumerate(mesh.faces):
        face_y_max = mesh.vertices[face, 1].max()
        if face_y_max > cut_y:
            keep_faces.append(fi)

    if len(keep_faces) == len(mesh.faces):
        return mesh  # nothing to trim

    if not keep_faces:
        print(f"  WARNING: blend trim would remove entire mesh, skipping")
        return mesh

    trimmed = mesh.submesh([keep_faces], append=True)
    removed = len(mesh.faces) - len(trimmed.faces)
    print(f"  Trimmed {removed} faces from bottom ({blend_fraction:.0%} of height)")
    return trimmed


def process_sprite(shape_pipeline, input_path: Path, steps=50):
    """Process a single sprite: generate shadow.glb for shadow casting.

    Layout: <tileset>/<sprite_key>/
        sprite.png        ← input (already exists)
        shadow.glb         ← AI mesh for shadow casting
    """
    sprite_dir = input_path.parent
    sprite_name = sprite_dir.name
    print(f"Processing: {sprite_name}")
    import trimesh

    # Read blend_height for shadow bottom trimming
    blend_height = 0.0
    # Check properties.json first
    props_path = sprite_dir / "properties.json"
    if props_path.exists():
        try:
            import json
            props = json.loads(props_path.read_text())
            blend_height = props.get("blend_height", 0.0)
        except Exception:
            pass
    # Fall back to blend_height.txt
    if blend_height <= 0.0:
        bh_file = sprite_dir / "blend_height.txt"
        if bh_file.exists():
            try:
                blend_height = float(bh_file.read_text().strip())
            except ValueError:
                pass
    if blend_height > 0.0:
        print(f"  Blend height: {blend_height}px (bottom trimmed from shadow)")

    original_image = Image.open(input_path).convert("RGBA")
    image = upscale_for_model(original_image.copy())

    alpha = np.array(image)[:, :, 3]
    if alpha.min() > 200:
        print("  Removing background...")
        image = remove_background(image)

    print(f"  Generating shape ({steps} steps)...")
    raw_mesh = shape_pipeline(
        image=image,
        num_inference_steps=steps,
        guidance_scale=7.5,
        octree_resolution=384,
    )

    if isinstance(raw_mesh, (list, tuple)):
        raw_mesh = raw_mesh[0] if len(raw_mesh) == 1 else raw_mesh

    # Convert to trimesh
    shadow_mesh = raw_mesh
    if not isinstance(shadow_mesh, (trimesh.Scene, trimesh.Trimesh)):
        shadow_mesh = trimesh.Scene(shadow_mesh)
    if isinstance(shadow_mesh, trimesh.Scene):
        geoms = [g for g in shadow_mesh.geometry.values() if isinstance(g, trimesh.Trimesh)]
        if geoms:
            shadow_mesh = trimesh.util.concatenate(geoms) if len(geoms) > 1 else geoms[0]

    # Trim bottom for blend_height
    if blend_height > 0.0 and isinstance(shadow_mesh, trimesh.Trimesh):
        shadow_mesh = trim_mesh_bottom(shadow_mesh, blend_height, original_image.height)

    # Export
    shadow_path = sprite_dir / "shadow.glb"
    if isinstance(shadow_mesh, trimesh.Trimesh):
        shadow_mesh.export(str(shadow_path), file_type='glb')
    else:
        trimesh.Scene(shadow_mesh).export(str(shadow_path), file_type='glb')
    print(f"  Saved: shadow.glb")


def extract_depth_profile(sprite_path: Path):
    """Extract a per-pixel depth profile from an existing GLB mesh.

    For each opaque pixel of the sprite, casts a ray along the mesh's depth
    axis to find the front surface. The result is a depth map where each
    pixel's value represents how far the mesh protrudes from the billboard
    plane (0 = flat, 255 = maximum protrusion).

    Outputs:
        depth.png  — single-channel depth map (same resolution as sprite)
        max_depth.txt — the mesh's depth extent in Hunyuan mesh units
        axes.txt   — depth axis identifier (e.g. "depth=Z")
    """
    import trimesh

    sprite_dir = sprite_path.parent
    sprite_name = sprite_dir.name

    # Find the mesh file
    shadow_glb = sprite_dir / "shadow.glb"
    mesh_glb = sprite_dir / "mesh.glb"
    if shadow_glb.exists():
        mesh_path = shadow_glb
    elif mesh_glb.exists():
        mesh_path = mesh_glb
    else:
        print(f"  SKIP {sprite_name}: no shadow.glb or mesh.glb")
        return

    print(f"  Extracting depth profile for {sprite_name}...")

    # Load mesh
    loaded = trimesh.load(str(mesh_path), force='mesh')
    if isinstance(loaded, trimesh.Scene):
        geoms = [g for g in loaded.geometry.values() if isinstance(g, trimesh.Trimesh)]
        if not geoms:
            print(f"  WARNING: No geometry in {mesh_path.name}")
            return
        tm = trimesh.util.concatenate(geoms) if len(geoms) > 1 else geoms[0]
    elif isinstance(loaded, trimesh.Trimesh):
        tm = loaded
    else:
        print(f"  WARNING: Unknown mesh type {type(loaded)}")
        return

    # Load sprite
    sprite = Image.open(sprite_path).convert("RGBA")
    sprite_arr = np.array(sprite)
    sprite_alpha = sprite_arr[:, :, 3]

    # Detect axes (same logic as voxelize_and_color)
    verts = tm.vertices
    ranges = verts.max(axis=0) - verts.min(axis=0)
    depth_axis = int(np.argmin(ranges))
    axes = [i for i in range(3) if i != depth_axis]

    if 1 in axes:
        v_axis = 1
        u_axis = axes[0] if axes[0] != 1 else axes[1]
    else:
        sprite_aspect = sprite.width / sprite.height
        r0, r1 = ranges[axes[0]], ranges[axes[1]]
        mesh_aspect = r0 / r1 if r1 > 0 else 1.0
        if abs(mesh_aspect - sprite_aspect) > abs((r1 / r0 if r0 > 0 else 1.0) - sprite_aspect):
            u_axis, v_axis = axes[1], axes[0]
        else:
            u_axis, v_axis = axes[0], axes[1]

    axis_names = ['X', 'Y', 'Z']
    print(f"    Axes: depth={axis_names[depth_axis]}, "
          f"U={axis_names[u_axis]}, V={axis_names[v_axis]}")

    u_min = verts[:, u_axis].min()
    u_range = ranges[u_axis]
    v_min = verts[:, v_axis].min()
    v_range = ranges[v_axis]
    depth_min = verts[:, depth_axis].min()
    depth_max = verts[:, depth_axis].max()
    depth_range = depth_max - depth_min

    if depth_range <= 0:
        print(f"  WARNING: zero depth range")
        return

    # Ray direction: shoot from front (depth_min) into the mesh
    ray_dir = np.zeros(3)
    ray_dir[depth_axis] = 1.0

    # Build depth map: for each sprite pixel, cast a ray and find the
    # front surface (closest hit along the depth axis).
    depth_map = np.zeros((sprite.height, sprite.width), dtype=np.float32)

    # Batch all rays for efficiency
    ray_origins = []
    pixel_coords = []  # (sy, sx) for each ray

    for sy in range(sprite.height):
        for sx in range(sprite.width):
            if sprite_alpha[sy, sx] < 32:
                continue

            # Map pixel to mesh world coordinates
            nu = (sx + 0.5) / sprite.width
            nv = 1.0 - (sy + 0.5) / sprite.height  # flip Y: image top = mesh top
            world_u = u_min + nu * u_range
            world_v = v_min + nv * v_range

            origin = np.zeros(3)
            origin[u_axis] = world_u
            origin[v_axis] = world_v
            origin[depth_axis] = depth_min - 0.01

            ray_origins.append(origin)
            pixel_coords.append((sy, sx))

    if not ray_origins:
        print(f"  WARNING: no opaque pixels")
        return

    ray_origins = np.array(ray_origins)
    ray_dirs = np.tile(ray_dir, (len(ray_origins), 1))

    # Batch ray-cast
    hits, ray_indices, _ = tm.ray.intersects_location(ray_origins, ray_dirs)

    if len(hits) > 0:
        # For each ray, find the furthest hit (maximum protrusion from billboard plane)
        hit_depths = hits[:, depth_axis]
        for i in range(len(ray_origins)):
            mask = ray_indices == i
            if not np.any(mask):
                continue
            # Maximum depth hit = furthest protrusion from the billboard plane
            max_hit = hit_depths[mask].max()
            protrusion = (max_hit - depth_min) / depth_range
            sy, sx = pixel_coords[i]
            depth_map[sy, sx] = np.clip(protrusion, 0.0, 1.0)

    # Convert to 8-bit image
    depth_u8 = (depth_map * 255.0).astype(np.uint8)
    depth_img = Image.fromarray(depth_u8, mode='L')
    depth_out = sprite_dir / "depth.png"
    depth_img.save(str(depth_out))

    # Write max depth in mesh units
    max_depth_path = sprite_dir / "max_depth.txt"
    max_depth_path.write_text(f"{depth_range:.6f}\n")

    # Write axes.txt
    axes_path = sprite_dir / "axes.txt"
    axes_path.write_text(f"depth={axis_names[depth_axis]}\n")

    nonzero = np.count_nonzero(depth_u8)
    print(f"    Saved depth.png ({sprite.width}x{sprite.height}, "
          f"{nonzero} pixels with depth, range={depth_range:.4f})")


def find_sprites(base_dir: Path) -> list[Path]:
    """Find all billboard sprites in the new subfolder layout.
    Each sprite lives at <tileset>/<sprite_key>/sprite.png
    """
    sprites = []
    for p in sorted(base_dir.rglob("sprite.png")):
        sprites.append(p)
    return sprites


def main():
    parser = argparse.ArgumentParser(description="Generate shadow meshes from sprites")
    parser.add_argument("paths", nargs="*", help="Specific sprite files to process")
    parser.add_argument("--force", action="store_true", help="Overwrite existing meshes")
    parser.add_argument("--steps", type=int, default=50, help="Shape inference steps (default: 50)")
    parser.add_argument("--device", default="cuda", help="Device (cuda or cpu)")
    parser.add_argument("--depth-only", action="store_true",
                        help="Only extract depth profiles from existing meshes (no AI generation)")
    args = parser.parse_args()

    script_dir = Path(__file__).resolve().parent
    project_root = script_dir.parent.parent
    objects_dir = project_root / "assets" / "objects"

    if args.paths:
        sprites = [Path(p).resolve() for p in args.paths]
    else:
        sprites = find_sprites(objects_dir)

    if not sprites:
        print("No sprites to process.")
        return

    # --depth-only: extract depth profiles from existing GLB meshes
    if args.depth_only:
        to_extract = []
        for sprite in sprites:
            has_mesh = (sprite.parent / "shadow.glb").exists() or (sprite.parent / "mesh.glb").exists()
            depth_exists = (sprite.parent / "depth.png").exists()
            if has_mesh and (not depth_exists or args.force):
                to_extract.append(sprite)

        if not to_extract:
            print(f"All sprites already have depth maps (or no meshes). Use --force to regenerate.")
            return

        print(f"Extracting depth profiles for {len(to_extract)} sprites")
        for i, sprite in enumerate(to_extract):
            print(f"\n[{i+1}/{len(to_extract)}] ", end="")
            try:
                extract_depth_profile(sprite)
            except Exception as e:
                print(f"  ERROR: {e}")
                import traceback
                traceback.print_exc()
        print(f"\nDone. Extracted {len(to_extract)} depth profiles.")
        return

    to_process = []
    for sprite in sprites:
        shadow_path = sprite.parent / "shadow.glb"
        if shadow_path.exists() and not args.force:
            continue
        to_process.append(sprite)

    if not to_process:
        print(f"All {len(sprites)} sprites already have shadow meshes. Use --force to regenerate.")
        return

    print(f"Found {len(to_process)} sprites to process (of {len(sprites)} total)")

    shape_pipeline = load_shape_model(device=args.device)

    for i, sprite in enumerate(to_process):
        print(f"\n[{i+1}/{len(to_process)}] ", end="")
        try:
            if args.force:
                old = sprite.parent / "shadow.glb"
                if old.exists():
                    old.unlink()
            process_sprite(shape_pipeline, sprite, steps=args.steps)
        except Exception as e:
            print(f"  ERROR: {e}")
            import traceback
            traceback.print_exc()
            continue

    print(f"\nDone. Processed {len(to_process)} sprites.")


if __name__ == "__main__":
    main()
