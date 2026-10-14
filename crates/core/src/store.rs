use crate::config::AgentId;
use crate::usage::{DayUsage, SessionInfo, Status, UsageRecord, UsageSummary};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::io::{BufRead, BufReader};
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
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", -8000)?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
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
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                ttft_ms INTEGER,
                duration_ms INTEGER NOT NULL,
                tps REAL,
                cost_usd REAL,
                session_id TEXT,
                agent_id TEXT,
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
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN agent_id TEXT", []);
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_usage_agent ON usage_records(agent_id)",
            [],
        );

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
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                ttft_ms, duration_ms, tps, cost_usd, session_id, agent_id, estimated, status
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
                r.reasoning_tokens as i64,
                r.ttft_ms.map(|v| v as i64),
                r.duration_ms as i64,
                r.tps,
                r.cost_usd,
                r.session_id,
                r.agent_id,
                r.estimated as i64,
                r.status.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn recent_usage(&self, limit: usize) -> anyhow::Result<Vec<UsageRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT id, ts, provider_id, provider_name, protocol, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    ttft_ms, duration_ms, tps, cost_usd, session_id, agent_id, estimated, status
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
                reasoning_tokens: row.get::<_, Option<i64>>(10)?.unwrap_or(0) as u64,
                ttft_ms: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                duration_ms: row.get::<_, i64>(12)? as u64,
                tps: row.get(13)?,
                cost_usd: row.get(14)?,
                session_id: row.get(15)?,
                agent_id: row.get(16)?,
                estimated: row.get::<_, i64>(17)? != 0,
                status: Status::from_db(&row.get::<_, String>(18)?),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn summary_since(&self, since_ms: i64) -> anyhow::Result<UsageSummary> {
        self.summary_since_for_agent(since_ms, None)
    }

    pub fn summary_since_for_agent(
        &self,
        since_ms: i64,
        agent_id: Option<&str>,
    ) -> anyhow::Result<UsageSummary> {
        let conn = self.conn.lock().unwrap();
        let mut out = UsageSummary::default();
        if let Some(ag) = agent_id {
            let mut stmt = conn.prepare_cached(
                "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                        COALESCE(SUM(reasoning_tokens),0),
                        COALESCE(SUM(cost_usd),0), COUNT(*), AVG(tps)
                 FROM usage_records WHERE ts >= ?1 AND status = 'ok' AND agent_id = ?2",
            )?;
            stmt.query_row(params![since_ms, ag], |row| {
                out.input_tokens = row.get::<_, i64>(0)? as u64;
                out.output_tokens = row.get::<_, i64>(1)? as u64;
                out.cache_read_tokens = row.get::<_, i64>(2)? as u64;
                out.cache_write_tokens = row.get::<_, i64>(3)? as u64;
                out.reasoning_tokens = row.get::<_, i64>(4)? as u64;
                out.cost_usd = row.get::<_, f64>(5)?;
                out.request_count = row.get::<_, i64>(6)? as u64;
                out.avg_tps = row.get(7)?;
                Ok(())
            })?;
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_tokens),0),
                        COALESCE(SUM(reasoning_tokens),0),
                        COALESCE(SUM(cost_usd),0), COUNT(*), AVG(tps)
                 FROM usage_records WHERE ts >= ?1 AND status = 'ok'",
            )?;
            stmt.query_row(params![since_ms], |row| {
                out.input_tokens = row.get::<_, i64>(0)? as u64;
                out.output_tokens = row.get::<_, i64>(1)? as u64;
                out.cache_read_tokens = row.get::<_, i64>(2)? as u64;
                out.cache_write_tokens = row.get::<_, i64>(3)? as u64;
                out.reasoning_tokens = row.get::<_, i64>(4)? as u64;
                out.cost_usd = row.get::<_, f64>(5)?;
                out.request_count = row.get::<_, i64>(6)? as u64;
                out.avg_tps = row.get(7)?;
                Ok(())
            })?;
        }
        Ok(out)
    }

    pub fn daily_usage(&self, since_ms: i64, max_days: usize) -> anyhow::Result<Vec<DayUsage>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT date(ts / 1000, 'unixepoch') AS d,
                    COALESCE(SUM(input_tokens + output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0),
                    COUNT(*)
             FROM usage_records
             WHERE ts >= ?1 AND status = 'ok'
             GROUP BY d
             ORDER BY d DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since_ms, max_days as i64], |row| {
            Ok(DayUsage {
                date: row.get(0)?,
                tokens: row.get::<_, i64>(1)? as u64,
                cost_usd: row.get::<_, f64>(2)?,
                requests: row.get::<_, i64>(3)? as u64,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        out.reverse();
        Ok(out)
    }

    pub fn sessions(&self, limit: usize) -> anyhow::Result<Vec<SessionInfo>> {
        self.sessions_filtered(None, limit, false)
    }

    pub fn sessions_filtered(
        &self,
        agent_filter: Option<&str>,
        limit: usize,
        scan_local: bool,
    ) -> anyhow::Result<Vec<SessionInfo>> {
        let mut out = Vec::new();
        {
            let conn = self.conn.lock().unwrap();
            let sql = if agent_filter.is_some() {
                "SELECT COALESCE(session_id, 'default'), COALESCE(agent_id, 'unknown'),
                        MIN(ts) as started, MAX(ts) as last,
                        COUNT(*), SUM(input_tokens), SUM(output_tokens), SUM(cost_usd)
                 FROM usage_records
                 WHERE session_id IS NOT NULL AND agent_id = ?1
                 GROUP BY session_id, agent_id ORDER BY last DESC LIMIT ?2"
            } else {
                "SELECT COALESCE(session_id, 'default'), COALESCE(agent_id, 'unknown'),
                        MIN(ts) as started, MAX(ts) as last,
                        COUNT(*), SUM(input_tokens), SUM(output_tokens), SUM(cost_usd)
                 FROM usage_records
                 WHERE session_id IS NOT NULL
                 GROUP BY session_id, agent_id ORDER BY last DESC LIMIT ?1"
            };
            let mut stmt = conn.prepare_cached(sql)?;
            if let Some(ag) = agent_filter {
                let rows = stmt.query_map(params![ag, limit as i64], |row| {
                    Ok(SessionInfo {
                        session_id: row.get(0)?,
                        agent_id: row.get(1)?,
                        source_kind: "proxy".to_string(),
                        started_at: DateTime::from_timestamp_millis(row.get(2)?)
                            .unwrap_or_else(Utc::now),
                        last_activity: DateTime::from_timestamp_millis(row.get(3)?)
                            .unwrap_or_else(Utc::now),
                        request_count: row.get::<_, i64>(4)? as u64,
                        total_input: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                        total_output: row.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                        total_cost: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                    })
                })?;
                for r in rows {
                    out.push(r?);
                }
            } else {
                let rows = stmt.query_map(params![limit as i64], |row| {
                    Ok(SessionInfo {
                        session_id: row.get(0)?,
                        agent_id: row.get(1)?,
                        source_kind: "proxy".to_string(),
                        started_at: DateTime::from_timestamp_millis(row.get(2)?)
                            .unwrap_or_else(Utc::now),
                        last_activity: DateTime::from_timestamp_millis(row.get(3)?)
                            .unwrap_or_else(Utc::now),
                        request_count: row.get::<_, i64>(4)? as u64,
                        total_input: row.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64,
                        total_output: row.get::<_, Option<i64>>(6)?.unwrap_or(0) as u64,
                        total_cost: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
                    })
                })?;
                for r in rows {
                    out.push(r?);
                }
            }
        }

        if scan_local {
            // 避免同一个物理目录 (如 codex 与 codex-desktop 共用 ~/.codex/sessions) 被重复扫描
            let mut visited_dirs = std::collections::HashSet::new();
            for agent in AgentId::ALL {
                if let Some(filter) = agent_filter {
                    if agent.as_str() != filter {
                        continue;
                    }
                }
                if let Some(dir) = agent.session_dir_path() {
                    if dir.exists() && visited_dirs.insert(dir.clone()) {
                        collect_local_sessions_recursive(&dir, agent.as_str(), 0, &mut out, 25);
                    }
                }
            }
            out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
            out.truncate(limit);
        }

        Ok(out)
    }

    pub fn export_csv(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let recs = self.recent_usage(100_000)?;
        let mut wtr = csv::Writer::from_path(path)?;
        wtr.write_record([
            "id",
            "ts",
            "agent",
            "provider",
            "protocol",
            "model",
            "input",
            "output",
            "cache_read",
            "cache_write",
            "reasoning",
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
                r.agent_id.unwrap_or_default(),
                r.provider_name,
                r.protocol.as_str().to_string(),
                r.model,
                r.input_tokens.to_string(),
                r.output_tokens.to_string(),
                r.cache_read_tokens.to_string(),
                r.cache_write_tokens.to_string(),
                r.reasoning_tokens.to_string(),
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

/// 递归最多 3 层扫描各 Agent 本地会话目录 (如 ~/.claude/projects/<proj>/*.jsonl, ~/.codex/sessions/YYYY/MM/DD/*.jsonl)
/// 仅提取数字型 usage 指标 (input_tokens / output_tokens)，不留存任何对话内容或个人目录路径
fn collect_local_sessions_recursive(
    dir: &Path,
    agent_slug: &str,
    depth: usize,
    out: &mut Vec<SessionInfo>,
    max_per_agent: usize,
) {
    if depth > 3 || out.len() >= max_per_agent * 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut added = 0usize;
    for entry in entries.flatten() {
        if added >= max_per_agent {
            break;
        }
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_local_sessions_recursive(&path, agent_slug, depth + 1, out, max_per_agent);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "jsonl" | "json") {
            continue;
        }
        let modified: DateTime<Utc> = meta
            .modified()
            .ok()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        let created: DateTime<Utc> = meta
            .created()
            .ok()
            .map(DateTime::<Utc>::from)
            .unwrap_or(modified);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session")
            .to_string();

        let (reqs, inp, out_tok) = parse_local_jsonl_usage(&path, meta.len());
        out.push(SessionInfo {
            session_id: stem,
            agent_id: agent_slug.to_string(),
            source_kind: "local".to_string(),
            started_at: created,
            last_activity: modified,
            request_count: reqs.max(1),
            total_input: inp,
            total_output: out_tok,
            total_cost: 0.0,
        });
        added += 1;
    }
}

/// 快速扫描本地 .jsonl 会话文件中的 usage 数字字段 (单文件上限扫描前 256 行, <1ms, 零阻塞)
fn parse_local_jsonl_usage(path: &Path, file_len: u64) -> (u64, u64, u64) {
    let Ok(file) = std::fs::File::open(path) else {
        return (1, file_len / 20, 0);
    };
    let reader = BufReader::new(file);
    let mut reqs = 0u64;
    let mut total_in = 0u64;
    let mut total_out = 0u64;

    for line in reader.lines().take(256).flatten() {
        if !line.contains("usage") && !line.contains("tokens") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let usage = v
            .pointer("/message/usage")
            .or_else(|| v.get("usage"))
            .or_else(|| v.pointer("/payload/usage"));
        if let Some(u) = usage {
            let i = u
                .get("input_tokens")
                .or_else(|| u.get("prompt_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let o = u
                .get("output_tokens")
                .or_else(|| u.get("completion_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            if i > 0 || o > 0 {
                reqs += 1;
                total_in += i;
                total_out += o;
            }
        }
    }
    if reqs == 0 {
        (1, file_len / 20, 0)
    } else {
        (reqs, total_in, total_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Protocol;
    use chrono::NaiveDate;
    use std::sync::Arc;

    fn in_memory() -> Store {
        let store = Store {
            conn: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        };
        store.migrate().unwrap();
        store
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
            .and_utc()
    }

    fn record(ts: DateTime<Utc>, input: u64, output: u64, cost: f64, status: Status) -> UsageRecord {
        let mut r = UsageRecord::new(
            "p1".into(),
            "Prov".into(),
            Protocol::Anthropic,
            "claude-sonnet-4-5".into(),
        );
        r.ts = ts;
        r.input_tokens = input;
        r.output_tokens = output;
        r.cost_usd = Some(cost);
        r.status = status;
        r
    }

    #[test]
    fn daily_usage_groups_by_utc_day() {
        let store = in_memory();
        store.insert_usage(&record(at(2026, 10, 11, 23), 100, 50, 0.5, Status::Ok)).unwrap();
        store.insert_usage(&record(at(2026, 10, 12, 1), 1000, 500, 1.25, Status::Ok)).unwrap();
        store.insert_usage(&record(at(2026, 10, 12, 22), 200, 100, 0.25, Status::Ok)).unwrap();

        let days = store.daily_usage(0, 90).unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].date, "2026-10-11");
        assert_eq!(days[0].tokens, 150);
        assert_eq!(days[1].date, "2026-10-12");
        assert_eq!(days[1].tokens, 1800);
        assert_eq!(days[1].requests, 2);
        assert!((days[1].cost_usd - 1.5).abs() < 1e-9);
    }

    #[test]
    fn daily_usage_skips_errors_and_respects_window() {
        let store = in_memory();
        store.insert_usage(&record(at(2026, 10, 12, 9), 100, 100, 0.1, Status::Error)).unwrap();
        store.insert_usage(&record(at(2026, 10, 12, 10), 100, 100, 0.1, Status::Aborted)).unwrap();
        assert!(store.daily_usage(0, 90).unwrap().is_empty());

        store.insert_usage(&record(at(2026, 10, 13, 10), 7, 7, 0.0, Status::Ok)).unwrap();
        let since = at(2026, 10, 13, 0).timestamp_millis();
        let days = store.daily_usage(since, 90).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].date, "2026-10-13");
    }
}
