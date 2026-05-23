use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use refinery::embed_migrations;
use rusqlite::Connection;
use tracing::info;

embed_migrations!("migrations");

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(())
    }
}

pub fn init_pool(database_url: &str) -> DbPool {
    let manager = SqliteConnectionManager::file(database_url);
    let pool = Pool::builder()
        .max_size(10)
        .connection_customizer(Box::new(PragmaCustomizer))
        .build(manager)
        .expect("Failed to create database connection pool");

    run_migrations(&pool);

    info!("Database initialized successfully");
    pool
}

fn run_migrations(pool: &DbPool) {
    let mut conn = pool.get().expect("Failed to get connection for migrations");
    migrations::runner()
        .run(&mut *conn)
        .expect("Failed to run database migrations");
    info!("Database migrations completed");
}
