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
    // Jita <-> Perimeter <-> Urlen <-> Niarja (lowsec shortcut) <-> Madirmilire <-> Amarr
    // Also a safe highsec bypass: Urlen <-> Madirmilire directly (simulating longer highsec detour)
    let jumps = vec![
        (30000142, 30000144), // Jita <-> Perimeter
        (30000144, 30000145), // Perimeter <-> Urlen
        (30000145, 30003491), // Urlen <-> Niarja (lowsec)
        (30003491, 30003492), // Niarja <-> Madirmilire
        (30000145, 30003492), // Urlen <-> Madirmilire (safe highsec link)
        (30003492, 30002187), // Madirmilire <-> Amarr
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

    // Jita to Amarr via safe highsec link (Jita -> Perimeter -> Urlen -> Madirmilire -> Amarr) = 4 jumps
    assert_eq!(graph.jump_count(jita, amarr, true), 4);

    // Jita to Lowsec (Niarja):
    // In normal mode (safe_only = false): Jita -> Perimeter -> Urlen -> Niarja = 3 jumps
    assert_eq!(graph.jump_count(jita, niarja_lowsec, false), 3);

    // In safe mode (safe_only = true): lowsec endpoint is strictly rejected (returns 9999)
    assert_eq!(graph.jump_count(jita, niarja_lowsec, true), 9999);
    assert_eq!(graph.jump_count(niarja_lowsec, jita, true), 9999);
}

#[test]
fn test_shortest_path_safe_highsec_only() {
    let graph = create_test_stargate_network();

    let jita = 30000142;
    let amarr = 30002187;
    let niarja_lowsec = 30003491;

    // Shortest path Jita to Amarr in safe mode
    let path = graph.shortest_path(jita, amarr, true);
    assert_eq!(path.len(), 5);
    assert_eq!(path.first(), Some(&jita));
    assert_eq!(path.last(), Some(&amarr));

    // Niarja (lowsec) cannot be part of safe highsec path
    assert!(!path.contains(&niarja_lowsec));

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
