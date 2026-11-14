use crate::error::{IndexError, Result};

pub const FOR_BLOCK_SIZE: usize = 128;

pub fn write_varint(mut val: u32, out: &mut Vec<u8>) {
    while val >= 0x80 {
        out.push((val as u8 & 0x7F) | 0x80);
        val >>= 7;
    }
    out.push(val as u8 & 0x7F);
}

pub fn read_varint(input: &[u8], cursor: &mut usize) -> Result<u32> {
    let mut val: u32 = 0;
    let mut shift = 0;

    while *cursor < input.len() {
        let byte = input[*cursor];
        *cursor += 1;

        if shift >= 35 {
            return Err(IndexError::Compression("varint integer overflow".into()));
        }

        val |= ((byte & 0x7F) as u32) << shift;
        if (byte & 0x80) == 0 {
            return Ok(val);
        }
        shift += 7;
    }

    Err(IndexError::Compression(
        "unexpected end of input while reading varint".into(),
    ))
}

pub fn doc_ids_to_deltas(doc_ids: &[u32]) -> Result<Vec<u32>> {
    if doc_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut deltas = Vec::with_capacity(doc_ids.len());
    deltas.push(doc_ids[0]);

    for window in doc_ids.windows(2) {
        if window[1] <= window[0] {
            return Err(IndexError::Compression(format!(
                "doc_ids must be strictly monotonically increasing: {} followed by {}",
                window[0], window[1]
            )));
        }
        deltas.push(window[1] - window[0] - 1);
    }

    Ok(deltas)
}

pub fn deltas_to_doc_ids(deltas: &[u32]) -> Vec<u32> {
    if deltas.is_empty() {
        return Vec::new();
    }

    let mut doc_ids = Vec::with_capacity(deltas.len());
    let mut current = deltas[0];
    doc_ids.push(current);

    for &delta in &deltas[1..] {
        current = current.checked_add(delta + 1).expect("doc_id overflow");
        doc_ids.push(current);
    }

    doc_ids
}

pub fn encode_for_block(block: &[u32; FOR_BLOCK_SIZE], out: &mut Vec<u8>) {
    let max_val = *block.iter().max().unwrap_or(&0);
    let bits = if max_val == 0 {
        0u8
    } else {
        (32 - max_val.leading_zeros()) as u8
    };

    out.push(bits);

    if bits == 0 {
        return;
    }

    let mut bit_buf: u64 = 0;
    let mut bits_in_buf = 0;

    for &val in block {
        bit_buf |= (val as u64) << bits_in_buf;
        bits_in_buf += bits as usize;

        while bits_in_buf >= 8 {
            out.push(bit_buf as u8);
            bit_buf >>= 8;
            bits_in_buf -= 8;
        }
    }

    debug_assert_eq!(
        bits_in_buf, 0,
        "128 * bits is always a multiple of 8, bits_in_buf must be 0"
    );
}

pub fn decode_for_block(
    input: &[u8],
    cursor: &mut usize,
    out: &mut [u32; FOR_BLOCK_SIZE],
) -> Result<()> {
    if *cursor >= input.len() {
        return Err(IndexError::Compression(
            "unexpected end of input reading FOR header".into(),
        ));
    }

    let bits = input[*cursor];
    *cursor += 1;

    if bits > 32 {
        return Err(IndexError::Compression(format!(
            "invalid FOR bit width: {}",
            bits
        )));
    }

    if bits == 0 {
        out.fill(0);
        return Ok(());
    }

    let bytes_needed = (FOR_BLOCK_SIZE * bits as usize) / 8;
    if *cursor + bytes_needed > input.len() {
        return Err(IndexError::Compression(
            "unexpected end of input reading FOR block payload".into(),
        ));
    }

    let payload = &input[*cursor..*cursor + bytes_needed];
    *cursor += bytes_needed;

    let mask = if bits == 32 {
        0xFFFF_FFFF
    } else {
        (1u64 << bits) - 1
    };
    let mut bit_buf: u64 = 0;
    let mut bits_in_buf = 0;
    let mut byte_idx = 0;

    for item in out.iter_mut() {
        while bits_in_buf < bits as usize {
            bit_buf |= (payload[byte_idx] as u64) << bits_in_buf;
            byte_idx += 1;
            bits_in_buf += 8;
        }

        *item = (bit_buf & mask) as u32;
        bit_buf >>= bits;
        bits_in_buf -= bits as usize;
    }

    Ok(())
}

pub fn encode_postings(doc_ids: &[u32], out: &mut Vec<u8>) -> Result<()> {
    write_varint(doc_ids.len() as u32, out);

    if doc_ids.is_empty() {
        return Ok(());
    }

    let deltas = doc_ids_to_deltas(doc_ids)?;
    let full_blocks = deltas.len() / FOR_BLOCK_SIZE;

    for i in 0..full_blocks {
        let start = i * FOR_BLOCK_SIZE;
        let block: &[u32; FOR_BLOCK_SIZE] = deltas[start..start + FOR_BLOCK_SIZE]
            .try_into()
            .expect("exact slice");
        encode_for_block(block, out);
    }

    let tail_start = full_blocks * FOR_BLOCK_SIZE;
    for &delta in &deltas[tail_start..] {
        write_varint(delta, out);
    }

    Ok(())
}

pub fn decode_postings(input: &[u8], cursor: &mut usize, out: &mut Vec<u32>) -> Result<()> {
    let count = read_varint(input, cursor)? as usize;
    if count == 0 {
        return Ok(());
    }

    let mut deltas = Vec::with_capacity(count);
    let full_blocks = count / FOR_BLOCK_SIZE;
    let mut block_buf = [0u32; FOR_BLOCK_SIZE];

    for _ in 0..full_blocks {
        decode_for_block(input, cursor, &mut block_buf)?;
        deltas.extend_from_slice(&block_buf);
    }

    let tail_count = count % FOR_BLOCK_SIZE;
    for _ in 0..tail_count {
        let delta = read_varint(input, cursor)?;
        deltas.push(delta);
    }

    let doc_ids = deltas_to_doc_ids(&deltas);
    out.extend_from_slice(&doc_ids);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_varint_round_trip() {
        let test_values = [0, 1, 127, 128, 255, 256, 16383, 16384, u32::MAX];
        for &val in &test_values {
            let mut buf = Vec::new();
            write_varint(val, &mut buf);
            let mut cursor = 0;
            let decoded = read_varint(&buf, &mut cursor).expect("read varint");
            assert_eq!(decoded, val);
            assert_eq!(cursor, buf.len());
        }
    }

    #[test]
    fn test_deltas_round_trip() {
        let doc_ids = vec![5, 10, 11, 100, 1000, 100000];
        let deltas = doc_ids_to_deltas(&doc_ids).expect("deltas");
        let round_trip = deltas_to_doc_ids(&deltas);
        assert_eq!(round_trip, doc_ids);
    }

    #[test]
    fn test_for_block_round_trip() {
        let mut block = [0u32; FOR_BLOCK_SIZE];
        for (i, item) in block.iter_mut().enumerate() {
            *item = (i * 7) as u32;
        }

        let mut buf = Vec::new();
        encode_for_block(&block, &mut buf);

        let mut decoded = [0u32; FOR_BLOCK_SIZE];
        let mut cursor = 0;
        decode_for_block(&buf, &mut cursor, &mut decoded).expect("decode block");
        assert_eq!(decoded, block);
        assert_eq!(cursor, buf.len());
    }

    #[test]
    fn test_for_block_zeros() {
        let block = [0u32; FOR_BLOCK_SIZE];
        let mut buf = Vec::new();
        encode_for_block(&block, &mut buf);
        assert_eq!(buf.len(), 1); // Only 1 byte for bit width 0

        let mut decoded = [0u32; FOR_BLOCK_SIZE];
        let mut cursor = 0;
        decode_for_block(&buf, &mut cursor, &mut decoded).expect("decode zeros");
        assert_eq!(decoded, block);
    }

    #[test]
    fn test_postings_round_trip_empty_and_small() {
        let mut buf = Vec::new();
        encode_postings(&[], &mut buf).expect("encode empty");
        let mut cursor = 0;
        let mut out = Vec::new();
        decode_postings(&buf, &mut cursor, &mut out).expect("decode empty");
        assert!(out.is_empty());

        let small = vec![42];
        buf.clear();
        encode_postings(&small, &mut buf).expect("encode small");
        cursor = 0;
        out.clear();
        decode_postings(&buf, &mut cursor, &mut out).expect("decode small");
        assert_eq!(out, small);
    }

    #[test]
    fn test_postings_round_trip_block_boundary() {
        for count in [127, 128, 129, 255, 256, 257, 500] {
            let doc_ids: Vec<u32> = (0..count).map(|x| x * 3 + 10).collect();
            let mut buf = Vec::new();
            encode_postings(&doc_ids, &mut buf).expect("encode");
            let mut cursor = 0;
            let mut out = Vec::new();
            decode_postings(&buf, &mut cursor, &mut out).expect("decode");
            assert_eq!(out, doc_ids);
            assert_eq!(cursor, buf.len());
        }
    }

    proptest! {
        #[test]
        fn test_proptest_varint_round_trip(val in any::<u32>()) {
            let mut buf = Vec::new();
            write_varint(val, &mut buf);
            let mut cursor = 0;
            let decoded = read_varint(&buf, &mut cursor).unwrap();
            prop_assert_eq!(decoded, val);
            prop_assert_eq!(cursor, buf.len());
        }

        #[test]
        fn test_proptest_postings_round_trip(
            mut doc_ids in prop::collection::vec(1u32..50_000, 0..1000)
        ) {
            doc_ids.sort_unstable();
            doc_ids.dedup();

            let mut buf = Vec::new();
            encode_postings(&doc_ids, &mut buf).unwrap();

            let mut cursor = 0;
            let mut decoded = Vec::new();
            decode_postings(&buf, &mut cursor, &mut decoded).unwrap();

            prop_assert_eq!(decoded, doc_ids);
            prop_assert_eq!(cursor, buf.len());
        }
    }
}
