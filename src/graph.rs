#![allow(dead_code)]

use crate::sde::{SdeLoader, SystemNode};
use anyhow::Result;
use lru::LruCache;
use petgraph::algo::{astar, dijkstra};
use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;

pub struct SystemGraph {
    graph: UnGraph<SystemNode, f64>,
    id_to_idx: HashMap<u32, NodeIndex>,
    jump_cache: Mutex<LruCache<(u32, u32, bool), u32>>,
}

impl SystemGraph {
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

    pub fn from_sde(sde: &SdeLoader) -> Result<Self> {
        let systems = sde.all_systems()?;
        let jumps = sde.all_jumps()?;
        Ok(Self::from_nodes_and_jumps(systems, jumps))
    }

    pub fn security(&self, system_id: u32) -> Option<f64> {
        let idx = self.id_to_idx.get(&system_id)?;
        Some(self.graph[*idx].security_status)
    }

    pub fn system_name(&self, system_id: u32) -> Option<&str> {
        let idx = self.id_to_idx.get(&system_id)?;
        Some(self.graph[*idx].name.as_str())
    }

    pub fn is_highsec(&self, system_id: u32) -> bool {
        self.security(system_id).map(|s| s >= 0.45).unwrap_or(false)
    }

    pub fn jump_count(&self, origin: u32, dest: u32, safe_only: bool) -> u32 {
        if origin == dest {
            return 0;
        }

        // Check LRU cache
        if let Ok(mut cache) = self.jump_cache.lock() {
            if let Some(&jumps) = cache.get(&(origin, dest, safe_only)) {
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
            cache.put((origin, dest, safe_only), result);
        }

        result
    }

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
