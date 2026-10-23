use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::sde::{SdeLoader, SystemNode, TempFileCleanupGuard};
use std::sync::Arc;

#[test]
fn test_temp_file_cleanup_guard_removes_on_error_and_preserves_on_disarm() {
    let temp_dir = std::env::temp_dir().join(format!("eve_rel_guard_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let test_file = temp_dir.join("test_download.part");
    std::fs::write(&test_file, b"partial download data").expect("Failed to write test file");
    assert!(test_file.exists());

    // 1. Simulate an early return / error: guard drops without disarm
    {
        let _guard = TempFileCleanupGuard::new(&test_file);
        // Simulate error exit
    }
    assert!(
        !test_file.exists(),
        "Guard must automatically remove temporary file when dropped without disarm"
    );

    // 2. Simulate success: guard is explicitly disarmed before drop
    std::fs::write(&test_file, b"completed validated data").expect("Failed to write test file");
    assert!(test_file.exists());
    {
        let guard = TempFileCleanupGuard::new(&test_file);
        // Successful validation and atomic rename simulated: disarm guard
        guard.disarm();
    }
    assert!(
        test_file.exists(),
        "Guard must preserve temporary file when explicitly disarmed"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_graph_jump_cache_poison_recovery() {
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
    ];
    let jumps = vec![(30000142, 30000144)];
    let graph = Arc::new(SystemGraph::from_nodes_and_jumps(systems, jumps));

    // Populate initial cache
    let initial_jumps = graph.jump_count(30000142, 30000144, true);
    assert_eq!(initial_jumps, 1);

    // Intentionally poison graph.jump_cache mutex to simulate worker thread panic
    graph.poison_jump_cache_for_test();

    assert!(
        graph.is_jump_cache_poisoned(),
        "jump_cache mutex must be poisoned by simulated worker panic"
    );

    // Verify jump_count recovers from the poisoned mutex without panicking
    let recovered_jumps = graph.jump_count(30000142, 30000144, true);
    assert_eq!(
        recovered_jumps, 1,
        "jump_count must recover from poisoned mutex and return correct distance"
    );

    // Verify cache can still receive new entries after poisoning recovery
    let same_system = graph.jump_count(30000142, 30000142, true);
    assert_eq!(same_system, 0);
}

#[test]
fn test_sde_loader_mutex_poison_recovery() {
    let sde = Arc::new(
        SdeLoader::in_memory_with_fixtures().expect("Failed to initialize in-memory SDE fixtures"),
    );

    // Verify baseline query works
    assert_eq!(sde.get_system_id("Jita"), Some(30000142));

    // Intentionally poison sde.conn mutex to simulate worker thread panic
    sde.poison_conn_for_test();

    assert!(
        sde.is_conn_poisoned(),
        "sde.conn mutex must be poisoned by simulated worker panic"
    );

    // 1. Verify get_system_id recovers from poisoned connection
    assert_eq!(
        sde.get_system_id("Jita"),
        Some(30000142),
        "get_system_id must recover from poisoned mutex"
    );

    // 2. Verify all_systems recovers from poisoned connection
    let systems = sde
        .all_systems()
        .expect("all_systems must recover from poisoned mutex");
    assert!(!systems.is_empty(), "Systems list must not be empty");

    // 3. Verify all_jumps recovers from poisoned connection
    let jumps = sde
        .all_jumps()
        .expect("all_jumps must recover from poisoned mutex");
    assert!(!jumps.is_empty(), "Jumps list must not be empty");

    // 4. Verify get_item_info recovers from poisoned connection
    let item = sde
        .get_item_info(34)
        .expect("Item info query must recover from poisoned mutex");
    assert_eq!(item.name, "Tritanium");
}

#[tokio::test]
async fn test_regional_market_fetch_aborts_tasks_on_network_error() {
    use eve_trade_scout::esi::fetch_markets_opts;
    use eve_trade_scout::esi::EsiClient;

    let temp_cache = std::env::temp_dir().join(format!("eve_market_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_cache);

    let client = EsiClient::new(&temp_cache).expect("Client init must succeed");

    // Pass invalid region IDs to trigger network failure and task abortion
    let res = fetch_markets_opts(&client, &[999999991, 999999992, 999999993], true).await;
    assert!(
        res.is_err(),
        "fetch_markets_opts must return error on invalid regions"
    );

    let _ = std::fs::remove_dir_all(&temp_cache);
}

#[tokio::test]
async fn test_save_cache_cleans_up_tmp_file_on_error() {
    use eve_trade_scout::esi::EsiClient;

    let temp_cache_dir =
        std::env::temp_dir().join(format!("eve_save_cache_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_cache_dir);

    let client = EsiClient::new(&temp_cache_dir).expect("Client init must succeed");

    // Create a directory at the destination path `etags.json` so that renaming tmp_file to etags_file fails
    let target_etags_file = temp_cache_dir.join("etags.json");
    std::fs::create_dir(&target_etags_file)
        .expect("Failed to create blocking dir at target path");

    let res = client.save_cache().await;
    assert!(
        res.is_err(),
        "save_cache must fail when target rename destination is a directory"
    );

    let tmp_file = temp_cache_dir.join("etags.json.tmp");
    assert!(
        !tmp_file.exists(),
        "etags.json.tmp must be cleaned up and not exist after failed save_cache"
    );

    let _ = std::fs::remove_dir_all(&temp_cache_dir);
}
