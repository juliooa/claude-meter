use rusqlite::{params, Connection};
use std::path::Path;

pub fn open(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            metric_name TEXT NOT NULL,
            value REAL NOT NULL,
            session_id TEXT,
            model TEXT,
            metric_type TEXT,
            tool_name TEXT,
            decision TEXT,
            attributes_json TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_timestamp ON metrics(timestamp);
        CREATE INDEX IF NOT EXISTS idx_name ON metrics(metric_name);
        CREATE INDEX IF NOT EXISTS idx_session ON metrics(session_id);
        CREATE INDEX IF NOT EXISTS idx_model ON metrics(model);",
    )?;
    Ok(conn)
}

pub struct MetricRow {
    pub timestamp: i64,
    pub metric_name: String,
    pub value: f64,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub metric_type: Option<String>,
    pub tool_name: Option<String>,
    pub decision: Option<String>,
    pub attributes_json: Option<String>,
}

pub fn insert(conn: &Connection, row: &MetricRow) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO metrics (timestamp, metric_name, value, session_id, model, metric_type, tool_name, decision, attributes_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.timestamp,
            row.metric_name,
            row.value,
            row.session_id,
            row.model,
            row.metric_type,
            row.tool_name,
            row.decision,
            row.attributes_json,
        ],
    )?;
    Ok(())
}

pub struct UsageSummary {
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cache_creation_tokens: f64,
    pub cost: f64,
    pub sessions: i64,
    pub lines_added: f64,
    pub lines_removed: f64,
    pub active_time_secs: f64,
}

pub fn query_usage(conn: &Connection, since_ts: i64) -> anyhow::Result<UsageSummary> {
    let token_sum = |typ: &str| -> anyhow::Result<f64> {
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(value), 0) FROM metrics WHERE timestamp >= ?1 AND metric_name LIKE '%token%' AND metric_type = ?2",
            params![since_ts, typ],
            |row| row.get(0),
        )?)
    };

    let input_tokens = token_sum("input")?;
    let output_tokens = token_sum("output")?;
    let cache_read_tokens = token_sum("cacheRead")?;
    let cache_creation_tokens = token_sum("cacheCreation")?;

    let cost: f64 = conn.query_row(
        "SELECT COALESCE(SUM(value), 0) FROM metrics WHERE timestamp >= ?1 AND metric_name LIKE '%cost%'",
        params![since_ts],
        |row| row.get(0),
    )?;

    let sessions: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT session_id) FROM metrics WHERE timestamp >= ?1 AND session_id IS NOT NULL",
        params![since_ts],
        |row| row.get(0),
    )?;

    let lines_sum = |typ: &str| -> anyhow::Result<f64> {
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(value), 0) FROM metrics WHERE timestamp >= ?1 AND metric_name LIKE '%line%' AND metric_type = ?2",
            params![since_ts, typ],
            |row| row.get(0),
        )?)
    };

    let lines_added = lines_sum("added")?;
    let lines_removed = lines_sum("removed")?;

    let active_time_secs: f64 = conn.query_row(
        "SELECT COALESCE(SUM(value), 0) FROM metrics WHERE timestamp >= ?1 AND metric_name LIKE '%active%'",
        params![since_ts],
        |row| row.get(0),
    )?;

    Ok(UsageSummary {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        cost,
        sessions,
        lines_added,
        lines_removed,
        active_time_secs,
    })
}

pub struct DaySummary {
    pub date: String,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cache_read_tokens: f64,
    pub cost: f64,
    pub sessions: i64,
}

pub fn query_history(conn: &Connection, since_ts: i64) -> anyhow::Result<Vec<DaySummary>> {
    let mut stmt = conn.prepare(
        "SELECT date(timestamp, 'unixepoch', 'localtime') as day,
                SUM(CASE WHEN metric_name LIKE '%token%' AND metric_type = 'input' THEN value ELSE 0 END),
                SUM(CASE WHEN metric_name LIKE '%token%' AND metric_type = 'output' THEN value ELSE 0 END),
                SUM(CASE WHEN metric_name LIKE '%token%' AND metric_type = 'cacheRead' THEN value ELSE 0 END),
                SUM(CASE WHEN metric_name LIKE '%cost%' THEN value ELSE 0 END),
                COUNT(DISTINCT session_id)
         FROM metrics
         WHERE timestamp >= ?1
         GROUP BY day
         ORDER BY day DESC",
    )?;

    let rows = stmt
        .query_map(params![since_ts], |row| {
            Ok(DaySummary {
                date: row.get(0)?,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                cache_read_tokens: row.get(3)?,
                cost: row.get(4)?,
                sessions: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub struct ModelSummary {
    pub model: String,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub cost: f64,
}

pub fn query_by_model(conn: &Connection) -> anyhow::Result<Vec<ModelSummary>> {
    let mut stmt = conn.prepare(
        "SELECT model,
                SUM(CASE WHEN metric_name LIKE '%token%' AND metric_type = 'input' THEN value ELSE 0 END),
                SUM(CASE WHEN metric_name LIKE '%token%' AND metric_type = 'output' THEN value ELSE 0 END),
                SUM(CASE WHEN metric_name LIKE '%cost%' THEN value ELSE 0 END)
         FROM metrics
         WHERE model IS NOT NULL
         GROUP BY model
         ORDER BY SUM(CASE WHEN metric_name LIKE '%cost%' THEN value ELSE 0 END) DESC",
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ModelSummary {
                model: row.get(0)?,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                cost: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub struct SessionSummary {
    pub session_id: String,
    pub date: String,
    pub total_tokens: f64,
    pub cost: f64,
    pub duration_secs: i64,
}

pub fn query_by_session(conn: &Connection, since_ts: i64) -> anyhow::Result<Vec<SessionSummary>> {
    let mut stmt = conn.prepare(
        "SELECT session_id,
                MIN(date(timestamp, 'unixepoch', 'localtime')),
                SUM(CASE WHEN metric_name LIKE '%token%' THEN value ELSE 0 END),
                SUM(CASE WHEN metric_name LIKE '%cost%' THEN value ELSE 0 END),
                MAX(timestamp) - MIN(timestamp)
         FROM metrics
         WHERE timestamp >= ?1 AND session_id IS NOT NULL
         GROUP BY session_id
         ORDER BY MIN(timestamp) DESC",
    )?;

    let rows = stmt
        .query_map(params![since_ts], |row| {
            Ok(SessionSummary {
                session_id: row.get(0)?,
                date: row.get(1)?,
                total_tokens: row.get(2)?,
                cost: row.get(3)?,
                duration_secs: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn count_before(conn: &Connection, cutoff_ts: i64) -> anyhow::Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM metrics WHERE timestamp < ?1",
        params![cutoff_ts],
        |row| row.get(0),
    )?)
}

pub fn purge(conn: &Connection, cutoff_ts: i64) -> anyhow::Result<usize> {
    Ok(conn.execute(
        "DELETE FROM metrics WHERE timestamp < ?1",
        params![cutoff_ts],
    )?)
}
