#!/usr/bin/env python3
"""Generate depth and normal maps from tileset sprites using DepthAnything V2.

Usage:
    python tools/generate_maps.py [--force] [--model small|large] [path...]

If no paths given, processes all .png files in assets/tilesets/.
Skips files that already have _depth/_normal variants unless --force.

Requires: torch, transformers, pillow, numpy, scipy
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
    """Process a single sprite: generate depth and normal maps."""
    depth_path = src_path.with_name(src_path.stem + "_depth.png")
    normal_path = src_path.with_name(src_path.stem + "_normal.png")

    if not force and depth_path.exists() and normal_path.exists():
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

    # Apply alpha mask — transparent pixels get flat normal (128, 128, 255)
    alpha = np.array(img)[:, :, 3]
    flat_normal = np.array([128, 128, 255], dtype=np.uint8)
    mask = alpha < 10
    normal[mask] = flat_normal

    # Save depth map (grayscale)
    depth_img = Image.fromarray((depth_small * 255).astype(np.uint8), mode="L")
    depth_img.save(depth_path)

    # Save normal map (RGB)
    normal_img = Image.fromarray(normal, mode="RGB")
    normal_img.save(normal_path)

    print(f"  generated: {depth_path.name}, {normal_path.name}")


def main():
    parser = argparse.ArgumentParser(description="Generate depth and normal maps from sprites")
    parser.add_argument("paths", nargs="*", help="Specific files to process")
    parser.add_argument("--force", action="store_true", help="Regenerate existing maps")
    parser.add_argument("--model", default="small", choices=["small", "large"],
                       help="DepthAnything V2 model size (default: small)")
    parser.add_argument("--strength", type=float, default=2.0,
                       help="Normal map strength (default: 2.0)")
    args = parser.parse_args()

    # Determine files to process
    if args.paths:
        files = [Path(p) for p in args.paths]
    else:
        tileset_dir = Path("assets/tilesets")
        if not tileset_dir.exists():
            print(f"Error: {tileset_dir} not found. Run from project root.")
            sys.exit(1)
        # Process all PNG files that aren't already depth/normal maps
        files = sorted(
            p for p in tileset_dir.glob("*.png")
            if not p.stem.endswith("_depth") and not p.stem.endswith("_normal")
        )

    if not files:
        print("No files to process.")
        return

    print(f"Processing {len(files)} files...")

    # Load model
    pipe = load_depth_model(args.model)

    for f in files:
        print(f"Processing: {f.name}")
        try:
            process_sprite(pipe, f, force=args.force, strength=args.strength)
        except Exception as e:
            print(f"  ERROR: {e}")

    print("Done!")


if __name__ == "__main__":
    main()
