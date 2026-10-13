use crate::usage::{SessionInfo, Status, UsageRecord, UsageSummary};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Clone for Store {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
        }
    }
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS usage_records (
                id TEXT PRIMARY KEY,
                ts INTEGER NOT NULL,
                provider_id TEXT NOT NULL,
                provider_name TEXT NOT NULL,
                protocol TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                cache_write_tokens INTEGER NOT NULL,
                ttft_ms INTEGER,
                duration_ms INTEGER NOT NULL,
                tps REAL,
                cost_usd REAL,
                session_id TEXT,
                estimated INTEGER NOT NULL,
                status TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_records(ts);
            CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model);
            CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_records(session_id);

            CREATE TABLE IF NOT EXISTS pricing_override (
                model TEXT PRIMARY KEY,
                input_per_m REAL NOT NULL,
                output_per_m REAL NOT NULL,
                cache_read_per_m REAL NOT NULL DEFAULT 0,
                cache_write_per_m REAL NOT NULL DEFAULT 0
            );
            "#,
        )?;
        // 老数据的 protocol/status 是 JSON 形式 ("ok"), 会让 summary 的 WHERE status='ok' 匹配不到; 就地洗一次
        conn.execute(
            "UPDATE usage_records SET
                status = REPLACE(status, '\"', ''),
                protocol = REPLACE(protocol, '\"', '')
             WHERE status LIKE '\"%' OR protocol LIKE '\"%'",
            [],
        )?;
        Ok(())
    }

    pub fn insert_usage(&self, r: &UsageRecord) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO usage_records (
                id, ts, provider_id, provider_name, protocol, model,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                ttft_ms, duration_ms, tps, cost_usd, session_id, estimated, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                r.id,
                r.ts.timestamp_millis(),
                r.provider_id,
                r.provider_name,
                r.protocol.as_str(),
                r.model,
                r.input_tokens as i64,
                r.output_tokens as i64,
                r.cache_read_tokens as i64,
                r.cache_write_tokens as i64,
                r.ttft_ms.map(|v| v as i64),
                r.duration_ms as i64,
                r.tps,
                r.cost_usd,
                r.session_id,
                r.estimated as i64,
                r.status.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn recent_usage(&self, limit: usize) -> anyhow::Result<Vec<UsageRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, ts, provider_id, provider_name, protocol, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    ttft_ms, duration_ms, tps, cost_usd, session_id, estimated, status
             FROM usage_records ORDER BY ts DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(UsageRecord {
                id: row.get(0)?,
                ts: DateTime::from_timestamp_millis(row.get(1)?).unwrap_or_else(Utc::now),
                provider_id: row.get(2)?,
                provider_name: row.get(3)?,
                protocol: crate::config::Protocol::from_db(&row.get::<_, String>(4)?),
                model: row.get(5)?,
                input_tokens: row.get::<_, i64>(6)? as u64,
                output_tokens: row.get::<_, i64>(7)? as u64,
                cache_read_tokens: row.get::<_, i64>(8)? as u64,
                cache_write_tokens: row.get::<_, i64>(9)? as u64,
                ttft_ms: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                duration_ms: row.get::<_, i64>(11)? as u64,
                tps: row.get(12)?,
                cost_usd: row.get(13)?,
                session_id: row.get(14)?,
                estimated: row.get::<_, i64>(15)? != 0,
                status: Status::from_db(&row.get::<_, String>(16)?),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn summary_since(&self, since_ms: i64) -> anyhow::Result<UsageSummary> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                    COALESCE(SUM(cost_usd),0), COUNT(*), AVG(tps)
             FROM usage_records WHERE ts >= ?1 AND status = 'ok'",
        )?;
        let mut out = UsageSummary::default();
        stmt.query_row(params![since_ms], |row| {
            out.input_tokens = row.get::<_, i64>(0)? as u64;
            out.output_tokens = row.get::<_, i64>(1)? as u64;
            out.cache_read_tokens = row.get::<_, i64>(2)? as u64;
            out.cache_write_tokens = row.get::<_, i64>(3)? as u64;
            out.cost_usd = row.get::<_, f64>(4)?;
            out.request_count = row.get::<_, i64>(5)? as u64;
            out.avg_tps = row.get(6)?;
            Ok(())
        })?;
        Ok(out)
    }

    pub fn sessions(&self, limit: usize) -> anyhow::Result<Vec<SessionInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, MIN(ts) as started, MAX(ts) as last,
                    COUNT(*), SUM(input_tokens), SUM(output_tokens), SUM(cost_usd)
             FROM usage_records WHERE session_id IS NOT NULL
             GROUP BY session_id ORDER BY last DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(SessionInfo {
                session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                started_at: DateTime::from_timestamp_millis(row.get(1)?).unwrap_or_else(Utc::now),
                last_activity: DateTime::from_timestamp_millis(row.get(2)?)
                    .unwrap_or_else(Utc::now),
                request_count: row.get::<_, i64>(3)? as u64,
                total_input: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                total_output: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                total_cost: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn export_csv(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let recs = self.recent_usage(100_000)?;
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record([
            "id",
            "ts",
            "provider",
            "protocol",
            "model",
            "input",
            "output",
            "cache_read",
            "cache_write",
            "ttft_ms",
            "duration_ms",
            "tps",
            "cost_usd",
            "session_id",
            "estimated",
            "status",
        ])?;
        for r in recs {
            wtr.write_record([
                r.id,
                r.ts.to_rfc3339(),
                r.provider_name,
                r.protocol.as_str().to_string(),
                r.model,
                r.input_tokens.to_string(),
                r.output_tokens.to_string(),
                r.cache_read_tokens.to_string(),
                r.cache_write_tokens.to_string(),
                r.ttft_ms.map(|v| v.to_string()).unwrap_or_default(),
                r.duration_ms.to_string(),
                r.tps.map(|v| format!("{:.2}", v)).unwrap_or_default(),
                r.cost_usd.map(|v| format!("{:.6}", v)).unwrap_or_default(),
                r.session_id.unwrap_or_default(),
                r.estimated.to_string(),
                r.status.as_str().to_string(),
            ])?;
        }
        wtr.flush()?;
        Ok(())
    }
}
