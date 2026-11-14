# Engineering Decisions and Records

## 2026-11-14: Layer B Full-Pipeline Hash Comparison vs Python imagehash

### Context
Step 0.17 requires validating the full Rust pipeline (`decode` -> `orient` -> `composite` -> `luma` -> `hash`) against the Python reference implementation (`Pillow` + `imagehash` + `scipy` + `pywt`) across >=200 synthetic images.

### Tolerance Criteria (§7.1)
- Mean Hamming distance across 200 images MUST be <= 2.0 bits.
- Maximum Hamming distance on any single image MUST be <= 6 bits.

### Measured Results (200 Synthetic Images)
- **aHash**: mean = 0.0600 bits, max = 3 bits
- **dHash**: mean = 0.0900 bits, max = 1 bit
- **pHash**: mean = 0.3400 bits, max = 2 bits
- **wHash**: mean = 0.0400 bits, max = 2 bits

All four hash algorithms pass the tolerance thresholds comfortably:
- All mean distances are <= 0.34 bits (well below the 2.0 bit threshold).
- All maximum distances are <= 3 bits (well below the 6 bit threshold).
