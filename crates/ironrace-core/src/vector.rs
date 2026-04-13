use hnsw_rs::prelude::*;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Default shard size for HNSW index. Shards use sequential insert to
/// guarantee fully connected graphs. 5K maintains ≥97% recall at all
/// tested scales (1K-100K). Larger shards reduce merge overhead but may
/// lower recall. Configurable via `VectorIndex::build_with_shard_size()`.
pub const DEFAULT_SHARD_SIZE: usize = 5000;

/// Internal storage: single graph for small datasets, sharded for large.
enum IndexInner {
    Single {
        hnsw: Hnsw<'static, f32, DistCosine>,
    },
    Sharded {
        shards: Vec<Hnsw<'static, f32, DistCosine>>,
        shard_offsets: Vec<usize>,
        shard_sizes: Vec<usize>,
    },
}

// SAFETY: Hnsw from hnsw_rs uses internal Arc<RwLock<...>> for graph layers.
// IndexInner only holds Vec<Hnsw> and primitive fields.
// Verified against hnsw_rs =0.3.4 source — no thread-local state or raw pointers
// that would violate Send or Sync. Re-verify on hnsw_rs version upgrades.
// hnsw_rs is pinned to =0.3.4 in Cargo.toml to prevent silent breakage.
unsafe impl Send for IndexInner {}
unsafe impl Sync for IndexInner {}

/// HNSW approximate nearest neighbor index.
///
/// Build once from a collection of embedding vectors, then search many times.
/// For datasets larger than `shard_size` vectors, the index is automatically
/// sharded into smaller HNSW graphs that are built and searched in parallel.
pub struct VectorIndex {
    inner: IndexInner,
    count: usize,
}

/// Compute ef_search for a given top_k and index size.
pub fn compute_ef_search(top_k: usize, count: usize) -> usize {
    if top_k >= count {
        count * 2
    } else if top_k >= count / 2 {
        count
    } else {
        top_k.max(100)
    }
}

/// A scored result for min-heap merging.
struct ScoredId {
    id: usize,
    score: f32,
}

impl PartialEq for ScoredId {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score && self.id == other.id
    }
}

impl Eq for ScoredId {}

impl PartialOrd for ScoredId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredId {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse: smallest score is "greatest" so it gets popped first from the min-heap
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

/// Merge top-k results from multiple shards using a bounded min-heap.
pub fn merge_top_k(shard_results: Vec<Vec<(usize, f32)>>, top_k: usize) -> Vec<(usize, f32)> {
    let mut heap: BinaryHeap<ScoredId> = BinaryHeap::with_capacity(top_k + 1);

    for results in shard_results {
        for (id, score) in results {
            if heap.len() < top_k {
                heap.push(ScoredId { id, score });
            } else if let Some(min) = heap.peek() {
                if score > min.score {
                    heap.pop();
                    heap.push(ScoredId { id, score });
                }
            }
        }
    }

    let mut result: Vec<(usize, f32)> = heap.into_iter().map(|s| (s.id, s.score)).collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    result
}

impl VectorIndex {
    /// Build index from a slice of embedding vectors using the default shard size.
    pub fn build(vectors: &[Vec<f32>], ef_construction: usize) -> Self {
        Self::build_with_shard_size(vectors, ef_construction, DEFAULT_SHARD_SIZE)
    }

    /// Build index with a custom shard size.
    ///
    /// Smaller shards improve recall but increase search latency (more shards
    /// to search + merge). Larger shards are faster but may lower recall if
    /// the graph becomes poorly connected.
    ///
    /// Recommended values: 2500-10000. Default: 5000.
    pub fn build_with_shard_size(
        vectors: &[Vec<f32>],
        ef_construction: usize,
        shard_size: usize,
    ) -> Self {
        let n = vectors.len();
        let max_nb_connection = 16;
        let max_layer = 16;
        let shard_size = if shard_size == 0 {
            DEFAULT_SHARD_SIZE
        } else {
            shard_size
        };

        if n <= shard_size {
            // Small dataset: single index, sequential insert
            let hnsw = Hnsw::new(max_nb_connection, n, max_layer, ef_construction, DistCosine);
            for (i, v) in vectors.iter().enumerate() {
                hnsw.insert((v.as_slice(), i));
            }
            VectorIndex {
                inner: IndexInner::Single { hnsw },
                count: n,
            }
        } else {
            // Large dataset: shard and build each in parallel with sequential insert
            let chunks: Vec<&[Vec<f32>]> = vectors.chunks(shard_size).collect();
            let shard_sizes: Vec<usize> = chunks.iter().map(|c| c.len()).collect();
            let shard_offsets: Vec<usize> = shard_sizes
                .iter()
                .scan(0usize, |acc, &size| {
                    let offset = *acc;
                    *acc += size;
                    Some(offset)
                })
                .collect();

            let shards: Vec<Hnsw<'static, f32, DistCosine>> = chunks
                .par_iter()
                .map(|chunk| {
                    let shard_n = chunk.len();
                    let hnsw = Hnsw::new(
                        max_nb_connection,
                        shard_n,
                        max_layer,
                        ef_construction,
                        DistCosine,
                    );
                    for (local_id, v) in chunk.iter().enumerate() {
                        hnsw.insert((v.as_slice(), local_id));
                    }
                    hnsw
                })
                .collect();

            VectorIndex {
                inner: IndexInner::Sharded {
                    shards,
                    shard_offsets,
                    shard_sizes,
                },
                count: n,
            }
        }
    }

    /// Search for the top_k nearest neighbors to the query vector.
    ///
    /// Returns a list of (original_index, similarity_score) tuples,
    /// sorted by similarity (highest first).
    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(usize, f32)> {
        if self.count == 0 {
            return vec![];
        }

        match &self.inner {
            IndexInner::Single { hnsw } => {
                let ef_search = compute_ef_search(top_k, self.count);
                let neighbours = hnsw.search(query, top_k, ef_search);
                neighbours
                    .iter()
                    .map(|n| {
                        let similarity = 1.0 - n.distance;
                        (n.d_id, similarity)
                    })
                    .collect()
            }
            IndexInner::Sharded {
                shards,
                shard_offsets,
                shard_sizes,
            } => {
                let per_shard_results: Vec<Vec<(usize, f32)>> = shards
                    .par_iter()
                    .enumerate()
                    .map(|(s, hnsw)| {
                        let ef_search = compute_ef_search(top_k, shard_sizes[s]);
                        let neighbours = hnsw.search(query, top_k, ef_search);
                        neighbours
                            .iter()
                            .map(|n| {
                                let global_id = shard_offsets[s] + n.d_id;
                                let similarity = 1.0 - n.distance;
                                (global_id, similarity)
                            })
                            .collect()
                    })
                    .collect();

                merge_top_k(per_shard_results, top_k)
            }
        }
    }

    /// Returns the number of vectors in the index.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns true if the index contains no vectors.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}
