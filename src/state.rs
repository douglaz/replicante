use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

pub struct StateManager {
    conn: Arc<Mutex<Connection>>,
}

impl StateManager {
    pub fn new(database_path: &str) -> Result<Self> {
        let conn = Connection::open(database_path)?;

        // Create tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                key TEXT UNIQUE NOT NULL,
                value TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thought TEXT NOT NULL,
                action TEXT NOT NULL,
                result TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS capabilities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tool_name TEXT NOT NULL,
                description TEXT,
                last_used TIMESTAMP,
                success_rate REAL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        info!("Database initialized at: {}", database_path);

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn remember(&self, key: &str, value: Value) -> Result<()> {
        let value_str = serde_json::to_string(&value)?;
        let conn = self.conn.lock().unwrap();

        // Insert or update
        conn.execute(
            "INSERT INTO memory (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET 
             value = excluded.value,
             updated_at = CURRENT_TIMESTAMP",
            params![key, value_str],
        )?;

        debug!("Remembered: {} = {}", key, value_str);
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn recall(&self, key: &str) -> Result<Option<Value>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare("SELECT value FROM memory WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;

        if let Some(row) = rows.next() {
            let value_str = row?;
            let value: Value = serde_json::from_str(&value_str)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub async fn get_memory(&self) -> Result<Value> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare("SELECT key, value FROM memory")?;
        let memory_iter = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut memory = serde_json::Map::new();
        for entry in memory_iter {
            let (key, value_str) = entry?;
            if let Ok(value) = serde_json::from_str::<Value>(&value_str) {
                memory.insert(key, value);
            }
        }

        Ok(Value::Object(memory))
    }

    pub async fn record_decision(
        &self,
        thought: &str,
        action: &str,
        result: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "INSERT INTO decisions (thought, action, result) VALUES (?1, ?2, ?3)",
            params![thought, action, result],
        )?;

        debug!("Recorded decision: {} -> {}", thought, action);
        Ok(())
    }

    pub async fn get_recent_decisions(&self, limit: usize) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT thought, action, result, created_at 
             FROM decisions 
             ORDER BY created_at DESC 
             LIMIT ?1",
        )?;

        let decisions = stmt.query_map(params![limit], |row| {
            let thought = row.get::<_, String>(0)?;
            let action = row.get::<_, String>(1)?;
            let result: Option<String> = row.get(2)?;
            let created_at = row.get::<_, String>(3)?;

            Ok(format!(
                "[{}] Thought: {} | Action: {} | Result: {}",
                created_at,
                thought,
                action,
                result.unwrap_or_else(|| "pending".to_string())
            ))
        })?;

        let mut results = Vec::new();
        for decision in decisions {
            results.push(decision?);
        }

        Ok(results)
    }

    #[allow(dead_code)]
    pub async fn record_capability(
        &self,
        tool_name: &str,
        description: Option<&str>,
        success: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Check if capability exists
        let mut stmt =
            conn.prepare("SELECT id, success_rate FROM capabilities WHERE tool_name = ?1")?;
        let existing = stmt
            .query_row(params![tool_name], |row| {
                Ok((row.get::<_, i32>(0)?, row.get::<_, Option<f64>>(1)?))
            })
            .optional()?;

        if let Some((id, current_rate)) = existing {
            // Update existing capability
            let new_rate = if let Some(rate) = current_rate {
                // Simple moving average
                (rate * 0.9) + (if success { 0.1 } else { 0.0 })
            } else if success {
                1.0
            } else {
                0.0
            };

            conn.execute(
                "UPDATE capabilities SET 
                 description = COALESCE(?1, description),
                 last_used = CURRENT_TIMESTAMP,
                 success_rate = ?2
                 WHERE id = ?3",
                params![description, new_rate, id],
            )?;
        } else {
            // Insert new capability
            conn.execute(
                "INSERT INTO capabilities (tool_name, description, last_used, success_rate) 
                 VALUES (?1, ?2, CURRENT_TIMESTAMP, ?3)",
                params![tool_name, description, if success { 1.0 } else { 0.0 }],
            )?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_capabilities(&self) -> Result<Vec<(String, Option<String>, Option<f64>)>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT tool_name, description, success_rate 
             FROM capabilities 
             ORDER BY last_used DESC",
        )?;

        let capabilities = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })?;

        let mut results = Vec::new();
        for cap in capabilities {
            results.push(cap?);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_state_manager() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file.path().to_str().unwrap();

        let state = StateManager::new(db_path)?;

        // Test remember and recall
        state
            .remember("test_key", serde_json::json!("test_value"))
            .await?;
        let value = state.recall("test_key").await?;
        assert_eq!(value, Some(serde_json::json!("test_value")));

        // Test decision recording
        state
            .record_decision("test thought", "test action", Some("test result"))
            .await?;
        let decisions = state.get_recent_decisions(1).await?;
        assert_eq!(decisions.len(), 1);

        Ok(())
    }
}
