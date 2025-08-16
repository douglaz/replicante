use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_rusqlite::Connection;
use tracing::{debug, info};

pub struct StateManager {
    conn: Arc<Connection>,
}

impl StateManager {
    pub async fn new(database_path: &str) -> Result<Self> {
        let conn = Connection::open(database_path)
            .await
            .context("Failed to open database connection")?;

        // Create tables
        conn.call(|conn| {
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

            // New table for action patterns
            conn.execute(
                "CREATE TABLE IF NOT EXISTS action_patterns (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    pattern_type TEXT NOT NULL,
                    context TEXT NOT NULL,
                    action TEXT NOT NULL,
                    outcome TEXT,
                    success BOOLEAN,
                    confidence REAL,
                    occurrence_count INTEGER DEFAULT 1,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )?;

            // New table for learning metrics
            conn.execute(
                "CREATE TABLE IF NOT EXISTS learning_metrics (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    metric_name TEXT UNIQUE NOT NULL,
                    metric_value REAL NOT NULL,
                    sample_count INTEGER DEFAULT 1,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )?;

            Ok(())
        })
        .await
        .context("Failed to create database tables")?;

        info!("Database initialized at: {database_path}");

        Ok(Self {
            conn: Arc::new(conn),
        })
    }

    pub async fn remember(&self, key: &str, value: Value) -> Result<()> {
        let value_str = serde_json::to_string(&value)?;
        let key_clone = key.to_string();

        self.conn
            .call(move |conn| {
                // Insert or update
                conn.execute(
                    "INSERT INTO memory (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET 
                     value = excluded.value,
                     updated_at = CURRENT_TIMESTAMP",
                    params![key_clone, value_str],
                )?;
                Ok(())
            })
            .await
            .context("Failed to remember value")?;

        debug!("Remembered: {key} = {value}");
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn recall(&self, key: &str) -> Result<Option<Value>> {
        let key_clone = key.to_string();

        let value_str_opt = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare("SELECT value FROM memory WHERE key = ?1")?;
                let mut rows = stmt.query_map(params![key_clone], |row| row.get::<_, String>(0))?;

                if let Some(row) = rows.next() {
                    let value_str = row?;
                    Ok(Some(value_str))
                } else {
                    Ok(None)
                }
            })
            .await
            .context("Failed to recall value")?;

        // Parse JSON outside the closure to avoid error type issues
        match value_str_opt {
            Some(value_str) => {
                let value: Value = serde_json::from_str(&value_str)
                    .context("Failed to parse stored JSON value")?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    pub async fn get_memory(&self) -> Result<Value> {
        let memory = self
            .conn
            .call(|conn| {
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
            })
            .await
            .context("Failed to get memory")?;

        Ok(memory)
    }

    pub async fn record_decision(
        &self,
        thought: &str,
        action: &str,
        result: Option<&str>,
    ) -> Result<()> {
        let thought_clone = thought.to_string();
        let action_clone = action.to_string();
        let result_clone = result.map(|s| s.to_string());

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO decisions (thought, action, result) VALUES (?1, ?2, ?3)",
                    params![thought_clone, action_clone, result_clone],
                )?;
                Ok(())
            })
            .await
            .context("Failed to record decision")?;

        debug!("Recorded decision: {thought} -> {action}");
        Ok(())
    }

    pub async fn get_recent_decisions(&self, limit: usize) -> Result<Vec<String>> {
        let decisions = self
            .conn
            .call(move |conn| {
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
                        "[{created_at}] Thought: {thought} | Action: {action} | Result: {result}",
                        result = result.unwrap_or_else(|| "pending".to_string())
                    ))
                })?;

                let mut results = Vec::new();
                for decision in decisions {
                    results.push(decision?);
                }

                Ok(results)
            })
            .await
            .context("Failed to get recent decisions")?;

        Ok(decisions)
    }

    #[allow(dead_code)]
    pub async fn record_capability(
        &self,
        tool_name: &str,
        description: Option<&str>,
        success: bool,
    ) -> Result<()> {
        let tool_name_clone = tool_name.to_string();
        let description_clone = description.map(|s| s.to_string());

        self.conn
            .call(move |conn| {
                // Check if capability exists
                let mut stmt =
                    conn.prepare("SELECT id, success_rate FROM capabilities WHERE tool_name = ?1")?;
                let existing = stmt
                    .query_row(params![tool_name_clone], |row| {
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
                        params![description_clone, new_rate, id],
                    )?;
                } else {
                    // Insert new capability
                    conn.execute(
                        "INSERT INTO capabilities (tool_name, description, last_used, success_rate) 
                         VALUES (?1, ?2, CURRENT_TIMESTAMP, ?3)",
                        params![
                            tool_name_clone,
                            description_clone,
                            if success { 1.0 } else { 0.0 }
                        ],
                    )?;
                }

                Ok(())
            })
            .await
            .context("Failed to record capability")?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_capabilities(&self) -> Result<Vec<(String, Option<String>, Option<f64>)>> {
        let capabilities = self
            .conn
            .call(|conn| {
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
            })
            .await
            .context("Failed to get capabilities")?;

        Ok(capabilities)
    }

    // Learning and pattern recognition methods

    pub async fn record_action_pattern(
        &self,
        pattern_type: &str,
        context: &str,
        action: &str,
        outcome: Option<&str>,
        success: bool,
    ) -> Result<()> {
        let pattern_type_clone = pattern_type.to_string();
        let context_clone = context.to_string();
        let action_clone = action.to_string();
        let outcome_clone = outcome.map(|s| s.to_string());

        self.conn
            .call(move |conn| {
                // Check if similar pattern exists
                let mut stmt = conn.prepare(
                    "SELECT id, occurrence_count, confidence 
                     FROM action_patterns 
                     WHERE pattern_type = ?1 AND context = ?2 AND action = ?3",
                )?;

                let existing = stmt
                    .query_row(
                        params![pattern_type_clone, context_clone, action_clone],
                        |row| {
                            Ok((
                                row.get::<_, i32>(0)?,
                                row.get::<_, i32>(1)?,
                                row.get::<_, f64>(2)?,
                            ))
                        },
                    )
                    .optional()?;

                if let Some((id, count, confidence)) = existing {
                    // Update existing pattern with exponential moving average
                    let new_confidence = confidence * 0.9 + if success { 0.1 } else { 0.0 };
                    let new_count = count + 1;

                    conn.execute(
                        "UPDATE action_patterns SET 
                         outcome = COALESCE(?1, outcome),
                         success = ?2,
                         confidence = ?3,
                         occurrence_count = ?4,
                         updated_at = CURRENT_TIMESTAMP
                         WHERE id = ?5",
                        params![outcome_clone, success, new_confidence, new_count, id],
                    )?;
                } else {
                    // Insert new pattern
                    conn.execute(
                        "INSERT INTO action_patterns 
                         (pattern_type, context, action, outcome, success, confidence) 
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            pattern_type_clone,
                            context_clone,
                            action_clone,
                            outcome_clone,
                            success,
                            if success { 1.0 } else { 0.0 }
                        ],
                    )?;
                }

                Ok(())
            })
            .await
            .context("Failed to record action pattern")?;

        Ok(())
    }

    pub async fn get_best_action_for_context(
        &self,
        pattern_type: &str,
        context: &str,
        min_confidence: f64,
    ) -> Result<Option<(String, f64)>> {
        let pattern_type_clone = pattern_type.to_string();
        let context_clone = context.to_string();

        let result = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT action, confidence 
                     FROM action_patterns 
                     WHERE pattern_type = ?1 AND context = ?2 AND confidence >= ?3 
                     ORDER BY confidence DESC, occurrence_count DESC 
                     LIMIT 1",
                )?;

                let result = stmt
                    .query_row(
                        params![pattern_type_clone, context_clone, min_confidence],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
                    )
                    .optional()?;
                Ok(result)
            })
            .await
            .context("Failed to get best action for context")?;

        Ok(result)
    }

    pub async fn update_learning_metric(&self, metric_name: &str, value: f64) -> Result<()> {
        let metric_name_clone = metric_name.to_string();

        self.conn
            .call(move |conn| {
                // Check if metric exists
                let mut stmt = conn.prepare(
                    "SELECT metric_value, sample_count FROM learning_metrics WHERE metric_name = ?1"
                )?;

                let existing = stmt
                    .query_row(params![metric_name_clone], |row| {
                        Ok((row.get::<_, f64>(0)?, row.get::<_, i32>(1)?))
                    })
                    .optional()?;

                if let Some((current_value, count)) = existing {
                    // Update with running average
                    let new_value = (current_value * count as f64 + value) / (count + 1) as f64;
                    let new_count = count + 1;

                    conn.execute(
                        "UPDATE learning_metrics SET 
                         metric_value = ?1,
                         sample_count = ?2,
                         updated_at = CURRENT_TIMESTAMP
                         WHERE metric_name = ?3",
                        params![new_value, new_count, metric_name_clone],
                    )?;
                } else {
                    // Insert new metric
                    conn.execute(
                        "INSERT INTO learning_metrics (metric_name, metric_value) 
                         VALUES (?1, ?2)",
                        params![metric_name_clone, value],
                    )?;
                }

                Ok(())
            })
            .await
            .context("Failed to update learning metric")?;

        Ok(())
    }

    pub async fn get_learning_metrics(&self) -> Result<HashMap<String, f64>> {
        let metrics = self
            .conn
            .call(|conn| {
                let mut stmt =
                    conn.prepare("SELECT metric_name, metric_value FROM learning_metrics")?;

                let metrics_iter = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })?;

                let mut metrics = HashMap::new();
                for metric in metrics_iter {
                    let (name, value) = metric?;
                    metrics.insert(name, value);
                }

                Ok(metrics)
            })
            .await
            .context("Failed to get learning metrics")?;

        Ok(metrics)
    }

    pub async fn analyze_decision_patterns(&self, lookback_hours: i64) -> Result<Value> {
        let analysis = self
            .conn
            .call(move |conn| {
                // Analyze success patterns
                let mut success_stmt = conn.prepare(
                    "SELECT pattern_type, COUNT(*) as count, AVG(confidence) as avg_confidence 
                     FROM action_patterns 
                     WHERE success = 1 AND 
                           datetime(updated_at) >= datetime('now', '-' || ?1 || ' hours')
                     GROUP BY pattern_type",
                )?;

                let success_patterns = success_stmt.query_map(params![lookback_hours], |row| {
                    Ok(serde_json::json!({
                        "type": row.get::<_, String>(0)?,
                        "count": row.get::<_, i32>(1)?,
                        "avg_confidence": row.get::<_, f64>(2)?
                    }))
                })?;

                let mut patterns = Vec::new();
                for pattern in success_patterns {
                    patterns.push(pattern?);
                }

                // Get tool usage statistics
                let mut tool_stmt = conn.prepare(
                    "SELECT tool_name, success_rate, 
                            COUNT(*) OVER() as total_tools
                     FROM capabilities 
                     WHERE last_used >= datetime('now', '-' || ?1 || ' hours')
                     ORDER BY success_rate DESC",
                )?;

                let tool_stats = tool_stmt.query_map(params![lookback_hours], |row| {
                    Ok(serde_json::json!({
                        "tool": row.get::<_, String>(0)?,
                        "success_rate": row.get::<_, Option<f64>>(1)?
                    }))
                })?;

                let mut tools = Vec::new();
                for tool in tool_stats {
                    tools.push(tool?);
                }

                Ok(serde_json::json!({
                    "successful_patterns": patterns,
                    "tool_performance": tools,
                    "analysis_period_hours": lookback_hours
                }))
            })
            .await
            .context("Failed to analyze decision patterns")?;

        Ok(analysis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_state_manager() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

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
