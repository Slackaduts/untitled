#!/usr/bin/env python3
"""Generate depth, normal, and roughness maps from sprites or terrain textures.

Usage:
    python tools/generate_maps.py [--force] [--model small|large] [path...]
    python tools/generate_maps.py --terrain [--force] [--strength N] [path...]

Sprite mode (default):
    Processes sprite.png files using DepthAnything V2 for depth estimation.
    If no paths given, processes all sprite.png in assets/objects/.

Terrain mode (--terrain):
    Processes tileable terrain textures using luminance-based normal generation
    (no AI model needed). Expects folder structure: textures/<name>/diffuse.qoi
    Outputs normal.png and roughness.png alongside diffuse in each folder.
    If no paths given, processes all assets/textures/*/diffuse.* files.

Requires: pillow, numpy, scipy (+ torch, transformers for sprite mode)
NixOS: nix-shell -p python3 python3Packages.torchWithCuda python3Packages.transformers python3Packages.pillow python3Packages.scipy
"""

import argparse
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def load_depth_model(model_size="small"):
    """Load DepthAnything V2 model via HuggingFace transformers."""
    from transformers import pipeline

    model_name = {
        "small": "depth-anything/Depth-Anything-V2-Small-hf",
        "large": "depth-anything/Depth-Anything-V2-Large-hf",
    }.get(model_size, "depth-anything/Depth-Anything-V2-Small-hf")

    import torch
    device = 0 if torch.cuda.is_available() else -1
    device_name = "GPU (CUDA)" if device == 0 else "CPU"
    print(f"Loading model: {model_name} on {device_name}")
    pipe = pipeline("depth-estimation", model=model_name, device=device)
    print(f"Model loaded on {device_name}")
    return pipe


def estimate_depth(pipe, image: Image.Image) -> np.ndarray:
    """Run depth estimation, returns float32 depth map [0, 1]."""
    result = pipe(image)
    depth = np.array(result["depth"], dtype=np.float32)
    # Normalize to [0, 1]
    dmin, dmax = depth.min(), depth.max()
    if dmax - dmin > 1e-6:
        depth = (depth - dmin) / (dmax - dmin)
    else:
        depth = np.zeros_like(depth)
    return depth


def depth_to_roughness(depth: np.ndarray, normal: np.ndarray) -> np.ndarray:
    """Derive a roughness map from depth and normal data.

    Roughness is estimated from local surface variation:
    - High-frequency depth changes → rough (gravel, bark, stone cracks)
    - Smooth depth gradients → smooth (polished surfaces, still water)
    - Normal deviation from flat adds extra roughness (bumpy areas)

    Returns uint8 grayscale image [0=smooth, 255=rough].
    """
    from scipy.ndimage import uniform_filter

    # Local depth variance in a 5×5 window — captures fine surface detail
    depth_mean = uniform_filter(depth, size=5)
    depth_sq_mean = uniform_filter(depth * depth, size=5)
    depth_var = np.maximum(depth_sq_mean - depth_mean * depth_mean, 0.0)

    # Normal deviation from flat (0,0,1) — normals are [0,255] with 128 = zero
    nx = (normal[:, :, 0].astype(np.float32) / 255.0) * 2.0 - 1.0
    ny = (normal[:, :, 1].astype(np.float32) / 255.0) * 2.0 - 1.0
    deviation = np.sqrt(nx * nx + ny * ny)  # 0 = flat, ~1 = steep

    # Combine: depth variance drives base roughness, normal deviation adds detail
    roughness = np.sqrt(depth_var) * 8.0 + deviation * 0.4
    roughness = roughness.clip(0.0, 1.0)

    # Bias toward mid-range — most natural surfaces aren't perfectly smooth
    roughness = 0.3 + roughness * 0.6

    return (roughness * 255).clip(0, 255).astype(np.uint8)


def depth_to_normal(depth: np.ndarray, strength: float = 2.0) -> np.ndarray:
    """Convert depth map to normal map using Sobel-like finite differences.

    Returns uint8 RGB image where:
        R = normal.x mapped from [-1,1] to [0,255]
        G = normal.y mapped from [-1,1] to [0,255]
        B = normal.z mapped from [0,1] to [128,255]
    """
    # Compute gradients
    dy = np.zeros_like(depth)
    dx = np.zeros_like(depth)

    # Central differences (Sobel-like)
    dy[1:-1, :] = (depth[2:, :] - depth[:-2, :]) * strength
    dx[:, 1:-1] = (depth[:, 2:] - depth[:, :-2]) * strength

    # Build normal vectors
    nx = -dx
    ny = -dy
    nz = np.ones_like(depth)

    # Normalize
    length = np.sqrt(nx * nx + ny * ny + nz * nz)
    length = np.maximum(length, 1e-8)
    nx /= length
    ny /= length
    nz /= length

    # Map to [0, 255]
    r = ((nx * 0.5 + 0.5) * 255).clip(0, 255).astype(np.uint8)
    g = ((ny * 0.5 + 0.5) * 255).clip(0, 255).astype(np.uint8)
    b = ((nz * 0.5 + 0.5) * 255).clip(0, 255).astype(np.uint8)

    return np.stack([r, g, b], axis=-1)


def process_sprite(pipe, src_path: Path, force: bool = False, strength: float = 2.0):
    """Process a single sprite: generate depth, normal, and roughness maps."""
    depth_path = src_path.with_name(src_path.stem + "_depth.png")
    normal_path = src_path.with_name(src_path.stem + "_normal.png")
    roughness_path = src_path.with_name(src_path.stem + "_roughness.png")

    if not force and depth_path.exists() and normal_path.exists() and roughness_path.exists():
        print(f"  skip (already exists): {src_path.name}")
        return

    # Load image
    img = Image.open(src_path).convert("RGBA")
    w, h = img.size

    # Upscale 4x with nearest-neighbor for better depth quality on pixel art
    scale = 4
    img_up = img.resize((w * scale, h * scale), Image.NEAREST)

    # Convert to RGB for depth estimation (ignore alpha)
    img_rgb = Image.new("RGB", img_up.size, (128, 128, 128))
    img_rgb.paste(img_up, mask=img_up.split()[3])  # paste using alpha as mask

    # Run depth estimation
    depth = estimate_depth(pipe, img_rgb)

    # Downscale depth back to original size
    depth_small = np.array(
        Image.fromarray((depth * 255).astype(np.uint8)).resize((w, h), Image.BILINEAR)
    ).astype(np.float32) / 255.0

    # Generate normal map from depth
    normal = depth_to_normal(depth_small, strength=strength)

    # Generate roughness map from depth + normals
    roughness = depth_to_roughness(depth_small, normal)

    # Apply alpha mask — transparent pixels get flat normal / default roughness
    alpha = np.array(img)[:, :, 3]
    flat_normal = np.array([128, 128, 255], dtype=np.uint8)
    mask = alpha < 10
    normal[mask] = flat_normal
    roughness[mask] = 128  # neutral roughness for transparent pixels

    # Save depth map (grayscale)
    depth_img = Image.fromarray((depth_small * 255).astype(np.uint8), mode="L")
    depth_img.save(depth_path)

    # Save normal map (RGB)
    normal_img = Image.fromarray(normal, mode="RGB")
    normal_img.save(normal_path)

    # Save roughness map (grayscale)
    roughness_img = Image.fromarray(roughness, mode="L")
    roughness_img.save(roughness_path)

    print(f"  generated: {depth_path.name}, {normal_path.name}, {roughness_path.name}")


# ── Terrain texture mode ────────────────────────────────────────────────────
# Luminance-based normal generation for tileable terrain textures.
# No AI model needed — derives normals from grayscale gradient.

def luminance_to_depth(img: Image.Image) -> np.ndarray:
    """Convert an RGB image to a normalized luminance-based depth map."""
    arr = np.array(img.convert("RGB"), dtype=np.float32) / 255.0
    # Perceptual luminance
    lum = 0.299 * arr[:, :, 0] + 0.587 * arr[:, :, 1] + 0.114 * arr[:, :, 2]
    return lum


def process_terrain(src_path: Path, force: bool = False, strength: float = 1.5):
    """Process a terrain diffuse texture: generate normal and roughness maps.

    Expects src_path to be e.g. assets/textures/grass/diffuse.qoi (or .png).
    Outputs normal.qoi and roughness.qoi in the same directory.
    """
    out_dir = src_path.parent
    normal_path = out_dir / "normal.qoi"
    roughness_path = out_dir / "roughness.qoi"

    if not force and normal_path.exists() and roughness_path.exists():
        # Skip only if normal is larger than placeholder (1x1)
        existing = Image.open(normal_path)
        if existing.size[0] > 1:
            print(f"  skip (already exists): {out_dir.name}/")
            return

    img = Image.open(src_path).convert("RGB")
    w, h = img.size
    print(f"  {w}x{h}")

    # Derive depth from luminance
    depth = luminance_to_depth(img)

    # Generate normal map
    normal = depth_to_normal(depth, strength=strength)

    # Generate roughness map
    roughness = depth_to_roughness(depth, normal)

    # Save as QOI (QOI requires RGB/RGBA — expand grayscale roughness to RGB)
    Image.fromarray(normal, mode="RGB").save(normal_path)
    roughness_rgb = np.stack([roughness, roughness, roughness], axis=-1)
    Image.fromarray(roughness_rgb, mode="RGB").save(roughness_path)
    print(f"  generated: {out_dir.name}/normal.qoi, roughness.qoi")


def main():
    parser = argparse.ArgumentParser(description="Generate depth, normal, and roughness maps")
    parser.add_argument("paths", nargs="*", help="Specific files to process")
    parser.add_argument("--force", action="store_true", help="Regenerate existing maps")
    parser.add_argument("--terrain", action="store_true",
                       help="Terrain mode: luminance-based normals (no AI model)")
    parser.add_argument("--model", default="small", choices=["small", "large"],
                       help="DepthAnything V2 model size (default: small, sprite mode only)")
    parser.add_argument("--strength", type=float, default=None,
                       help="Normal map strength (default: 2.0 sprites, 1.5 terrain)")
    args = parser.parse_args()

    strength = args.strength

    if args.terrain:
        # ── Terrain mode ────────────────────────────────────────────
        if strength is None:
            strength = 1.5

        if args.paths:
            files = [Path(p) for p in args.paths]
        else:
            tex_dir = Path("assets/textures")
            if not tex_dir.exists():
                print(f"Error: {tex_dir} not found. Run from project root.")
                sys.exit(1)
            files = sorted(tex_dir.glob("*/diffuse.*"))

        if not files:
            print("No terrain textures found.")
            return

        print(f"Processing {len(files)} terrain textures...")
        for f in files:
            print(f"Processing: {f.parent.name}/{f.name}")
            try:
                process_terrain(f, force=args.force, strength=strength)
            except Exception as e:
                print(f"  ERROR: {e}")
    else:
        # ── Sprite mode ─────────────────────────────────────────────
        if strength is None:
            strength = 2.0

        if args.paths:
            files = [Path(p) for p in args.paths]
        else:
            objects_dir = Path("assets/objects")
            if not objects_dir.exists():
                print(f"Error: {objects_dir} not found. Run from project root.")
                sys.exit(1)
            files = sorted(objects_dir.rglob("sprite.png"))

        if not files:
            print("No files to process.")
            return

        print(f"Processing {len(files)} files...")
        pipe = load_depth_model(args.model)

        for f in files:
            print(f"Processing: {f.name}")
            try:
                process_sprite(pipe, f, force=args.force, strength=strength)
            except Exception as e:
                print(f"  ERROR: {e}")

    print("Done!")


if __name__ == "__main__":
    main()
