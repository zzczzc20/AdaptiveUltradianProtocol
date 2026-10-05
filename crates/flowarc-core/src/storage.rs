//! 本地 SQLite 存储。默认完全本地、无账号。

use crate::model::*;
use crate::rules::BlockOutcome;
use crate::session::EngineState;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, StorageError>;

pub struct Storage {
    conn: Connection,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS blocks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL, mode TEXT NOT NULL,
  intent TEXT NOT NULL DEFAULT '', criterion TEXT NOT NULL DEFAULT '',
  start INTEGER NOT NULL, focus_start INTEGER NOT NULL, end INTEGER NOT NULL DEFAULT 0,
  soft_floor_min INTEGER NOT NULL, cap_min INTEGER NOT NULL,
  end_mode TEXT, early_reason TEXT, postponed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS snapshots (
  block_id INTEGER PRIMARY KEY REFERENCES blocks(id) ON DELETE CASCADE,
  stopped_at TEXT NOT NULL DEFAULT '', open_question TEXT NOT NULL DEFAULT '',
  first_step TEXT NOT NULL DEFAULT '',
  flow INTEGER, fatigue INTEGER, completion TEXT,
  context TEXT NOT NULL DEFAULT '[]', created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS breaks (
  id INTEGER PRIMARY KEY AUTOINCREMENT, block_id INTEGER NOT NULL,
  recommended TEXT NOT NULL, actual TEXT, planned_min INTEGER NOT NULL,
  actual_min INTEGER, occupied INTEGER NOT NULL DEFAULT 0,
  start INTEGER NOT NULL, end INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS checkins (
  date TEXT PRIMARY KEY, sleep_hours REAL NOT NULL, energy INTEGER NOT NULL,
  gold_budget INTEGER NOT NULL, silver_budget INTEGER NOT NULL,
  cap_delta INTEGER NOT NULL, end_fatigue INTEGER
);
CREATE TABLE IF NOT EXISTS parking (
  id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL,
  created INTEGER NOT NULL, block_id INTEGER, done INTEGER NOT NULL DEFAULT 0
);
-- 每分钟一条：仅记录“是否活跃”与前台应用名，绝不记录按键内容
CREATE TABLE IF NOT EXISTS activity (
  minute INTEGER PRIMARY KEY, active INTEGER NOT NULL, app TEXT
);
CREATE INDEX IF NOT EXISTS idx_blocks_start ON blocks(start);
"#;

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }
    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }
    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Storage { conn })
    }

    // ── kv / 设置 / 引擎状态 ──
    fn kv_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT value FROM kv WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }
    fn kv_set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO kv(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, value],
        )?;
        Ok(())
    }
    pub fn settings(&self) -> Result<Settings> {
        Ok(match self.kv_get("settings")? {
            Some(j) => serde_json::from_str(&j).unwrap_or_default(),
            None => Settings::default(),
        })
    }
    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.kv_set("settings", &serde_json::to_string(s)?)
    }
    pub fn load_state(&self) -> Result<EngineState> {
        Ok(match self.kv_get("engine_state")? {
            Some(j) => serde_json::from_str(&j).unwrap_or_default(),
            None => EngineState::default(),
        })
    }
    pub fn save_state(&self, st: &EngineState) -> Result<()> {
        self.kv_set("engine_state", &serde_json::to_string(st)?)
    }
    /// 个人上限 U（由规则引擎逐步调整后持久化）
    pub fn personal_cap(&self, default: u32) -> Result<u32> {
        Ok(self.kv_get("personal_cap")?.and_then(|v| v.parse().ok()).unwrap_or(default))
    }
    pub fn set_personal_cap(&self, v: u32) -> Result<()> {
        self.kv_set("personal_cap", &v.to_string())
    }

    // ── 块 ──
    pub fn insert_block(&self, kind: BlockKind, mode: WorkMode, intent: &str, criterion: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO blocks(kind,mode,intent,criterion,start,focus_start,soft_floor_min,cap_min)
             VALUES(?1,?2,?3,?4,0,0,0,0)",
            params![kind.as_str(), mode.as_str(), intent, criterion],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn update_block(&self, b: &Block) -> Result<()> {
        self.conn.execute(
            "UPDATE blocks SET kind=?2,mode=?3,intent=?4,criterion=?5,start=?6,focus_start=?7,end=?8,
             soft_floor_min=?9,cap_min=?10,end_mode=?11,early_reason=?12,postponed=?13 WHERE id=?1",
            params![
                b.id,
                b.kind.unwrap_or(BlockKind::Gold).as_str(),
                b.mode.unwrap_or(WorkMode::Solo).as_str(),
                b.intent,
                b.criterion,
                b.start,
                b.focus_start,
                b.end,
                b.soft_floor_min,
                b.cap_min,
                b.end_mode.map(|m| m.as_str()),
                b.early_reason,
                b.postponed as i32
            ],
        )?;
        Ok(())
    }
    /// 删除从未进入专注的空块（例如启动后立刻放弃）
    pub fn delete_block(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM blocks WHERE id=?1", [id])?;
        Ok(())
    }

    // ── 快照 ──
    pub fn save_snapshot(&self, s: &Snapshot) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO snapshots(block_id,stopped_at,open_question,first_step,flow,fatigue,completion,context,created)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                s.block_id,
                s.stopped_at,
                s.open_question,
                s.first_step,
                s.flow,
                s.fatigue,
                s.completion,
                serde_json::to_string(&s.context)?,
                s.created
            ],
        )?;
        Ok(())
    }
    pub fn latest_snapshot(&self) -> Result<Option<Snapshot>> {
        Ok(self
            .conn
            .query_row(
                "SELECT block_id,stopped_at,open_question,first_step,flow,fatigue,completion,context,created
                 FROM snapshots ORDER BY created DESC LIMIT 1",
                [],
                |r| {
                    let ctx: String = r.get(7)?;
                    Ok(Snapshot {
                        block_id: r.get(0)?,
                        stopped_at: r.get(1)?,
                        open_question: r.get(2)?,
                        first_step: r.get(3)?,
                        flow: r.get(4)?,
                        fatigue: r.get(5)?,
                        completion: r.get(6)?,
                        context: serde_json::from_str(&ctx).unwrap_or_default(),
                        created: r.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    // ── 休息 / 签到 ──
    pub fn insert_break(&self, b: &BreakRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO breaks(block_id,recommended,actual,planned_min,actual_min,occupied,start,end)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![b.block_id, b.recommended, b.actual, b.planned_min, b.actual_min, b.occupied as i32, b.start, b.end],
        )?;
        Ok(())
    }
    pub fn save_checkin(&self, c: &DayCheckin) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO checkins(date,sleep_hours,energy,gold_budget,silver_budget,cap_delta,end_fatigue)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![c.date, c.sleep_hours, c.energy, c.gold_budget, c.silver_budget, c.cap_delta, c.end_fatigue],
        )?;
        Ok(())
    }
    pub fn get_checkin(&self, date: &str) -> Result<Option<DayCheckin>> {
        Ok(self
            .conn
            .query_row(
                "SELECT date,sleep_hours,energy,gold_budget,silver_budget,cap_delta,end_fatigue FROM checkins WHERE date=?1",
                [date],
                |r| {
                    Ok(DayCheckin {
                        date: r.get(0)?,
                        sleep_hours: r.get(1)?,
                        energy: r.get(2)?,
                        gold_budget: r.get(3)?,
                        silver_budget: r.get(4)?,
                        cap_delta: r.get(5)?,
                        end_fatigue: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    // ── 停车场 ──
    pub fn add_parking(&self, text: &str, now: i64, block_id: Option<i64>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO parking(text,created,block_id) VALUES(?1,?2,?3)",
            params![text, now, block_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
    pub fn list_parking(&self, include_done: bool) -> Result<Vec<ParkingItem>> {
        let sql = if include_done {
            "SELECT id,text,created,block_id FROM parking ORDER BY id DESC"
        } else {
            "SELECT id,text,created,block_id FROM parking WHERE done=0 ORDER BY id DESC"
        };
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map([], |r| {
            Ok(ParkingItem { id: r.get(0)?, text: r.get(1)?, created: r.get(2)?, block_id: r.get(3)? })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn complete_parking(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE parking SET done=1 WHERE id=?1", [id])?;
        Ok(())
    }

    // ── 传感：每分钟活动 ──
    pub fn record_activity(&self, now: i64, active: bool, app: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO activity(minute,active,app) VALUES(?1,?2,?3)",
            params![now / 60, active as i32, app],
        )?;
        Ok(())
    }

    // ── 规则引擎输入 ──
    /// 最近 n 个已完成块（旧→新）
    pub fn recent_outcomes(&self, n: usize) -> Result<Vec<BlockOutcome>> {
        let mut st = self.conn.prepare(
            "SELECT b.end-b.focus_start, b.cap_min, s.flow, s.fatigue, s.completion
             FROM blocks b JOIN snapshots s ON s.block_id=b.id
             WHERE b.end>0 ORDER BY b.start DESC LIMIT ?1",
        )?;
        let mut v: Vec<BlockOutcome> = st
            .query_map([n as i64], |r| {
                let secs: i64 = r.get(0)?;
                let comp: Option<String> = r.get(4)?;
                Ok(BlockOutcome {
                    focus_min: (secs.max(0) / 60) as u32,
                    cap_min: r.get::<_, i64>(1)? as u32,
                    flow: r.get::<_, Option<i64>>(2)?.map(|x| x as u8),
                    fatigue: r.get::<_, Option<i64>>(3)?.map(|x| x as u8),
                    criterion_met: comp.as_deref().and_then(Completion::parse).map(|c| c == Completion::Yes),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        v.reverse();
        Ok(v)
    }

    /// (小时, 心流) 样本，用于峰值时段识别
    pub fn hourly_flow_samples(&self) -> Result<Vec<(u8, u8)>> {
        let mut st = self.conn.prepare(
            "SELECT CAST(strftime('%H', b.focus_start,'unixepoch','localtime') AS INTEGER), s.flow
             FROM blocks b JOIN snapshots s ON s.block_id=b.id WHERE s.flow IS NOT NULL AND b.end>0",
        )?;
        let v = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)? as u8, r.get::<_, i64>(1)? as u8)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(v)
    }

    pub fn day_summary(&self, date: &str) -> Result<DaySummary> {
        let mut sum = DaySummary { date: date.to_string(), ..Default::default() };
        let mut st = self.conn.prepare(
            "SELECT b.kind, b.end-b.focus_start, b.end_mode, s.flow
             FROM blocks b LEFT JOIN snapshots s ON s.block_id=b.id
             WHERE b.end>0 AND date(b.start,'unixepoch','localtime')=?1",
        )?;
        let mut flows = vec![];
        let rows = st.query_map([date], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<i64>>(3)?))
        })?;
        for row in rows {
            let (kind, secs, mode, flow) = row?;
            sum.blocks += 1;
            sum.deep_minutes += (secs.max(0) / 60) as u32;
            match kind.as_str() {
                "gold" => sum.gold_done += 1,
                "silver" => sum.silver_done += 1,
                _ => {}
            }
            if mode.as_deref() == Some("breakpoint") {
                sum.breakpoint_landings += 1;
            }
            if let Some(f) = flow {
                flows.push(f as f32);
            }
        }
        if !flows.is_empty() {
            sum.mean_flow = Some(flows.iter().sum::<f32>() / flows.len() as f32);
        }
        sum.end_fatigue = self.get_checkin(date)?.and_then(|c| c.end_fatigue);
        sum.parking = self.list_parking(false)?.into_iter().map(|p| p.text).collect();
        Ok(sum)
    }

    /// 最近两天心流均值（用于次日加块修正）
    pub fn recent_flow_mean(&self, days: u32) -> Result<Option<f32>> {
        let m: Option<f64> = self.conn.query_row(
            "SELECT AVG(s.flow) FROM blocks b JOIN snapshots s ON s.block_id=b.id
             WHERE s.flow IS NOT NULL AND b.start >= strftime('%s','now', ?1)",
            [format!("-{} days", days)],
            |r| r.get(0),
        )?;
        Ok(m.map(|x| x as f32))
    }

    // ── 隐私：保留期、导出、全部删除 ──
    /// 清理超过保留期的上下文（窗口标题/URL 等）与活动记录。days=0 表示立即清空。
    pub fn prune_context(&self, retention_days: u32, now: i64) -> Result<()> {
        let cutoff = now - retention_days as i64 * 86_400;
        self.conn
            .execute("UPDATE snapshots SET context='[]' WHERE created < ?1", [cutoff])?;
        self.conn.execute("DELETE FROM activity WHERE minute < ?1", [cutoff / 60])?;
        Ok(())
    }

    pub fn export_json(&self) -> Result<String> {
        fn table(conn: &Connection, sql: &str) -> Result<Vec<serde_json::Value>> {
            let mut st = conn.prepare(sql)?;
            let cols: Vec<String> = st.column_names().iter().map(|s| s.to_string()).collect();
            let rows = st.query_map([], |r| {
                let mut m = serde_json::Map::new();
                for (i, c) in cols.iter().enumerate() {
                    let v: rusqlite::types::Value = r.get(i)?;
                    m.insert(
                        c.clone(),
                        match v {
                            rusqlite::types::Value::Null => serde_json::Value::Null,
                            rusqlite::types::Value::Integer(i) => i.into(),
                            rusqlite::types::Value::Real(f) => serde_json::json!(f),
                            rusqlite::types::Value::Text(t) => t.into(),
                            rusqlite::types::Value::Blob(_) => serde_json::Value::Null,
                        },
                    );
                }
                Ok(serde_json::Value::Object(m))
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        }
        let c = &self.conn;
        let out = serde_json::json!({
            "app": "FlowArc", "format": 1,
            "settings": self.settings()?,
            "blocks": table(c, "SELECT * FROM blocks ORDER BY id")?,
            "snapshots": table(c, "SELECT * FROM snapshots ORDER BY block_id")?,
            "breaks": table(c, "SELECT * FROM breaks ORDER BY id")?,
            "checkins": table(c, "SELECT * FROM checkins ORDER BY date")?,
            "parking": table(c, "SELECT * FROM parking ORDER BY id")?,
        });
        Ok(serde_json::to_string_pretty(&out)?)
    }

    /// 块 + 快照的扁平 CSV
    pub fn export_csv(&self) -> Result<String> {
        fn esc(s: &str) -> String {
            if s.contains(['"', ',', '\n', '\r']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_string()
            }
        }
        let mut out = String::from(
            "block_id,kind,mode,start,focus_start,end,soft_floor_min,cap_min,end_mode,early_reason,intent,criterion,flow,fatigue,completion,first_step\n",
        );
        let mut st = self.conn.prepare(
            "SELECT b.id,b.kind,b.mode,b.start,b.focus_start,b.end,b.soft_floor_min,b.cap_min,
                    COALESCE(b.end_mode,''),COALESCE(b.early_reason,''),b.intent,b.criterion,
                    COALESCE(s.flow,''),COALESCE(s.fatigue,''),COALESCE(s.completion,''),COALESCE(s.first_step,'')
             FROM blocks b LEFT JOIN snapshots s ON s.block_id=b.id ORDER BY b.id",
        )?;
        let rows = st.query_map([], |r| {
            let mut f = Vec::new();
            for i in 0..16 {
                let v: rusqlite::types::Value = r.get(i)?;
                f.push(match v {
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Real(x) => x.to_string(),
                    rusqlite::types::Value::Text(t) => esc(&t),
                    _ => String::new(),
                });
            }
            Ok(f.join(","))
        })?;
        for row in rows {
            out.push_str(&row?);
            out.push('\n');
        }
        Ok(out)
    }

    /// 清空全部数据（设置也一并重置）
    pub fn delete_all(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM snapshots; DELETE FROM breaks; DELETE FROM blocks; DELETE FROM checkins;
             DELETE FROM parking; DELETE FROM activity; DELETE FROM kv; VACUUM;",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finished_block(db: &Storage, start: i64, focus_min: i64, flow: u8, fat: u8) -> i64 {
        let id = db.insert_block(BlockKind::Gold, WorkMode::Solo, "写作, \"含逗号\"", "完成").unwrap();
        let b = Block {
            id,
            kind: Some(BlockKind::Gold),
            mode: Some(WorkMode::Solo),
            intent: "写作, \"含逗号\"".into(),
            criterion: "完成".into(),
            start,
            focus_start: start + 180,
            end: start + 180 + focus_min * 60,
            soft_floor_min: 45,
            cap_min: 90,
            end_mode: Some(EndMode::Breakpoint),
            ..Default::default()
        };
        db.update_block(&b).unwrap();
        db.save_snapshot(&Snapshot {
            block_id: id,
            first_step: "继续".into(),
            flow: Some(flow),
            fatigue: Some(fat),
            completion: Some("yes".into()),
            created: b.end,
            ..Default::default()
        })
        .unwrap();
        id
    }

    #[test]
    fn settings_and_state_persist() {
        let db = Storage::open_memory().unwrap();
        assert_eq!(db.settings().unwrap().cap_min, 90);
        let mut s = Settings::default();
        s.cap_min = 75;
        db.save_settings(&s).unwrap();
        assert_eq!(db.settings().unwrap().cap_min, 75);
        let mut st = EngineState::default();
        st.gold_done = 2;
        db.save_state(&st).unwrap();
        assert_eq!(db.load_state().unwrap().gold_done, 2);
        assert_eq!(db.personal_cap(90).unwrap(), 90);
        db.set_personal_cap(95).unwrap();
        assert_eq!(db.personal_cap(90).unwrap(), 95);
    }

    #[test]
    fn outcomes_and_latest_snapshot() {
        let db = Storage::open_memory().unwrap();
        for i in 0..4 {
            finished_block(&db, 1_000_000 + i * 10_000, 60 + i, 4, 2);
        }
        let o = db.recent_outcomes(3).unwrap();
        assert_eq!(o.len(), 3);
        assert_eq!(o[2].focus_min, 63); // 旧→新
        assert_eq!(o[0].flow, Some(4));
        assert_eq!(o[0].criterion_met, Some(true));
        assert_eq!(db.latest_snapshot().unwrap().unwrap().first_step, "继续");
    }

    #[test]
    fn parking_lot() {
        let db = Storage::open_memory().unwrap();
        let a = db.add_parking("回复邮件", 1, None).unwrap();
        db.add_parking("查文献", 2, Some(7)).unwrap();
        assert_eq!(db.list_parking(false).unwrap().len(), 2);
        db.complete_parking(a).unwrap();
        let l = db.list_parking(false).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "查文献");
    }

    #[test]
    fn checkin_roundtrip_and_summary() {
        let db = Storage::open_memory().unwrap();
        // 使用 SQLite 的 now，使日期与 localtime 一致
        let (date, now): (String, i64) = db
            .conn
            .query_row("SELECT date('now','localtime'), CAST(strftime('%s','now') AS INTEGER)", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        finished_block(&db, now - 3_000, 40, 5, 2);
        db.save_checkin(&DayCheckin {
            date: date.clone(), sleep_hours: 7.0, energy: 3, gold_budget: 3, silver_budget: 2, cap_delta: 0, end_fatigue: Some(2),
        })
        .unwrap();
        let s = db.day_summary(&date).unwrap();
        assert_eq!((s.blocks, s.gold_done, s.deep_minutes), (1, 1, 40));
        assert_eq!(s.mean_flow, Some(5.0));
        assert_eq!(s.end_fatigue, Some(2));
        assert_eq!(s.breakpoint_landings, 1);
    }

    #[test]
    fn export_and_delete() {
        let db = Storage::open_memory().unwrap();
        finished_block(&db, 1_000_000, 50, 4, 2);
        let j = db.export_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["blocks"].as_array().unwrap().len(), 1);
        let csv = db.export_csv().unwrap();
        assert!(csv.lines().count() == 2);
        assert!(csv.contains("\"写作, \"\"含逗号\"\"\""));
        db.delete_all().unwrap();
        assert!(db.recent_outcomes(5).unwrap().is_empty());
        assert!(db.latest_snapshot().unwrap().is_none());
    }

    #[test]
    fn prune_context_clears_old_only() {
        let db = Storage::open_memory().unwrap();
        let id = finished_block(&db, 1_000, 50, 4, 2);
        let mut s = db.latest_snapshot().unwrap().unwrap();
        s.block_id = id;
        s.context = vec![ContextItem { kind: "window".into(), value: "secret".into(), keep: true }];
        db.save_snapshot(&s).unwrap();
        db.prune_context(30, s.created + 31 * 86_400).unwrap();
        assert!(db.latest_snapshot().unwrap().unwrap().context.is_empty());
    }

    #[test]
    fn activity_never_stores_content() {
        let db = Storage::open_memory().unwrap();
        db.record_activity(600, true, Some("Code")).unwrap();
        db.record_activity(660, false, None).unwrap();
        let n: i64 = db.conn.query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }
}
