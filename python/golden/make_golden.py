#!/usr/bin/env python3
import json
import os
import numpy as np
import scipy.fftpack
import scipy.ndimage
import pywt
import imagehash
from PIL import Image, ImageDraw

SEED = 1337
NUM_IMAGES = 200

def hash_to_u64(bool_matrix):
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
    diff = matrix_9x8[:, 1:] > matrix_9x8[:, :-1]
    return hash_to_u64(diff)

def compute_layer_a_phash(matrix_32x32):
    dct = scipy.fftpack.dct(scipy.fftpack.dct(matrix_32x32.astype(float), axis=0), axis=1)
    dct_8x8 = dct[:8, :8]
    med = float(np.median(dct_8x8))
    diff = dct_8x8 > med
    return hash_to_u64(diff)

def compute_layer_a_whash(matrix_64x64):
    pixels = matrix_64x64.astype(float)
    ll_max_level = int(np.log2(64))
    level = int(np.log2(8))
    dwt_level = ll_max_level - level

    coeffs = pywt.wavedec2(pixels, 'haar', level=ll_max_level)
    coeffs[0] = np.zeros_like(coeffs[0])
    reconstructed = pywt.waverec2(coeffs, 'haar')

    coeffs_3 = pywt.wavedec2(reconstructed, 'haar', level=dwt_level)
    ll3 = coeffs_3[0]
    med = float(np.median(ll3))
    diff = ll3 > med
    return hash_to_u64(diff)

def compute_layer_a_pdq(matrix_64x64):
    dct = scipy.fftpack.dct(scipy.fftpack.dct(matrix_64x64.astype(float), axis=0, norm=None), axis=1, norm=None)
    block16 = dct[:16, :16]
    med = float(np.median(block16))
    diff = block16 > med
    flat = diff.flatten()
    bytes_out = bytearray(32)
    for i, bit in enumerate(flat):
        if bit:
            bytes_out[i // 8] |= (1 << (7 - (i % 8)))

    grad_sum = 0
    for y in range(64):
        for x in range(63):
            u = float(matrix_64x64[y, x])
            v = float(matrix_64x64[y, x + 1])
            d = int(np.trunc((u - v) * 100.0 / 255.0))
            grad_sum += abs(d)
    for y in range(63):
        for x in range(64):
            u = float(matrix_64x64[y, x])
            v = float(matrix_64x64[y + 1, x])
            d = int(np.trunc((u - v) * 100.0 / 255.0))
            grad_sum += abs(d)
    quality = min(100, max(0, grad_sum // 90))
    return bytes_out.hex(), quality

def generate_synthetic_image(idx, rng, size=(128, 128)):
    w, h = size
    base = rng.uniform(40, 215, (h, w, 3))
    base = scipy.ndimage.gaussian_filter(base, sigma=(rng.uniform(1.5, 3.5), rng.uniform(1.5, 3.5), 0))

    x = np.linspace(-2, 2, w)
    y = np.linspace(-2, 2, h)
    xx, yy = np.meshgrid(x, y)
    freq1, freq2 = rng.uniform(1, 4), rng.uniform(1, 4)
    pattern = np.sin(xx * freq1 + yy * freq2) * 35.0 + np.cos(xx * freq2 - yy * freq1) * 25.0
    base = np.clip(base + pattern[..., None], 0, 255).astype(np.uint8)

    im = Image.fromarray(base, 'RGB')
    draw = ImageDraw.Draw(im)
    for _ in range(rng.integers(2, 5)):
        x0, y0 = rng.integers(10, w - 40), rng.integers(10, h - 40)
        x1, y1 = x0 + rng.integers(15, 35), y0 + rng.integers(15, 35)
        col = tuple(rng.integers(20, 230, size=3).tolist())
        if rng.random() > 0.5:
            draw.rectangle([x0, y0, x1, y1], outline=col, width=2)
        else:
            draw.ellipse([x0, y0, x1, y1], outline=col, width=2)

    arr = scipy.ndimage.gaussian_filter(np.array(im, dtype=float), sigma=(0.8, 0.8, 0))
    return Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8), 'RGB')

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

        b_ahash = int(str(imagehash.average_hash(img, 8)), 16)
        b_dhash = int(str(imagehash.dhash(img, 8)), 16)
        b_phash = int(str(imagehash.phash(img, 8, 4)), 16)
        b_whash = int(str(imagehash.whash(img, hash_size=8, image_scale=64, mode='haar', remove_max_haar_ll=True)), 16)

        luma_img = img.convert('L')
        mat_8x8 = np.asarray(luma_img.resize((8, 8), Image.Resampling.LANCZOS), dtype=np.uint8)
        mat_9x8 = np.asarray(luma_img.resize((9, 8), Image.Resampling.LANCZOS), dtype=np.uint8)
        mat_32x32 = np.asarray(luma_img.resize((32, 32), Image.Resampling.LANCZOS), dtype=np.uint8)
        mat_64x64 = np.asarray(luma_img.resize((64, 64), Image.Resampling.LANCZOS), dtype=np.uint8)

        a_ahash = compute_layer_a_ahash(mat_8x8)
        a_dhash = compute_layer_a_dhash(mat_9x8)
        a_phash = compute_layer_a_phash(mat_32x32)
        a_whash = compute_layer_a_whash(mat_64x64)
        a_pdq, a_quality = compute_layer_a_pdq(mat_64x64)

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
                "pdq": a_pdq,
                "pdq_quality": a_quality,
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
