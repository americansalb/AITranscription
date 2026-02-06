use rusqlite::{Connection, Result};
use std::path::PathBuf;
use std::sync::Mutex;

/// Global database connection wrapped in a mutex for thread safety
pub static DB: once_cell::sync::Lazy<Mutex<Option<Connection>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(None));

/// Get the path to the queue database file
pub fn get_database_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let mut path = PathBuf::from(appdata);
            path.push("Vaak");
            std::fs::create_dir_all(&path).ok()?;
            path.push("queue.db");
            return Some(path);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let mut path = PathBuf::from(home);
            path.push(".vaak");
            std::fs::create_dir_all(&path).ok()?;
            path.push("queue.db");
            return Some(path);
        }
    }

    None
}

/// Initialize the database connection and create tables
pub fn init_database() -> Result<(), String> {
    let db_path = get_database_path().ok_or("Failed to get database path")?;

    eprintln!("Vaak: Initializing queue database at {:?}", db_path);

    let conn = Connection::open(&db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Create queue_items table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS queue_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uuid TEXT UNIQUE NOT NULL,
            session_id TEXT NOT NULL,
            text TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            position INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,
            duration_ms INTEGER,
            error_message TEXT
        )",
        [],
    ).map_err(|e| format!("Failed to create queue_items table: {}", e))?;

    // Create indexes for efficient queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_queue_status ON queue_items(status)",
        [],
    ).map_err(|e| format!("Failed to create status index: {}", e))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_queue_position ON queue_items(position)",
        [],
    ).map_err(|e| format!("Failed to create position index: {}", e))?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_queue_created ON queue_items(created_at DESC)",
        [],
    ).map_err(|e| format!("Failed to create created_at index: {}", e))?;

    // Store the connection
    let mut db = DB.lock().map_err(|e| format!("Failed to lock database: {}", e))?;
    *db = Some(conn);

    eprintln!("Vaak: Queue database initialized successfully");

    Ok(())
}

/// Execute a function with the database connection
pub fn with_db<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&Connection) -> Result<T, String>,
{
    let db = DB.lock().map_err(|e| format!("Failed to lock database: {}", e))?;
    let conn = db.as_ref().ok_or("Database not initialized")?;
    f(conn)
}
