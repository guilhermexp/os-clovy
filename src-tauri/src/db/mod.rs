pub mod migrations;
pub mod repositories;

/// Close SQLite workers, including returns racing SQLx 0.8's final idle drain.
pub(crate) async fn close_sqlite_pool(pool: &sqlx_sqlite::SqlitePool) {
    pool.close().await;
    // The first close rejects acquisitions and collects all permits, but a
    // returning connection can have entered the idle queue after its last
    // drain. With all permits returned, this terminal drain is stable and
    // awaits the remaining SQLite worker's native shutdown.
    if pool.size() != 0 {
        pool.close().await;
    }
}
