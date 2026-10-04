//! Behavioral tests kept in their own file so the test text stays fixed while
//! the implementation it guards changes (the ticket's RED/GREEN receipts hash
//! this file).

use chrono::{TimeZone, Utc};

use super::key::MemoryKeyStore;
use super::store::{ActivityStore, NewFrame, TextSource, ACTIVITY_DB_FILE};

/// The activity database is unreadable without its Keychain key: a plain
/// SQLite client sees "file is not a database", no captured text appears in
/// the file, and the same key opens it again.
#[tokio::test]
async fn encrypted_file_is_unreadable_without_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(ACTIVITY_DB_FILE);
    let keys = MemoryKeyStore::default();
    let store = ActivityStore::open(&path, &keys).await.expect("open");
    store
        .insert_frame(&NewFrame {
            captured_at: Utc.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap(),
            app_name: "Zed".into(),
            bundle_id: Some("dev.zed.Zed".into()),
            window_title: Some("main.rs".into()),
            browser_url: None,
            text_source: TextSource::Accessibility,
            text: Some("secret meeting notes".into()),
        })
        .await
        .expect("insert");
    store.close().await;

    // A plain SQLite client (no PRAGMA key) must not recognize the file.
    let plain = sqlx_sqlite::SqliteConnectOptions::new().filename(&path);
    let plain_read = match sqlx_sqlite::SqlitePool::connect_with(plain).await {
        Ok(pool) => sqlx::query::query("SELECT count(*) FROM frames")
            .fetch_one(&pool)
            .await
            .map(|_| ()),
        Err(error) => Err(error),
    };
    let error = plain_read.expect_err("plain SQLite must not read the activity database");
    assert!(
        error.to_string().contains("file is not a database"),
        "unexpected error: {error}"
    );
    let raw = std::fs::read(&path).expect("read file");
    assert!(!raw.starts_with(b"SQLite format 3"));
    assert!(!raw.windows(6).any(|window| window == b"secret"));

    // The same key reopens it.
    let reopened = ActivityStore::open(&path, &keys).await.expect("reopen");
    assert_eq!(reopened.frames_after(0, 10).await.unwrap().len(), 1);
}
