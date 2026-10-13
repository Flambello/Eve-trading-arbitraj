use crate::sde::{SdeLoader, SystemNode};
use anyhow::Result;
use lru::LruCache;
use petgraph::algo::{astar, dijkstra};
use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;

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

        let cache = LruCache::new(NonZeroUsize::new(20_000).unwrap());

        Self {
            graph,
            id_to_idx,
            jump_cache: Mutex::new(cache),
        }
    }

    /// Loads all solar systems and stargate jump connections from the SQLite SDE database.
    pub fn from_sde(sde: &SdeLoader) -> Result<Self> {
        let systems = sde.all_systems()?;
        let jumps = sde.all_jumps()?;
        Ok(Self::from_nodes_and_jumps(systems, jumps))
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

        // Check LRU cache
        if let Ok(mut cache) = self.jump_cache.lock() {
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

        let result = if safe_only {
            // Strictly exclude lowsec and nullsec origin or destination
            if !self.is_highsec(origin) || !self.is_highsec(dest) {
                return 9999;
            }

            // Dijkstra where target system security < 0.45 has high weight 1000.0
            let costs = dijkstra(&self.graph, origin_idx, Some(dest_idx), |edge| {
                let target = edge.target();
                let sec = self.graph[target].security_status;
                if sec < 0.45 {
                    1000.0
                } else {
                    1.0
                }
            });

            match costs.get(&dest_idx) {
                Some(&cost) if cost < 1000.0 => cost as u32,
                _ => 9999, // Requires lowsec/nullsec or unreachable
            }
        } else {
            let costs = dijkstra(&self.graph, origin_idx, Some(dest_idx), |_| 1.0);
            match costs.get(&dest_idx) {
                Some(&cost) => cost as u32,
                None => 9999,
            }
        };

        if let Ok(mut cache) = self.jump_cache.lock() {
            cache.put(cache_key, result);
        }

        result
    }

    /// Calculates the ordered list of solar system IDs along the shortest stargate path using A*.
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

        let path_opt = astar(
            &self.graph,
            origin_idx,
            |finish| finish == dest_idx,
            |edge| {
                let target = edge.target();
                let sec = self.graph[target].security_status;
                if safe_only && sec < 0.45 {
                    1000.0
                } else {
                    1.0
                }
            },
            |_| 0.0,
        );

        match path_opt {
            Some((cost, node_path)) => {
                if safe_only && cost >= 1000.0 {
                    Vec::new()
                } else {
                    node_path
                        .into_iter()
                        .map(|idx| self.graph[idx].system_id)
                        .collect()
                }
            }
            None => Vec::new(),
        }
    }
}
