#!/usr/bin/env python3
"""
Generates deterministic synthetic test fixtures and golden hash vectors for Layer A and Layer B tests.
Enforces 100% determinism with a fixed PRNG seed.
"""

import json
import os
import numpy as np
import scipy.fftpack
import pywt
import imagehash
from PIL import Image, ImageDraw

SEED = 1337
NUM_IMAGES = 200

def hash_to_u64(bool_matrix):
    """Converts an 8x8 boolean matrix to a u64 integer where (0,0) is MSB."""
    flat = bool_matrix.flatten()
    val = 0
    for bit in flat:
        val = (val << 1) | (1 if bit else 0)
    return val

def compute_layer_a_ahash(matrix_8x8):
    mean_val = float(np.mean(matrix_8x8))
    diff = matrix_8x8 > mean_val
    return hash_to_u64(diff)

def compute_layer_a_dhash(matrix_9x8):
    # matrix is 8 rows, 9 cols
    diff = matrix_9x8[:, 1:] > matrix_9x8[:, :-1]
    return hash_to_u64(diff)

def compute_layer_a_phash(matrix_32x32):
    # 2D DCT-II unnormalized (scipy default matches imagehash)
    dct = scipy.fftpack.dct(scipy.fftpack.dct(matrix_32x32.astype(float), axis=0), axis=1)
    dct_8x8 = dct[:8, :8]
    med = float(np.median(dct_8x8))
    diff = dct_8x8 > med
    return hash_to_u64(diff)

def compute_layer_a_whash(matrix_64x64):
    pixels = matrix_64x64.astype(float)
    ll_max_level = int(np.log2(64))  # 6
    level = int(np.log2(8))          # 3
    dwt_level = ll_max_level - level  # 3

    # Remove global LL (level 6)
    coeffs = pywt.wavedec2(pixels, 'haar', level=ll_max_level)
    coeffs[0] = np.zeros_like(coeffs[0])
    reconstructed = pywt.waverec2(coeffs, 'haar')

    # Decompose to 3 levels -> LL3 is 8x8
    coeffs_3 = pywt.wavedec2(reconstructed, 'haar', level=dwt_level)
    ll3 = coeffs_3[0]
    med = float(np.median(ll3))
    diff = ll3 > med
    return hash_to_u64(diff)

def generate_synthetic_image(idx, rng, size=(128, 128)):
    w, h = size
    mode_type = idx % 8

    if mode_type == 0:
        # Linear gradient
        angle = rng.uniform(0, 2 * np.pi)
        x = np.linspace(-1, 1, w)
        y = np.linspace(-1, 1, h)
        xx, yy = np.meshgrid(x, y)
        grad = xx * np.cos(angle) + yy * np.sin(angle)
        grad = ((grad - grad.min()) / (grad.max() - grad.min() + 1e-6) * 255).astype(np.uint8)
        arr = np.stack([grad, np.roll(grad, 20, axis=0), np.roll(grad, 40, axis=1)], axis=-1)
        return Image.fromarray(arr, 'RGB')

    elif mode_type == 1:
        # Radial gradient / concentric rings
        x = np.linspace(-2, 2, w)
        y = np.linspace(-2, 2, h)
        xx, yy = np.meshgrid(x, y)
        r = np.sqrt(xx**2 + yy**2)
        rings = np.sin(r * rng.uniform(5, 20))
        rings = ((rings + 1) * 127.5).astype(np.uint8)
        arr = np.stack([rings, 255 - rings, np.roll(rings, 30, axis=0)], axis=-1)
        return Image.fromarray(arr, 'RGB')

    elif mode_type == 2:
        # Geometric shapes (rectangles, ellipses)
        img = Image.new('RGB', (w, h), color=(int(rng.integers(0, 255)), int(rng.integers(0, 255)), int(rng.integers(0, 255))))
        draw = ImageDraw.Draw(img)
        for _ in range(5):
            x0, y0 = rng.integers(0, w - 20), rng.integers(0, h - 20)
            x1, y1 = x0 + rng.integers(10, 50), y0 + rng.integers(10, 50)
            col = (int(rng.integers(0, 255)), int(rng.integers(0, 255)), int(rng.integers(0, 255)))
            if rng.random() > 0.5:
                draw.rectangle([x0, y0, x1, y1], fill=col)
            else:
                draw.ellipse([x0, y0, x1, y1], fill=col)
        return img

    elif mode_type == 3:
        # Checkerboard / grid
        tile_size = int(rng.choice([4, 8, 16, 32]))
        grid = np.indices((h, w)).sum(axis=0) // tile_size % 2
        c1 = rng.integers(0, 256, size=3)
        c2 = rng.integers(0, 256, size=3)
        arr = np.where(grid[..., None] == 1, c1, c2).astype(np.uint8)
        return Image.fromarray(arr, 'RGB')

    elif mode_type == 4:
        # Gaussian smoothed noise
        noise = rng.normal(128, 50, (h, w, 3)).clip(0, 255).astype(np.uint8)
        return Image.fromarray(noise, 'RGB')

    elif mode_type == 5:
        # Stripes / bars
        freq = rng.uniform(2, 10)
        axis = int(rng.choice([0, 1]))
        t = np.linspace(0, freq * 2 * np.pi, w if axis == 1 else h)
        bar = ((np.sin(t) + 1) * 127.5).astype(np.uint8)
        if axis == 1:
            arr = np.tile(bar, (h, 1))
        else:
            arr = np.tile(bar[:, None], (1, w))
        c = rng.integers(50, 200, size=3)
        rgb = np.stack([arr * c[0] // 255, arr * c[1] // 255, arr * c[2] // 255], axis=-1).astype(np.uint8)
        return Image.fromarray(rgb, 'RGB')

    elif mode_type == 6:
        # High contrast synthetic scene with borders
        img = Image.new('RGB', (w, h), color=(255, 255, 255))
        draw = ImageDraw.Draw(img)
        draw.line([0, 0, w, h], fill=(0, 0, 0), width=3)
        draw.line([0, h, w, 0], fill=(255, 0, 0), width=3)
        draw.rectangle([w // 4, h // 4, 3 * w // 4, 3 * h // 4], outline=(0, 128, 0), width=4, fill=(200, 200, 200))
        return img

    else:
        # Multi-frequency sinusoid texture
        x = np.linspace(0, 10, w)
        y = np.linspace(0, 10, h)
        xx, yy = np.meshgrid(x, y)
        z = np.sin(xx * 2) + np.cos(yy * 3) + np.sin(xx + yy)
        z = ((z - z.min()) / (z.max() - z.min()) * 255).astype(np.uint8)
        arr = np.stack([z, np.rot90(z), 255 - z], axis=-1)
        return Image.fromarray(arr, 'RGB')

def main():
    rng = np.random.default_rng(SEED)
    fixtures_dir = os.path.join("tests", "fixtures")
    synthetic_img_dir = os.path.join(fixtures_dir, "synthetic")
    os.makedirs(synthetic_img_dir, exist_ok=True)

    records = []

    print(f"Generating {NUM_IMAGES} deterministic synthetic fixtures and golden hashes...")
    for idx in range(NUM_IMAGES):
        img = generate_synthetic_image(idx, rng)
        img_filename = f"img_{idx:03d}.png"
        img_path = os.path.join(synthetic_img_dir, img_filename)
        img.save(img_path)

        # Full-image PIL hashes (Layer B oracles)
        b_ahash = int(str(imagehash.average_hash(img, 8)), 16)
        b_dhash = int(str(imagehash.dhash(img, 8)), 16)
        b_phash = int(str(imagehash.phash(img, 8, 4)), 16)
        b_whash = int(str(imagehash.whash(img, hash_size=8, image_scale=64, mode='haar', remove_max_haar_ll=True)), 16)

        # Extract resized luma matrices using PIL Lanczos (same as imagehash internally)
        luma_img = img.convert('L')
        mat_8x8 = np.asarray(luma_img.resize((8, 8), Image.Resampling.LANCZOS), dtype=np.uint8)
        mat_9x8 = np.asarray(luma_img.resize((9, 8), Image.Resampling.LANCZOS), dtype=np.uint8)
        mat_32x32 = np.asarray(luma_img.resize((32, 32), Image.Resampling.LANCZOS), dtype=np.uint8)
        mat_64x64 = np.asarray(luma_img.resize((64, 64), Image.Resampling.LANCZOS), dtype=np.uint8)

        # Layer A hashes computed directly on the matrices
        a_ahash = compute_layer_a_ahash(mat_8x8)
        a_dhash = compute_layer_a_dhash(mat_9x8)
        a_phash = compute_layer_a_phash(mat_32x32)
        a_whash = compute_layer_a_whash(mat_64x64)

        record = {
            "id": idx,
            "filename": img_filename,
            "layer_a": {
                "mat_8x8": mat_8x8.tolist(),
                "mat_9x8": mat_9x8.tolist(),
                "mat_32x32": mat_32x32.tolist(),
                "mat_64x64": mat_64x64.tolist(),
                "ahash": a_ahash,
                "dhash": a_dhash,
                "phash": a_phash,
                "whash": a_whash,
            },
            "layer_b": {
                "ahash": b_ahash,
                "dhash": b_dhash,
                "phash": b_phash,
                "whash": b_whash,
            }
        }
        records.append(record)

    golden_json_path = os.path.join(fixtures_dir, "golden_hashes.json")
    with open(golden_json_path, "w", encoding="utf-8") as f:
        json.dump(records, f, indent=2)

    print(f"Wrote {len(records)} golden records to {golden_json_path}")

if __name__ == "__main__":
    main()

