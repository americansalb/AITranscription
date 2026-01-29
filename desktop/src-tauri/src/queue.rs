use crate::database::with_db;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Queue item structure matching the database schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: i64,
    pub uuid: String,
    pub session_id: String,
    pub text: String,
    pub status: String,
    pub position: i64,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub error_message: Option<String>,
}

/// Get current timestamp in milliseconds
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Add a new item to the queue
#[tauri::command]
pub fn add_queue_item(text: String, session_id: String) -> Result<QueueItem, String> {
    let uuid = Uuid::new_v4().to_string();
    let created_at = now_ms();

    with_db(|conn| {
        // Get the next position (max position + 1)
        let position: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM queue_items WHERE status IN ('pending', 'playing', 'paused')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);

        conn.execute(
            "INSERT INTO queue_items (uuid, session_id, text, status, position, created_at) VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
            params![uuid, session_id, text, position, created_at],
        )
        .map_err(|e| format!("Failed to insert queue item: {}", e))?;

        let id = conn.last_insert_rowid();

        Ok(QueueItem {
            id,
            uuid,
            session_id,
            text,
            status: "pending".to_string(),
            position,
            created_at,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            error_message: None,
        })
    })
}

/// Get queue items with optional filtering
#[tauri::command]
pub fn get_queue_items(
    status: Option<String>,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<QueueItem>, String> {
    with_db(|conn| {
        let mut sql = String::from(
            "SELECT id, uuid, session_id, text, status, position, created_at, started_at, completed_at, duration_ms, error_message FROM queue_items WHERE 1=1",
        );

        if status.is_some() {
            sql.push_str(" AND status = ?1");
        }
        if session_id.is_some() {
            sql.push_str(if status.is_some() {
                " AND session_id = ?2"
            } else {
                " AND session_id = ?1"
            });
        }

        sql.push_str(" ORDER BY CASE WHEN status IN ('pending', 'playing', 'paused') THEN position ELSE created_at END");

        if let Some(l) = limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let items: Vec<QueueItem> = match (&status, &session_id) {
            (Some(s), Some(sid)) => stmt
                .query_map(params![s, sid], |row| {
                    Ok(QueueItem {
                        id: row.get(0)?,
                        uuid: row.get(1)?,
                        session_id: row.get(2)?,
                        text: row.get(3)?,
                        status: row.get(4)?,
                        position: row.get(5)?,
                        created_at: row.get(6)?,
                        started_at: row.get(7)?,
                        completed_at: row.get(8)?,
                        duration_ms: row.get(9)?,
                        error_message: row.get(10)?,
                    })
                })
                .map_err(|e| format!("Query failed: {}", e))?
                .filter_map(|r| r.ok())
                .collect(),
            (Some(s), None) => stmt
                .query_map(params![s], |row| {
                    Ok(QueueItem {
                        id: row.get(0)?,
                        uuid: row.get(1)?,
                        session_id: row.get(2)?,
                        text: row.get(3)?,
                        status: row.get(4)?,
                        position: row.get(5)?,
                        created_at: row.get(6)?,
                        started_at: row.get(7)?,
                        completed_at: row.get(8)?,
                        duration_ms: row.get(9)?,
                        error_message: row.get(10)?,
                    })
                })
                .map_err(|e| format!("Query failed: {}", e))?
                .filter_map(|r| r.ok())
                .collect(),
            (None, Some(sid)) => stmt
                .query_map(params![sid], |row| {
                    Ok(QueueItem {
                        id: row.get(0)?,
                        uuid: row.get(1)?,
                        session_id: row.get(2)?,
                        text: row.get(3)?,
                        status: row.get(4)?,
                        position: row.get(5)?,
                        created_at: row.get(6)?,
                        started_at: row.get(7)?,
                        completed_at: row.get(8)?,
                        duration_ms: row.get(9)?,
                        error_message: row.get(10)?,
                    })
                })
                .map_err(|e| format!("Query failed: {}", e))?
                .filter_map(|r| r.ok())
                .collect(),
            (None, None) => stmt
                .query_map([], |row| {
                    Ok(QueueItem {
                        id: row.get(0)?,
                        uuid: row.get(1)?,
                        session_id: row.get(2)?,
                        text: row.get(3)?,
                        status: row.get(4)?,
                        position: row.get(5)?,
                        created_at: row.get(6)?,
                        started_at: row.get(7)?,
                        completed_at: row.get(8)?,
                        duration_ms: row.get(9)?,
                        error_message: row.get(10)?,
                    })
                })
                .map_err(|e| format!("Query failed: {}", e))?
                .filter_map(|r| r.ok())
                .collect(),
        };

        Ok(items)
    })
}

/// Update queue item status
#[tauri::command]
pub fn update_queue_item_status(
    uuid: String,
    status: String,
    duration_ms: Option<i64>,
    error_message: Option<String>,
) -> Result<(), String> {
    eprintln!("Vaak: [Queue] update_queue_item_status called - uuid={}, status={}", uuid, status);

    with_db(|conn| {
        let now = now_ms();

        match status.as_str() {
            "playing" => {
                let rows = conn.execute(
                    "UPDATE queue_items SET status = ?1, started_at = ?2 WHERE uuid = ?3",
                    params![status, now, uuid],
                )
                .map_err(|e| format!("Failed to update status: {}", e))?;
                eprintln!("Vaak: [Queue] Updated {} row(s) to playing", rows);
            }
            "completed" => {
                let rows = conn.execute(
                    "UPDATE queue_items SET status = ?1, completed_at = ?2, duration_ms = ?3 WHERE uuid = ?4",
                    params![status, now, duration_ms, uuid],
                )
                .map_err(|e| format!("Failed to update status: {}", e))?;
                eprintln!("Vaak: [Queue] Updated {} row(s) to completed, duration_ms={:?}", rows, duration_ms);
            }
            "failed" => {
                conn.execute(
                    "UPDATE queue_items SET status = ?1, completed_at = ?2, error_message = ?3 WHERE uuid = ?4",
                    params![status, now, error_message, uuid],
                )
                .map_err(|e| format!("Failed to update status: {}", e))?;
            }
            _ => {
                conn.execute(
                    "UPDATE queue_items SET status = ?1 WHERE uuid = ?2",
                    params![status, uuid],
                )
                .map_err(|e| format!("Failed to update status: {}", e))?;
            }
        }

        Ok(())
    })
}

/// Reorder a queue item to a new position
#[tauri::command]
#[allow(non_snake_case)]
pub fn reorder_queue_item(uuid: String, newPosition: i64) -> Result<(), String> {
    eprintln!("[Queue] reorder_queue_item called: uuid={}, newPosition={}", uuid, newPosition);
    let new_position = newPosition;
    with_db(|conn| {
        // Get current position
        let current_position: i64 = conn
            .query_row(
                "SELECT position FROM queue_items WHERE uuid = ?1",
                params![uuid],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to get current position: {}", e))?;

        if current_position == new_position {
            return Ok(());
        }

        if new_position > current_position {
            // Moving down: shift items up
            conn.execute(
                "UPDATE queue_items SET position = position - 1 WHERE position > ?1 AND position <= ?2 AND status IN ('pending', 'playing', 'paused')",
                params![current_position, new_position],
            )
            .map_err(|e| format!("Failed to shift items: {}", e))?;
        } else {
            // Moving up: shift items down
            conn.execute(
                "UPDATE queue_items SET position = position + 1 WHERE position >= ?1 AND position < ?2 AND status IN ('pending', 'playing', 'paused')",
                params![new_position, current_position],
            )
            .map_err(|e| format!("Failed to shift items: {}", e))?;
        }

        // Set the new position
        conn.execute(
            "UPDATE queue_items SET position = ?1 WHERE uuid = ?2",
            params![new_position, uuid],
        )
        .map_err(|e| format!("Failed to update position: {}", e))?;

        Ok(())
    })
}

/// Remove a queue item
#[tauri::command]
pub fn remove_queue_item(uuid: String) -> Result<(), String> {
    with_db(|conn| {
        conn.execute("DELETE FROM queue_items WHERE uuid = ?1", params![uuid])
            .map_err(|e| format!("Failed to remove item: {}", e))?;
        Ok(())
    })
}

/// Clear completed items older than specified days (or all completed if None)
#[tauri::command]
pub fn clear_completed_items(older_than_days: Option<i64>) -> Result<i64, String> {
    with_db(|conn| {
        let deleted = if let Some(days) = older_than_days {
            let cutoff = now_ms() - (days * 24 * 60 * 60 * 1000);
            conn.execute(
                "DELETE FROM queue_items WHERE status = 'completed' AND completed_at < ?1",
                params![cutoff],
            )
            .map_err(|e| format!("Failed to clear items: {}", e))?
        } else {
            conn.execute("DELETE FROM queue_items WHERE status = 'completed'", [])
                .map_err(|e| format!("Failed to clear items: {}", e))?
        };
        Ok(deleted as i64)
    })
}

/// Get pending items count
#[tauri::command]
pub fn get_pending_count() -> Result<i64, String> {
    with_db(|conn| {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM queue_items WHERE status IN ('pending', 'playing', 'paused')",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to count items: {}", e))?;
        Ok(count)
    })
}

/// Get the next pending item to play
#[tauri::command]
pub fn get_next_pending_item() -> Result<Option<QueueItem>, String> {
    with_db(|conn| {
        let result = conn.query_row(
            "SELECT id, uuid, session_id, text, status, position, created_at, started_at, completed_at, duration_ms, error_message
             FROM queue_items
             WHERE status = 'pending'
             ORDER BY position ASC
             LIMIT 1",
            [],
            |row| {
                Ok(QueueItem {
                    id: row.get(0)?,
                    uuid: row.get(1)?,
                    session_id: row.get(2)?,
                    text: row.get(3)?,
                    status: row.get(4)?,
                    position: row.get(5)?,
                    created_at: row.get(6)?,
                    started_at: row.get(7)?,
                    completed_at: row.get(8)?,
                    duration_ms: row.get(9)?,
                    error_message: row.get(10)?,
                })
            },
        );

        match result {
            Ok(item) => Ok(Some(item)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("Failed to get next item: {}", e)),
        }
    })
}
