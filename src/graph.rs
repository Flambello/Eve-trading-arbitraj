use crate::sde::{SdeLoader, SystemNode};
use anyhow::Result;
use lru::LruCache;
use petgraph::graph::{NodeIndex, UnGraph};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::sync::Mutex;

#[derive(Clone)]
struct FastBitSet {
    words: Vec<u64>,
}

impl FastBitSet {
    #[inline]
    fn new(size: usize) -> Self {
        Self {
            words: vec![0u64; size.div_ceil(64)],
        }
    }

    #[inline]
    fn get(&self, idx: usize) -> bool {
        (self.words[idx / 64] & (1u64 << (idx % 64))) != 0
    }

    #[inline]
    fn set(&mut self, idx: usize) {
        self.words[idx / 64] |= 1u64 << (idx % 64);
    }
}

/// Stargate graph representation of the EVE Online universe with memoized jump distances.
pub struct SystemGraph {
    graph: UnGraph<SystemNode, f64>,
    id_to_idx: HashMap<u32, NodeIndex>,
    jump_cache: Mutex<LruCache<(u32, u32, bool), u32>>,
}

impl SystemGraph {
    /// Constructs a graph directly from a vector of system nodes and undirected stargate jump pairs.
    pub fn from_nodes_and_jumps(systems: Vec<SystemNode>, jumps: Vec<(u32, u32)>) -> Self {
        let mut graph = UnGraph::new_undirected();
        let mut id_to_idx = HashMap::with_capacity(systems.len());

        for sys in systems {
            let sys_id = sys.system_id;
            let idx = graph.add_node(sys);
            id_to_idx.insert(sys_id, idx);
        }

        for (from, to) in jumps {
            if let (Some(&from_idx), Some(&to_idx)) = (id_to_idx.get(&from), id_to_idx.get(&to)) {
                // Undirected graph adds edge between both
                if !graph.contains_edge(from_idx, to_idx) {
                    graph.add_edge(from_idx, to_idx, 1.0);
                }
            }
        }

        let cache = LruCache::new(NonZeroUsize::new(20_000).unwrap_or(NonZeroUsize::MIN));

        Self {
            graph,
            id_to_idx,
            jump_cache: Mutex::new(cache),
        }
    }

    /// Loads all solar systems and stargate jump connections from the SQLite SDE database.
    pub fn from_sde(sde: &SdeLoader) -> Result<Self> {
        let systems = sde.all_systems().map_err(|e| {
            tracing::error!(
                error_code = %crate::error::ErrorCode::ErrGraphBuildFailed,
                error = %e,
                "Failed to load solar systems from SDE for graph construction"
            );
            e
        })?;
        let jumps = sde.all_jumps().map_err(|e| {
            tracing::error!(
                error_code = %crate::error::ErrorCode::ErrGraphBuildFailed,
                error = %e,
                "Failed to load stargate jumps from SDE for graph construction"
            );
            e
        })?;
        let graph = Self::from_nodes_and_jumps(systems, jumps);
        tracing::info!(
            node_count = graph.graph.node_count(),
            "Compiled New Eden stargate topology network"
        );
        Ok(graph)
    }

    /// Returns the true security status (-1.0 to 1.0) of a solar system if present in the graph.
    pub fn security(&self, system_id: u32) -> Option<f64> {
        let idx = self.id_to_idx.get(&system_id)?;
        Some(self.graph[*idx].security_status)
    }

    /// Returns the solar system name for a given system ID if present in the graph.
    pub fn system_name(&self, system_id: u32) -> Option<&str> {
        let idx = self.id_to_idx.get(&system_id)?;
        Some(self.graph[*idx].name.as_str())
    }

    /// Discovers all region IDs connected via stargates to solar systems in `region_id`.
    ///
    /// If `safe_only` is true, only highsec ($\ge 0.45$) stargate connections are traversed.
    pub fn adjacent_regions(&self, region_id: u32, safe_only: bool) -> Vec<u32> {
        let mut adj = std::collections::HashSet::new();

        for node_idx in self.graph.node_indices() {
            let node = &self.graph[node_idx];
            if node.region_id == region_id {
                if safe_only && node.security_status < 0.45 {
                    continue;
                }

                for neighbor_idx in self.graph.neighbors(node_idx) {
                    let neighbor = &self.graph[neighbor_idx];
                    if neighbor.region_id != region_id
                        && (!safe_only || neighbor.security_status >= 0.45)
                    {
                        adj.insert(neighbor.region_id);
                    }
                }
            }
        }

        let mut res: Vec<u32> = adj.into_iter().collect();
        res.sort_unstable();
        res
    }

    /// Returns true if the system's security status is $\ge 0.45$ (highsec).
    pub fn is_highsec(&self, system_id: u32) -> bool {
        self.security(system_id).map(|s| s >= 0.45).unwrap_or(false)
    }

    /// Calculates the number of stargate jumps between `origin` and `dest`.
    ///
    /// If `safe_only` is true:
    /// - Rejects origin or destination if in lowsec/nullsec (returns 9999).
    /// - Uses Dijkstra with edge weight 1000.0 on systems with security $< 0.45$.
    /// - Returns the sentinel `9999` if no path exists or if reaching `dest` requires traversing lowsec.
    ///
    /// Results are cached in an LRU cache with capacity 20,000.
    pub fn jump_count(&self, origin: u32, dest: u32, safe_only: bool) -> u32 {
        if origin == dest {
            return 0;
        }

        let cache_key = (origin.min(dest), origin.max(dest), safe_only);

        // Check LRU cache (with poisoning recovery)
        {
            let mut cache = self.jump_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(&jumps) = cache.get(&cache_key) {
                return jumps;
            }
        }

        let origin_idx = match self.id_to_idx.get(&origin) {
            Some(&idx) => idx,
            None => return 9999,
        };

        let dest_idx = match self.id_to_idx.get(&dest) {
            Some(&idx) => idx,
            None => return 9999,
        };

        if safe_only
            && (self.graph[origin_idx].security_status < 0.45
                || self.graph[dest_idx].security_status < 0.45)
        {
            let mut cache = self.jump_cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.put(cache_key, 9999);
            return 9999;
        }

        let node_count = self.graph.node_count();
        let mut visited = FastBitSet::new(node_count);
        let mut queue = VecDeque::new();
        visited.set(origin_idx.index());
        queue.push_back((origin_idx, 0u32));

        let result = if safe_only {
            let mut found_dist = None;
            while let Some((curr, dist)) = queue.pop_front() {
                if curr == dest_idx {
                    found_dist = Some(dist);
                    break;
                }
                for neighbor in self.graph.neighbors(curr) {
                    let n_idx = neighbor.index();
                    if !visited.get(n_idx) && self.graph[neighbor].security_status >= 0.45 {
                        visited.set(n_idx);
                        queue.push_back((neighbor, dist + 1));
                    }
                }
            }

            found_dist.unwrap_or(9999)
        } else {
            let mut found_dist = None;
            while let Some((curr, dist)) = queue.pop_front() {
                if curr == dest_idx {
                    found_dist = Some(dist);
                    break;
                }
                for neighbor in self.graph.neighbors(curr) {
                    let n_idx = neighbor.index();
                    if !visited.get(n_idx) {
                        visited.set(n_idx);
                        queue.push_back((neighbor, dist + 1));
                    }
                }
            }

            found_dist.unwrap_or(9999)
        };

        {
            let mut cache = self.jump_cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.put(cache_key, result);
        }

        result
    }

    /// Calculates the ordered list of solar system IDs along the shortest stargate path using BFS.
    ///
    /// Returns an empty `Vec` as a sentinel if no path exists, or if `safe_only` is true and
    /// any endpoint or connecting path enters lowsec/nullsec.
    pub fn shortest_path(&self, origin: u32, dest: u32, safe_only: bool) -> Vec<u32> {
        if origin == dest {
            return vec![origin];
        }

        if safe_only && (!self.is_highsec(origin) || !self.is_highsec(dest)) {
            return Vec::new();
        }

        let origin_idx = match self.id_to_idx.get(&origin) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        let dest_idx = match self.id_to_idx.get(&dest) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        let node_count = self.graph.node_count();
        let mut visited = FastBitSet::new(node_count);
        let mut parent = vec![None; node_count];
        let mut queue = VecDeque::new();

        visited.set(origin_idx.index());
        queue.push_back(origin_idx);

        let mut found = false;
        while let Some(curr) = queue.pop_front() {
            if curr == dest_idx {
                found = true;
                break;
            }
            for neighbor in self.graph.neighbors(curr) {
                let n_idx = neighbor.index();
                if !visited.get(n_idx) {
                    if safe_only && self.graph[neighbor].security_status < 0.45 {
                        continue;
                    }
                    visited.set(n_idx);
                    parent[n_idx] = Some(curr);
                    queue.push_back(neighbor);
                }
            }
        }

        if !found {
            return Vec::new();
        }

        let mut path = Vec::new();
        let mut curr = dest_idx;
        while curr != origin_idx {
            path.push(self.graph[curr].system_id);
            curr = match parent[curr.index()] {
                Some(p) => p,
                None => return Vec::new(),
            };
        }
        path.push(self.graph[origin_idx].system_id);
        path.reverse();
        path
    }

    /// Returns true if the internal jump calculation LRU cache mutex is poisoned.
    pub fn is_jump_cache_poisoned(&self) -> bool {
        self.jump_cache.is_poisoned()
    }

    /// Simulates a thread panic to poison the jump cache mutex for reliability verification.
    #[doc(hidden)]
    pub fn poison_jump_cache_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.jump_cache.lock().unwrap();
            panic!("poisoning jump_cache for reliability verification");
        }));
    }
}
