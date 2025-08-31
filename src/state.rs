use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_rusqlite::Connection;
use tracing::{debug, info};

use crate::{DecisionRecord, DecisionResult};

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

    /// Get a summarized version of memory for LLM context
    /// Limits to recent and relevant entries to avoid context explosion
    /// Clean up old memory entries to prevent unbounded growth
    pub async fn cleanup_old_memory(&self, keep_days: i64) -> Result<usize> {
        let deleted = self
            .conn
            .call(move |conn| {
                // Delete old tool results and errors
                let mut result = conn.execute(
                    "DELETE FROM memory 
                     WHERE (key LIKE 'tool_result_%' OR key LIKE 'error_%')
                       AND datetime(updated_at) < datetime('now', ?1)",
                    params![format!("-{keep_days} days")],
                )?;

                // Also delete discovered_tools as it's redundant
                result += conn.execute("DELETE FROM memory WHERE key = 'discovered_tools'", [])?;

                Ok(result)
            })
            .await
            .context("Failed to cleanup old memory")?;

        info!("Cleaned up {deleted} old memory entries");
        Ok(deleted)
    }

    pub async fn get_memory_summary(&self, max_entries: usize, max_size: usize) -> Result<Value> {
        let memory = self
            .conn
            .call(move |conn| {
                let mut memory = serde_json::Map::new();
                let mut total_size = 0;
                let mut entries_count = 0;

                // First, get the last 3 tool results (most recent)
                let mut tool_stmt = conn.prepare(
                    "SELECT key, value, LENGTH(value) as size 
                     FROM memory 
                     WHERE key LIKE 'tool_result_%'
                     ORDER BY key DESC  -- Keys contain timestamp, so DESC gives most recent
                     LIMIT 3",
                )?;

                let tool_results = tool_stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;

                // Add tool results to memory
                for entry in tool_results {
                    if entries_count >= max_entries || total_size >= max_size {
                        break;
                    }

                    let (key, value_str, size) = entry?;

                    if let Ok(mut value) = serde_json::from_str::<Value>(&value_str) {
                        // Truncate large values in tool results
                        if let Some(content) =
                            value.get("truncated_content").or(value.get("content"))
                            && let Some(s) = content.as_str()
                            && s.len() > 1000
                        {
                            value["truncated_content"] =
                                Value::String(format!("{}... [truncated]", &s[..1000]));
                        }
                        memory.insert(key, value);
                        total_size += size.min(1000) as usize; // Count truncated size
                        entries_count += 1;
                    }
                }

                // Then get other important memory entries
                let mut stmt = conn.prepare(
                    "SELECT key, value, LENGTH(value) as size 
                     FROM memory 
                     WHERE key NOT LIKE 'tool_result_%' 
                       AND key NOT LIKE 'error_%'        -- Exclude detailed errors
                       AND key != 'discovered_tools'     -- Exclude redundant tool list
                     ORDER BY 
                       CASE 
                         WHEN key IN ('agent_id', 'initial_goals', 'current_task') THEN 0
                         WHEN key LIKE 'fedimint_%' THEN 1  -- Prioritize task-specific
                         ELSE 2 
                       END,
                       updated_at DESC 
                     LIMIT ?1",
                )?;

                let memory_iter = stmt.query_map(params![max_entries - entries_count], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })?;

                // Add other memory entries
                for entry in memory_iter {
                    if entries_count >= max_entries || total_size >= max_size {
                        break;
                    }

                    let (key, value_str, size) = entry?;

                    // Stop if we exceed size limit
                    if total_size + size as usize > max_size {
                        break;
                    }

                    if let Ok(mut value) = serde_json::from_str::<Value>(&value_str) {
                        // Truncate large string values
                        if let Some(s) = value.as_str()
                            && s.len() > 1000
                        {
                            value = Value::String(format!("{}... [truncated]", &s[..1000]));
                        }
                        memory.insert(key, value);
                        total_size += size as usize;
                        entries_count += 1;
                    }
                }

                // Add memory statistics
                let mut stats_stmt = conn.prepare(
                    "SELECT COUNT(*) as total, SUM(LENGTH(value)) as total_size FROM memory",
                )?;

                let stats = stats_stmt
                    .query_row([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;

                memory.insert(
                    "_memory_stats".to_string(),
                    serde_json::json!({
                        "total_entries": stats.0,
                        "total_size": stats.1,
                        "shown_entries": memory.len(),
                        "shown_size": total_size
                    }),
                );

                Ok(Value::Object(memory))
            })
            .await
            .context("Failed to get memory summary")?;

        Ok(memory)
    }

    pub async fn record_decision(
        &self,
        thought: &str,
        action: &str,
        result: Option<&str>,
    ) -> Result<i64> {
        let thought_clone = thought.to_string();
        let action_clone = action.to_string();
        let result_clone = result.map(|s| s.to_string());

        let id = self
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO decisions (thought, action, result) VALUES (?1, ?2, ?3)",
                    params![thought_clone, action_clone, result_clone],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .context("Failed to record decision")?;

        debug!("Recorded decision #{id}: {thought} -> {action}");
        Ok(id)
    }

    pub async fn update_decision_result(
        &self,
        decision_id: i64,
        result: &DecisionResult,
    ) -> Result<()> {
        let result_json = serde_json::to_string(result)?;

        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE decisions SET result = ?1 WHERE id = ?2",
                    params![result_json, decision_id],
                )?;
                Ok(())
            })
            .await
            .context("Failed to update decision result")?;

        debug!("Updated decision #{decision_id} with result: {result:?}");
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

    pub async fn get_recent_decisions_structured(
        &self,
        limit: usize,
    ) -> Result<Vec<DecisionRecord>> {
        let decisions = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, thought, action, result, created_at 
                     FROM decisions 
                     ORDER BY created_at DESC 
                     LIMIT ?1",
                )?;

                let decisions = stmt.query_map(params![limit], |row| {
                    let id = row.get::<_, i64>(0)?;
                    let thought = row.get::<_, String>(1)?;
                    let action_str = row.get::<_, String>(2)?;
                    let result_str: Option<String> = row.get(3)?;
                    let created_at_str = row.get::<_, String>(4)?;

                    // Parse action and parameters from the stored string
                    // Format is "action: <action>, params: <params>" or just plain action
                    let (action, parameters) = if let Some(idx) = action_str.find(", params:") {
                        // Has params format: "action: <action>, params: <params>"
                        let action_part = if action_str.starts_with("action: ") {
                            action_str[7..idx].trim().to_string() // Skip "action: " and trim
                        } else {
                            action_str[..idx].trim().to_string()
                        };
                        let params_part = &action_str[idx + 9..]; // Skip ", params: "
                        let params =
                            if params_part.trim() != "None" && params_part.trim() != "Some(Null)" {
                                serde_json::from_str(params_part.trim()).ok()
                            } else {
                                None
                            };
                        (action_part, params)
                    } else if action_str.starts_with("action: ") {
                        // Simple format with "action: " prefix
                        (action_str[7..].trim().to_string(), None)
                    } else {
                        // Plain action string
                        (action_str.trim().to_string(), None)
                    };

                    // Parse result if it's JSON
                    let result =
                        result_str.and_then(|s| serde_json::from_str::<DecisionResult>(&s).ok());

                    // Parse timestamp
                    let timestamp = if let Ok(dt) = DateTime::parse_from_rfc3339(&created_at_str) {
                        dt.with_timezone(&Utc)
                    } else {
                        // Fallback for SQLite's default format
                        let naive = chrono::NaiveDateTime::parse_from_str(
                            &created_at_str,
                            "%Y-%m-%d %H:%M:%S",
                        )
                        .unwrap_or_else(|_| chrono::Local::now().naive_utc());
                        DateTime::from_naive_utc_and_offset(naive, Utc)
                    };

                    Ok(DecisionRecord {
                        id,
                        timestamp,
                        thought,
                        action,
                        parameters,
                        result,
                    })
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
    use chrono::Utc;
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
        let decision_id = state
            .record_decision("test thought", "test action", Some("test result"))
            .await?;
        assert!(decision_id > 0);
        let decisions = state.get_recent_decisions(1).await?;
        assert_eq!(decisions.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn test_decision_recording_with_id() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Record a decision and get its ID
        let decision_id = state
            .record_decision(
                "test thought",
                "action: use_tool:test, params: Some(Object {\"key\": \"value\"})",
                None,
            )
            .await?;

        assert!(decision_id > 0, "Decision ID should be positive");

        // Verify we can update the decision result
        let result = DecisionResult {
            status: "success".to_string(),
            summary: Some("Test completed successfully".to_string()),
            error: None,
            duration_ms: Some(123),
        };

        state.update_decision_result(decision_id, &result).await?;

        // Verify the result was updated
        let decisions = state.get_recent_decisions_structured(1).await?;
        assert_eq!(decisions.len(), 1);

        let decision = &decisions[0];
        assert_eq!(decision.id, decision_id);
        assert_eq!(decision.thought, "test thought");
        assert!(decision.result.is_some());

        let stored_result = decision.result.as_ref().unwrap();
        assert_eq!(stored_result.status, "success");
        assert_eq!(
            stored_result.summary,
            Some("Test completed successfully".to_string())
        );
        assert_eq!(stored_result.duration_ms, Some(123));

        Ok(())
    }

    #[tokio::test]
    async fn test_structured_decision_parsing() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Test parsing of action with parameters
        let _id1 = state
            .record_decision(
                "thought 1",
                r#"action: use_tool:http:http_get, params: {"url": "https://example.com"}"#,
                None,
            )
            .await?;

        // Test parsing of action without parameters
        let _id2 = state
            .record_decision("thought 2", "action: wait, params: None", None)
            .await?;

        // Test simple action format (without the "action: " prefix)
        let _id3 = state
            .record_decision("thought 3", "action: simple_action", None)
            .await?;

        let decisions = state.get_recent_decisions_structured(3).await?;
        assert_eq!(decisions.len(), 3);

        // Decisions are returned in DESC order (most recent first)
        // So the actual order is: simple_action (id3), wait (id2), use_tool (id1)

        // Check first decision (oldest, with parameters) - id1
        assert_eq!(decisions[0].action, "use_tool:http:http_get");
        assert!(decisions[0].parameters.is_some());
        assert_eq!(
            decisions[0].parameters.as_ref().unwrap()["url"],
            "https://example.com"
        );

        // Check second decision (no parameters) - id2
        assert_eq!(decisions[1].action, "wait");
        assert!(decisions[1].parameters.is_none());

        // Check third decision (most recent, simple format) - id3
        assert_eq!(decisions[2].action, "simple_action");
        assert!(decisions[2].parameters.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn test_tool_result_visibility() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Store multiple tool results
        for i in 1..=5 {
            let key = format!(
                "tool_result_{}",
                Utc::now().timestamp_nanos_opt().unwrap_or(i)
            );
            let value = serde_json::json!({
                "tool": format!("test_tool_{i}"),
                "success": true,
                "content": format!("This is result {i} with some content"),
                "timestamp": Utc::now().to_rfc3339(),
            });
            state.remember(&key, value).await?;
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Store some other memory entries
        state
            .remember("agent_id", serde_json::json!("test-agent"))
            .await?;
        state
            .remember("initial_goals", serde_json::json!("test goals"))
            .await?;

        // Get memory summary
        let summary = state.get_memory_summary(20, 10000).await?;

        let summary_obj = summary
            .as_object()
            .context("Memory summary should be an object")?;

        // Should include the 3 most recent tool results
        let tool_result_count = summary_obj
            .keys()
            .filter(|k| k.starts_with("tool_result_"))
            .count();

        assert_eq!(
            tool_result_count, 3,
            "Should show exactly 3 most recent tool results"
        );

        // Should also include other important memory
        assert!(summary_obj.contains_key("agent_id"));
        assert!(summary_obj.contains_key("initial_goals"));
        assert!(summary_obj.contains_key("_memory_stats"));

        Ok(())
    }

    #[tokio::test]
    async fn test_count_based_filtering() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Create tool results with timestamps in keys for ordering
        let base_time = 1700000000;
        for i in 0..10 {
            let key = format!("tool_result_{timestamp}", timestamp = base_time + i);
            let value = serde_json::json!({
                "tool": format!("tool_{i}"),
                "success": true,
                "content": format!("Result {i}"),
                "timestamp": format!("2024-01-0{i}T12:00:00Z"),
            });
            state.remember(&key, value).await?;
        }

        // Get memory summary with limited entries
        let summary = state.get_memory_summary(10, 50000).await?;

        let summary_obj = summary
            .as_object()
            .context("Memory summary should be an object")?;

        // Count tool results in summary
        let tool_results: Vec<_> = summary_obj
            .keys()
            .filter(|k| k.starts_with("tool_result_"))
            .collect();

        // Should have exactly 3 tool results (count-based limit)
        assert_eq!(
            tool_results.len(),
            3,
            "Should return exactly 3 tool results"
        );

        // Verify they are the most recent ones (highest timestamps)
        for key in &tool_results {
            let num: i32 = key.trim_start_matches("tool_result_").parse().unwrap();
            assert!(
                num >= base_time + 7,
                "Should include only the most recent results"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_memory_truncation() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Store a tool result with very large content
        let large_content = "x".repeat(2000);
        let key = format!(
            "tool_result_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(1)
        );
        let value = serde_json::json!({
            "tool": "test_tool",
            "success": true,
            "content": large_content.clone(),
            "truncated_content": large_content.clone(),
        });
        state.remember(&key, value).await?;

        // Get memory summary
        let summary = state.get_memory_summary(10, 50000).await?;

        let summary_obj = summary
            .as_object()
            .context("Memory summary should be an object")?;

        // Find the tool result
        let tool_result = summary_obj
            .get(&key)
            .context("Tool result should be in summary")?;

        // Check that content is truncated
        if let Some(truncated) = tool_result.get("truncated_content") {
            let content_str = truncated.as_str().unwrap();
            assert!(
                content_str.ends_with("... [truncated]"),
                "Large content should be truncated"
            );
            assert!(
                content_str.len() < 1100,
                "Truncated content should be around 1000 chars"
            );
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_decision_result_update() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Record a decision
        let decision_id = state
            .record_decision("test thought", "test action", None)
            .await?;

        // Update with success result
        let success_result = DecisionResult {
            status: "success".to_string(),
            summary: Some("Operation completed".to_string()),
            error: None,
            duration_ms: Some(500),
        };
        state
            .update_decision_result(decision_id, &success_result)
            .await?;

        // Verify update
        let decisions = state.get_recent_decisions_structured(1).await?;
        let decision = &decisions[0];
        assert_eq!(decision.result.as_ref().unwrap().status, "success");
        assert_eq!(decision.result.as_ref().unwrap().duration_ms, Some(500));

        // Update with error result
        let error_result = DecisionResult {
            status: "error".to_string(),
            summary: None,
            error: Some("Connection timeout".to_string()),
            duration_ms: Some(30000),
        };
        state
            .update_decision_result(decision_id, &error_result)
            .await?;

        // Verify error update
        let decisions = state.get_recent_decisions_structured(1).await?;
        let decision = &decisions[0];
        assert_eq!(decision.result.as_ref().unwrap().status, "error");
        assert_eq!(
            decision.result.as_ref().unwrap().error,
            Some("Connection timeout".to_string())
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_memory_stats_accuracy() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file
            .path()
            .to_str()
            .context("Failed to get temp file path")?;

        let state = StateManager::new(db_path).await?;

        // Add various memory entries
        state.remember("key1", serde_json::json!("value1")).await?;
        state.remember("key2", serde_json::json!("value2")).await?;
        state
            .remember("tool_result_1", serde_json::json!({"data": "result1"}))
            .await?;
        state
            .remember("tool_result_2", serde_json::json!({"data": "result2"}))
            .await?;

        // Get full memory to check total count
        let full_memory = state.get_memory().await?;
        let full_obj = full_memory.as_object().unwrap();
        assert_eq!(full_obj.len(), 4, "Should have 4 total entries");

        // Get summary with limits
        let summary = state.get_memory_summary(10, 50000).await?;
        let summary_obj = summary.as_object().unwrap();

        // Check memory stats
        let stats = summary_obj
            .get("_memory_stats")
            .context("Should have memory stats")?
            .as_object()
            .context("Stats should be an object")?;

        assert_eq!(stats.get("total_entries").unwrap().as_i64().unwrap(), 4);
        // shown_entries includes _memory_stats itself
        let shown = stats.get("shown_entries").unwrap().as_i64().unwrap();
        assert!(
            shown > 0 && shown <= 5,
            "Should show some but not too many entries"
        );

        Ok(())
    }
}
