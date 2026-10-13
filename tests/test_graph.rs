use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::sde::SystemNode;

fn create_test_stargate_network() -> SystemGraph {
    let systems = vec![
        SystemNode {
            system_id: 30000142,
            name: "Jita".to_string(),
            security_status: 0.95,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30000144,
            name: "Perimeter".to_string(),
            security_status: 0.90,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30000145,
            name: "Urlen".to_string(),
            security_status: 0.80,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30003491,
            name: "Niarja".to_string(),
            security_status: 0.38, // Lowsec
            region_id: 10000043,
        },
        SystemNode {
            system_id: 30003492,
            name: "Madirmilire".to_string(),
            security_status: 0.70, // Highsec
            region_id: 10000043,
        },
        SystemNode {
            system_id: 30002187,
            name: "Amarr".to_string(),
            security_status: 0.90,
            region_id: 10000043,
        },
    ];

    // Stargate jumps:
    // Unsafe route: Jita <-> Perimeter <-> Niarja (lowsec) <-> Amarr (3 jumps total)
    // Safe route:   Jita <-> Perimeter <-> Urlen <-> Madirmilire <-> Amarr (4 jumps total)
    let jumps = vec![
        (30000142, 30000144), // Jita <-> Perimeter
        (30000144, 30003491), // Perimeter <-> Niarja (lowsec shortcut)
        (30003491, 30002187), // Niarja <-> Amarr
        (30000144, 30000145), // Perimeter <-> Urlen (safe detour link 1)
        (30000145, 30003492), // Urlen <-> Madirmilire (safe detour link 2)
        (30003492, 30002187), // Madirmilire <-> Amarr (safe detour link 3)
    ];

    SystemGraph::from_nodes_and_jumps(systems, jumps)
}

#[test]
fn test_system_security_and_is_highsec() {
    let graph = create_test_stargate_network();

    assert_eq!(graph.security(30000142), Some(0.95));
    assert!(graph.is_highsec(30000142)); // Jita

    assert_eq!(graph.security(30000144), Some(0.90));
    assert!(graph.is_highsec(30000144)); // Perimeter

    assert_eq!(graph.security(30003491), Some(0.38));
    assert!(!graph.is_highsec(30003491)); // Niarja (Lowsec)

    assert_eq!(graph.security(99999999), None);
    assert!(!graph.is_highsec(99999999));
}

#[test]
fn test_jump_counts_normal_and_safe_mode() {
    let graph = create_test_stargate_network();

    let jita = 30000142;
    let perimeter = 30000144;
    let amarr = 30002187;
    let niarja_lowsec = 30003491;

    // Same system is 0 jumps
    assert_eq!(graph.jump_count(jita, jita, false), 0);
    assert_eq!(graph.jump_count(jita, jita, true), 0);

    // Direct neighbor is 1 jump
    assert_eq!(graph.jump_count(jita, perimeter, false), 1);
    assert_eq!(graph.jump_count(jita, perimeter, true), 1);

    // In normal mode (safe_only = false): Jita -> Perimeter -> Niarja (lowsec) -> Amarr = 3 jumps
    assert_eq!(graph.jump_count(jita, amarr, false), 3);

    // In safe mode (safe_only = true): routes around lowsec via Urlen & Madirmilire = 4 jumps
    assert_eq!(graph.jump_count(jita, amarr, true), 4);

    // Jita to Lowsec (Niarja):
    // In normal mode: Jita -> Perimeter -> Niarja = 2 jumps
    assert_eq!(graph.jump_count(jita, niarja_lowsec, false), 2);

    // In safe mode: lowsec endpoint is strictly rejected (returns 9999)
    assert_eq!(graph.jump_count(jita, niarja_lowsec, true), 9999);
    assert_eq!(graph.jump_count(niarja_lowsec, jita, true), 9999);
}

#[test]
fn test_shortest_path_safe_highsec_only() {
    let graph = create_test_stargate_network();

    let jita = 30000142;
    let perimeter = 30000144;
    let urlen = 30000145;
    let madirmilire = 30003492;
    let amarr = 30002187;
    let niarja_lowsec = 30003491;

    // Normal mode: shortest path uses lowsec shortcut (Jita -> Perimeter -> Niarja -> Amarr)
    let unsafe_path = graph.shortest_path(jita, amarr, false);
    assert_eq!(unsafe_path, vec![jita, perimeter, niarja_lowsec, amarr]);

    // Safe mode: shortest path diverts around lowsec (Jita -> Perimeter -> Urlen -> Madirmilire -> Amarr)
    let safe_path = graph.shortest_path(jita, amarr, true);
    assert_eq!(safe_path, vec![jita, perimeter, urlen, madirmilire, amarr]);
    assert!(!safe_path.contains(&niarja_lowsec));

    // Attempting safe path to lowsec destination must return empty
    let invalid_path = graph.shortest_path(jita, niarja_lowsec, true);
    assert!(
        invalid_path.is_empty(),
        "Path to lowsec must be empty in safe mode"
    );
}

#[test]
fn test_adjacent_regions() {
    let graph = create_test_stargate_network();

    let the_forge = 10000002;
    let domain = 10000043;

    let neighbors_safe = graph.adjacent_regions(the_forge, true);
    assert_eq!(neighbors_safe, vec![domain]);

    let domain_neighbors = graph.adjacent_regions(domain, true);
    assert_eq!(domain_neighbors, vec![the_forge]);

    let nonexistent = graph.adjacent_regions(99999999, true);
    assert!(nonexistent.is_empty());
}

#[test]
fn test_disconnected_components_and_unknown_systems() {
    // Two disconnected highsec components:
    // Component 1: Jita (30000142) <-> Perimeter (30000144)
    // Component 2: Kisogo (30000200) <-> Maurasi (30000201) (isolated island)
    let systems = vec![
        SystemNode {
            system_id: 30000142,
            name: "Jita".to_string(),
            security_status: 0.95,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30000144,
            name: "Perimeter".to_string(),
            security_status: 0.90,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30000200,
            name: "Kisogo".to_string(),
            security_status: 0.85,
            region_id: 10000003,
        },
        SystemNode {
            system_id: 30000201,
            name: "Maurasi".to_string(),
            security_status: 0.75,
            region_id: 10000003,
        },
    ];

    let jumps = vec![
        (30000142, 30000144), // Component 1 internal link
        (30000200, 30000201), // Component 2 internal link
    ];

    let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

    let jita = 30000142;
    let perimeter = 30000144;
    let kisogo = 30000200;
    let maurasi = 30000201;
    let unknown_sys = 99999999;

    // Within component 1: reachable
    assert_eq!(graph.jump_count(jita, perimeter, false), 1);
    assert_eq!(graph.jump_count(jita, perimeter, true), 1);
    assert_eq!(graph.shortest_path(jita, perimeter, true), vec![jita, perimeter]);

    // Within component 2: reachable
    assert_eq!(graph.jump_count(kisogo, maurasi, false), 1);
    assert_eq!(graph.jump_count(kisogo, maurasi, true), 1);
    assert_eq!(graph.shortest_path(kisogo, maurasi, true), vec![kisogo, maurasi]);

    // Across disconnected components: must return 9999 jumps and empty shortest path
    assert_eq!(graph.jump_count(jita, kisogo, false), 9999);
    assert_eq!(graph.jump_count(jita, kisogo, true), 9999);
    assert!(graph.shortest_path(jita, kisogo, false).is_empty());
    assert!(graph.shortest_path(jita, kisogo, true).is_empty());

    assert_eq!(graph.jump_count(perimeter, maurasi, false), 9999);
    assert_eq!(graph.jump_count(perimeter, maurasi, true), 9999);
    assert!(graph.shortest_path(perimeter, maurasi, false).is_empty());
    assert!(graph.shortest_path(perimeter, maurasi, true).is_empty());

    // Unknown system ID: must return 9999 jumps and empty path
    assert_eq!(graph.jump_count(jita, unknown_sys, false), 9999);
    assert_eq!(graph.jump_count(jita, unknown_sys, true), 9999);
    assert_eq!(graph.jump_count(unknown_sys, jita, false), 9999);
    assert_eq!(graph.jump_count(unknown_sys, jita, true), 9999);
    assert!(graph.shortest_path(jita, unknown_sys, false).is_empty());
    assert!(graph.shortest_path(jita, unknown_sys, true).is_empty());
    assert!(graph.shortest_path(unknown_sys, jita, true).is_empty());
}
