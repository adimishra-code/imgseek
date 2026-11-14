use imgseek_index::DocumentRecord;

pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0xdead_beef_cafe_babe
            } else {
                seed
            },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn next_bytes(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let val = self.next_u64().to_le_bytes();
            let len = chunk.len();
            chunk.copy_from_slice(&val[..len]);
        }
    }
}

pub fn generate_synthetic_dataset(
    count: usize,
    cluster_ratio: f64,
    seed: u64,
) -> Vec<DocumentRecord> {
    let mut rng = SimpleRng::new(seed);
    let mut records = Vec::with_capacity(count);

    let num_centroids = ((count as f64) * 0.05).max(1.0) as usize;
    let mut centroids = Vec::with_capacity(num_centroids);

    for _ in 0..num_centroids {
        let mut pdq = [0u8; 32];
        rng.next_bytes(&mut pdq);
        centroids.push((
            rng.next_u64(),
            rng.next_u64(),
            rng.next_u64(),
            rng.next_u64(),
            pdq,
        ));
    }

    for i in 0..count {
        let doc_id = (i + 1) as u32;
        let is_clustered = (rng.next_u32() as f64 / u32::MAX as f64) < cluster_ratio;

        let (phash, whash, dhash, ahash, pdq) = if is_clustered && !centroids.is_empty() {
            let centroid_idx = (rng.next_u32() as usize) % centroids.len();
            let c = &centroids[centroid_idx];

            // Perturb 1-3 bits
            let flip_bit_64 = |mut val: u64, rng: &mut SimpleRng| -> u64 {
                let bit = (rng.next_u32() % 64) as usize;
                val ^= 1u64 << bit;
                val
            };

            let p = flip_bit_64(c.0, &mut rng);
            let w = flip_bit_64(c.1, &mut rng);
            let d = flip_bit_64(c.2, &mut rng);
            let a = flip_bit_64(c.3, &mut rng);

            let mut q = c.4;
            let byte_idx = (rng.next_u32() % 32) as usize;
            let bit_idx = (rng.next_u32() % 8) as usize;
            q[byte_idx] ^= 1u8 << bit_idx;

            (p, w, d, a, q)
        } else {
            let mut pdq = [0u8; 32];
            rng.next_bytes(&mut pdq);
            (
                rng.next_u64(),
                rng.next_u64(),
                rng.next_u64(),
                rng.next_u64(),
                pdq,
            )
        };

        records.push(DocumentRecord {
            doc_id,
            phash,
            whash,
            dhash,
            ahash,
            pdq,
            pdq_quality: 90,
            width: 800,
            height: 600,
            chroma_ref: [0x55; 32],
        });
    }

    records
}
