use eframe::egui;
use eve_trade_scout::config::UserConfig;
use eve_trade_scout::esi::sso::{
    ensure_fresh_token_opts, save_token, token_storage_path, SsoToken,
};
use eve_trade_scout::sde::SdeLoader;
use eve_trade_scout::ui::{DesktopApp, StartupMsg};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    var: &'static str,
    orig: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(var: &'static str, val: &std::path::Path) -> Self {
        let orig = std::env::var_os(var);
        std::env::set_var(var, val);
        Self { var, orig }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(ref val) = self.orig {
            std::env::set_var(self.var, val);
        } else {
            std::env::remove_var(self.var);
        }
    }
}

struct ConfigDirGuard {
    _xdg: EnvGuard,
    _eve: EnvGuard,
}

impl ConfigDirGuard {
    fn set(path: &std::path::Path) -> Self {
        Self {
            _xdg: EnvGuard::set("XDG_CONFIG_HOME", path),
            _eve: EnvGuard::set("EVE_CONFIG_DIR", path),
        }
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[test]
fn test_scan_worker_disconnect_recovers_state() {
    let (_startup_tx, startup_rx) = mpsc::channel::<StartupMsg>();
    let mut app = DesktopApp::new_loading(&UserConfig::default(), startup_rx);

    let (scan_tx, scan_rx) = mpsc::channel();
    app.set_scan_rx_for_test(scan_rx);
    assert!(app.is_scanning(), "App must report scanning actively");

    // Simulate worker thread crash or early return: sender is dropped
    drop(scan_tx);

    let ctx = egui::Context::default();
    app.poll_active_workers(&ctx);

    assert!(
        !app.is_scanning(),
        "is_scanning must reset to false upon worker disconnect"
    );
    assert!(
        app.status_text()
            .contains("Market scan worker terminated unexpectedly"),
        "Status text must reflect worker termination"
    );
    assert!(
        app.toast_text()
            .unwrap_or_default()
            .contains("Market scan worker failed unexpectedly"),
        "Toast must inform user of worker failure"
    );
}

#[test]
fn test_chain_worker_disconnect_recovers_state() {
    let (_startup_tx, startup_rx) = mpsc::channel::<StartupMsg>();
    let mut app = DesktopApp::new_loading(&UserConfig::default(), startup_rx);

    let (chain_tx, chain_rx) = mpsc::channel();
    app.set_chain_rx_for_test(chain_rx);
    assert!(
        app.is_chain_running(),
        "App must report chain route running actively"
    );

    // Simulate worker thread crash: sender is dropped
    drop(chain_tx);

    let ctx = egui::Context::default();
    app.poll_active_workers(&ctx);

    assert!(
        !app.is_chain_running(),
        "chain_is_running must reset to false upon worker disconnect"
    );
    assert!(
        app.toast_text()
            .unwrap_or_default()
            .contains("Chain route calculation failed unexpectedly"),
        "Toast must inform user of chain route worker failure"
    );
}

#[test]
fn test_sso_workers_disconnect_recovers_state() {
    let (_startup_tx, startup_rx) = mpsc::channel::<StartupMsg>();
    let mut app = DesktopApp::new_loading(&UserConfig::default(), startup_rx);
    let ctx = egui::Context::default();

    // 1. SSO Token Refresh Worker
    {
        let (tx, rx) = mpsc::channel();
        app.set_sso_refresh_rx_for_test(rx);
        assert!(app.is_sso_refreshing());
        drop(tx);
        app.poll_active_workers(&ctx);
        assert!(
            !app.is_sso_refreshing(),
            "sso_is_refreshing must reset to false"
        );
    }

    // 2. SSO Authentication Worker
    {
        let (tx, rx) = mpsc::channel();
        app.set_sso_auth_rx_for_test(rx);
        assert!(app.is_sso_authenticating());
        drop(tx);
        app.poll_active_workers(&ctx);
        assert!(
            !app.is_sso_authenticating(),
            "sso_is_authenticating must reset to false"
        );
        assert!(
            app.toast_text()
                .unwrap_or_default()
                .contains("SSO Authentication worker failed unexpectedly"),
            "Toast must inform user of SSO auth worker failure"
        );
    }

    // 3. SSO Waypoint Push Worker
    {
        let (tx, rx) = mpsc::channel();
        app.set_sso_push_rx_for_test(rx);
        assert!(app.is_sso_pushing());
        drop(tx);
        app.poll_active_workers(&ctx);
        assert!(!app.is_sso_pushing(), "sso_is_pushing must reset to false");
        assert!(
            app.toast_text()
                .unwrap_or_default()
                .contains("Autopilot injection worker failed unexpectedly"),
            "Toast must inform user of waypoint push worker failure"
        );
    }

    // 4. SSO Character Location Worker
    {
        let (tx, rx) = mpsc::channel();
        app.set_sso_location_rx_for_test(rx);
        assert!(app.is_sso_fetching_location());
        drop(tx);
        app.poll_active_workers(&ctx);
        assert!(
            !app.is_sso_fetching_location(),
            "sso_is_fetching_location must reset to false"
        );
        assert!(
            app.toast_text()
                .unwrap_or_default()
                .contains("Character location sync failed unexpectedly"),
            "Toast must inform user of character location worker failure"
        );
    }
}

#[test]
fn test_sde_loader_item_cache_poison_recovery_and_persistence() {
    let sde = Arc::new(
        SdeLoader::in_memory_with_fixtures().expect("Failed to initialize in-memory SDE fixtures"),
    );

    // Initial item query works and populates cache
    let tritanium = sde
        .get_item_info(34)
        .expect("Tritanium must exist in fixture");
    assert_eq!(tritanium.name, "Tritanium");

    // Intentionally poison the item_cache mutex to simulate a worker thread panic
    sde.poison_item_cache_for_test();
    assert!(
        sde.is_item_cache_poisoned(),
        "item_cache mutex must be poisoned"
    );

    // 1. Verify get_item_info recovers from poisoned mutex without panicking
    let tritanium_recovered = sde
        .get_item_info(34)
        .expect("Tritanium must be returned even after mutex poisoning");
    assert_eq!(tritanium_recovered.name, "Tritanium");

    // 2. Verify cache insertion still functions under poison: query an uncached item (Pyerite: 35)
    let pyerite = sde
        .get_item_info(35)
        .expect("Pyerite must exist in fixture");
    assert_eq!(pyerite.name, "Pyerite");

    // 3. Query Pyerite a second time to ensure the newly cached value is retained and retrieved
    let pyerite_cached = sde
        .get_item_info(35)
        .expect("Pyerite must be retrieved from cache");
    assert_eq!(pyerite_cached.name, "Pyerite");
}

#[test]
fn test_sde_loader_concurrent_item_cache_access() {
    let sde = Arc::new(
        SdeLoader::in_memory_with_fixtures().expect("Failed to initialize in-memory SDE fixtures"),
    );

    let mut handles = Vec::new();

    for thread_id in 0..8 {
        let sde_clone = Arc::clone(&sde);
        let handle = std::thread::spawn(move || {
            for i in 0..100 {
                // If thread 0 on iteration 20, induce mutex poison mid-flight
                if thread_id == 0 && i == 20 {
                    sde_clone.poison_item_cache_for_test();
                }

                let id = if i % 2 == 0 { 34 } else { 35 };
                let item = sde_clone.get_item_info(id);
                assert!(item.is_some(), "Item {id} must resolve successfully");
                let expected_name = if id == 34 { "Tritanium" } else { "Pyerite" };
                assert_eq!(item.unwrap().name, expected_name);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().expect("Worker thread panicked unexpectedly");
    }
}

#[test]
fn test_sso_concurrent_save_token_atomic_tmp_uniqueness() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_token_concurrent_save_{}_{}",
        std::process::id(),
        now_secs()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("Must create test directory");
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    let mut handles = Vec::new();

    for i in 0..15 {
        let handle = std::thread::spawn(move || {
            let token = SsoToken {
                access_token: format!("access_token_{i}"),
                refresh_token: format!("refresh_token_{i}"),
                token_type: "Bearer".to_string(),
                expires_at: now_secs() + 1200 + i as i64,
                character_id: 2110000000 + i as u64,
                character_name: format!("Pilot {i}"),
                client_id: Some("test_client".to_string()),
            };
            save_token(&token)
        });
        handles.push(handle);
    }

    for handle in handles {
        let res = handle.join().expect("Thread panicked");
        assert!(res.is_ok(), "Concurrent save_token must succeed: {:?}", res);
    }

    // Verify token file exists and is valid JSON
    let path = token_storage_path();
    assert!(path.exists(), "Target token.json must exist");

    let content = std::fs::read_to_string(&path).expect("Must read token.json");
    let parsed: SsoToken = serde_json::from_str(&content).expect("Must deserialize valid SsoToken");
    assert!(parsed.character_id >= 2110000000);

    // Verify no temporary files remain in the storage directory
    let parent = path.parent().expect("Storage directory must exist");
    for entry in std::fs::read_dir(parent).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".tmp."),
            "No temporary atomic write files should linger in directory: {name}"
        );
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sso_concurrent_ensure_fresh_token_double_checked_locking() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_sso_refresh_dcl_{}_{}",
        std::process::id(),
        now_secs()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    // Initial expired token saved to disk
    let initial_expired_token = SsoToken {
        access_token: "expired_access_token".to_string(),
        refresh_token: "initial_refresh_token".to_string(),
        token_type: "Bearer".to_string(),
        expires_at: now_secs() - 600, // expired 10 minutes ago
        character_id: 2115555555,
        character_name: "Concurrency Tester".to_string(),
        client_id: Some("test_client".to_string()),
    };
    save_token(&initial_expired_token).expect("Must save initial token");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mock_token_url = format!("http://{}/v2/oauth/token", addr);

    let refresh_count = Arc::new(AtomicUsize::new(0));
    let srv_count = Arc::clone(&refresh_count);

    // Server loop handling token refresh requests
    let server_task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = vec![0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }

            srv_count.fetch_add(1, Ordering::SeqCst);

            // Return freshly rotated token
            let body = r#"{"access_token":"brand_new_access_token","token_type":"Bearer","expires_in":1200,"refresh_token":"rotated_refresh_token"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
        }
    });

    let client = reqwest::Client::new();
    let mut tasks = Vec::new();

    // Spawn 5 concurrent tasks simultaneously attempting to ensure fresh token for the same character
    for _ in 0..5 {
        let client_clone = client.clone();
        let url = mock_token_url.clone();
        let mut task_token = initial_expired_token.clone();

        let handle = tokio::spawn(async move {
            ensure_fresh_token_opts(&client_clone, &url, "test_client", &mut task_token, false)
                .await
                .map(|_| task_token)
        });
        tasks.push(handle);
    }

    for handle in tasks {
        let res = handle.await.expect("Task failed");
        assert!(
            res.is_ok(),
            "ensure_fresh_token_opts must succeed: {:?}",
            res
        );
        let fresh_tok = res.unwrap();
        assert_eq!(fresh_tok.access_token, "brand_new_access_token");
        assert_eq!(fresh_tok.refresh_token, "rotated_refresh_token");
        assert!(!fresh_tok.needs_refresh(), "Token must now be fresh");
    }

    // Crucial concurrency check: The mock server must have been called exactly 1 time!
    // The other 4 tasks must have been serialized by REFRESH_MUTEX and used the double-checked locking
    // branch to reload the saved token from disk, preventing race conditions with the OAuth server!
    let total_refreshes = refresh_count.load(Ordering::SeqCst);
    assert_eq!(
        total_refreshes, 1,
        "Double-checked locking must ensure exactly 1 network request on concurrent token refreshes, but saw {total_refreshes}"
    );

    server_task.abort();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
