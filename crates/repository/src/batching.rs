/// Splits `ids` into chunks of at most `batch_size`, matching the role of
/// Go's `batchSize`-based loops in `product_repository.go` (avoiding MySQL's
/// 65535 bound-placeholder limit) and `import_service.go` (SKU lookup batching).
pub fn chunk_ids(ids: &[u32], batch_size: usize) -> Vec<&[u32]> {
    if batch_size == 0 {
        return if ids.is_empty() { Vec::new() } else { vec![ids] };
    }
    ids.chunks(batch_size).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_chunks() {
        assert_eq!(chunk_ids(&[], 10), Vec::<&[u32]>::new());
    }

    #[test]
    fn splits_evenly_divisible_input() {
        let ids: Vec<u32> = (1..=10).collect();
        let chunks = chunk_ids(&ids, 5);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], &ids[0..5]);
        assert_eq!(chunks[1], &ids[5..10]);
    }

    #[test]
    fn splits_with_remainder_in_last_chunk() {
        let ids: Vec<u32> = (1..=7).collect();
        let chunks = chunk_ids(&ids, 3);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[2].len(), 1);
    }

    #[test]
    fn batch_size_larger_than_input_yields_one_chunk() {
        let ids: Vec<u32> = (1..=3).collect();
        let chunks = chunk_ids(&ids, 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], &ids[..]);
    }

    #[test]
    fn zero_batch_size_falls_back_to_single_chunk() {
        let ids: Vec<u32> = (1..=3).collect();
        let chunks = chunk_ids(&ids, 0);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], &ids[..]);
    }

    #[test]
    fn zero_batch_size_with_empty_input_yields_no_chunks() {
        assert_eq!(chunk_ids(&[], 0), Vec::<&[u32]>::new());
    }
}
