//! SQLite 儲存：專案、聲音庫、錄音片段。
//!
//! 單機單使用者的桌面程式，SQLite 就夠了（評估見 docs/architecture.md）。
//! 結構變更一律寫成 `MIGRATIONS` 的新項目，用 `PRAGMA user_version` 記錄版本。

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::model::{Project, Take, Voice, VoiceKind, now};
use crate::{Error, Result};

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE projects (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL UNIQUE,
        script      TEXT NOT NULL DEFAULT '',
        created_at  TEXT NOT NULL,
        updated_at  TEXT NOT NULL
    );
    CREATE TABLE voices (
        id          TEXT PRIMARY KEY,
        name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
        engine_id   TEXT NOT NULL,
        kind_json   TEXT NOT NULL,
        notes       TEXT NOT NULL DEFAULT '',
        created_at  TEXT NOT NULL
    );
    CREATE TABLE takes (
        id            TEXT PRIMARY KEY,
        project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        text          TEXT NOT NULL,
        voice_name    TEXT NOT NULL,
        engine_id     TEXT NOT NULL,
        path          TEXT NOT NULL,
        duration_sec  REAL NOT NULL,
        sample_rate   INTEGER NOT NULL,
        segments      INTEGER NOT NULL,
        elapsed_sec   REAL NOT NULL,
        loudness_lufs REAL,
        starred       INTEGER NOT NULL DEFAULT 0,
        created_at    TEXT NOT NULL
    );
    CREATE INDEX takes_project_created ON takes(project_id, created_at DESC);
    "#,
    // v2：時間軸順序。新 take 排在專案最後面。
    r#"
    ALTER TABLE takes ADD COLUMN position REAL NOT NULL DEFAULT 0;
    UPDATE takes SET position = rowid;
    CREATE INDEX takes_project_position ON takes(project_id, position);
    "#,
];

/// 沒有指定專案時，合成結果歸到這個專案。
pub const QUICK_PROJECT: &str = "快速合成";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let mut store = Self { conn };
        store.migrate()?;
        store.ensure_project(QUICK_PROJECT)?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self.conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(usize::try_from(version).unwrap_or(0)) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(())
    }

    // ---- 專案 ----

    pub fn ensure_project(&self, name: &str) -> Result<Project> {
        if let Some(p) = self.project_by_name(name)? {
            return Ok(p);
        }
        let ts = now();
        let p = Project { id: Uuid::new_v4(), name: name.into(), script: String::new(), created_at: ts, updated_at: ts };
        self.conn.execute(
            "INSERT INTO projects (id, name, script, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.id.to_string(), p.name, p.script, fmt(p.created_at), fmt(p.updated_at)],
        )?;
        Ok(p)
    }

    pub fn project_by_name(&self, name: &str) -> Result<Option<Project>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, script, created_at, updated_at FROM projects WHERE name = ?1",
                [name],
                row_project,
            )
            .optional()?)
    }

    pub fn project(&self, id: Uuid) -> Result<Project> {
        self.conn
            .query_row(
                "SELECT id, name, script, created_at, updated_at FROM projects WHERE id = ?1",
                [id.to_string()],
                row_project,
            )
            .optional()?
            .ok_or_else(|| Error::not_found("專案", id.to_string()))
    }

    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, script, created_at, updated_at FROM projects ORDER BY updated_at DESC")?;
        let rows = stmt.query_map([], row_project)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn save_script(&self, id: Uuid, script: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE projects SET script = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), script, fmt(now())],
        )?;
        if n == 0 { Err(Error::not_found("專案", id.to_string())) } else { Ok(()) }
    }

    pub fn rename_project(&self, id: Uuid, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("專案名稱不能是空的".into()));
        }
        self.conn.execute(
            "UPDATE projects SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.to_string(), name, fmt(now())],
        )?;
        Ok(())
    }

    // ---- 聲音庫 ----

    pub fn add_voice(&self, name: &str, engine_id: &str, kind: VoiceKind, notes: &str) -> Result<Voice> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("聲音名稱不能是空的".into()));
        }
        if self.voice_by_name(name)?.is_some() {
            return Err(Error::Invalid(format!("已經有叫「{name}」的聲音了")));
        }
        let v = Voice { id: Uuid::new_v4(), name: name.into(), engine_id: engine_id.into(), kind, notes: notes.into(), created_at: now() };
        self.conn.execute(
            "INSERT INTO voices (id, name, engine_id, kind_json, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![v.id.to_string(), v.name, v.engine_id, to_json(&v.kind)?, v.notes, fmt(v.created_at)],
        )?;
        Ok(v)
    }

    pub fn voices(&self) -> Result<Vec<Voice>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, engine_id, kind_json, notes, created_at FROM voices ORDER BY name")?;
        let rows = stmt.query_map([], row_voice)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn voice_by_name(&self, name: &str) -> Result<Option<Voice>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, engine_id, kind_json, notes, created_at FROM voices WHERE name = ?1",
                [name.trim()],
                row_voice,
            )
            .optional()?)
    }

    /// 用名稱或 UUID 找聲音。
    pub fn find_voice(&self, key: &str) -> Result<Option<Voice>> {
        if let Ok(id) = Uuid::parse_str(key.trim()) {
            let found = self
                .conn
                .query_row(
                    "SELECT id, name, engine_id, kind_json, notes, created_at FROM voices WHERE id = ?1",
                    [id.to_string()],
                    row_voice,
                )
                .optional()?;
            if found.is_some() {
                return Ok(found);
            }
        }
        self.voice_by_name(key)
    }

    pub fn delete_voice(&self, id: Uuid) -> Result<()> {
        self.conn.execute("DELETE FROM voices WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }

    // ---- 錄音片段 ----

    pub fn add_take(&self, t: &Take) -> Result<()> {
        let position: f64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM takes WHERE project_id = ?1",
            [t.project_id.to_string()],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO takes (id, project_id, text, voice_name, engine_id, path, duration_sec, sample_rate,
                                segments, elapsed_sec, loudness_lufs, starred, created_at, position)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                t.id.to_string(),
                t.project_id.to_string(),
                t.text,
                t.voice_name,
                t.engine_id,
                t.path,
                t.duration_sec,
                t.sample_rate,
                t.segments,
                t.elapsed_sec,
                t.loudness_lufs,
                t.starred,
                fmt(t.created_at),
                position,
            ],
        )?;
        self.conn.execute(
            "UPDATE projects SET updated_at = ?2 WHERE id = ?1",
            params![t.project_id.to_string(), fmt(now())],
        )?;
        Ok(())
    }

    pub fn takes(&self, project_id: Uuid, limit: usize) -> Result<Vec<Take>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, text, voice_name, engine_id, path, duration_sec, sample_rate, segments,
                    elapsed_sec, loudness_lufs, starred, created_at
             FROM takes WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![project_id.to_string(), limit as i64], row_take)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn recent_takes(&self, limit: usize) -> Result<Vec<Take>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, text, voice_name, engine_id, path, duration_sec, sample_rate, segments,
                    elapsed_sec, loudness_lufs, starred, created_at
             FROM takes ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], row_take)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 時間軸：依位置排序（由前到後）。
    pub fn timeline(&self, project_id: Uuid) -> Result<Vec<Take>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, text, voice_name, engine_id, path, duration_sec, sample_rate, segments,
                    elapsed_sec, loudness_lufs, starred, created_at
             FROM takes WHERE project_id = ?1 ORDER BY position, created_at",
        )?;
        let rows = stmt.query_map([project_id.to_string()], row_take)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// 把 take 在時間軸上往前（delta < 0）或往後移一格。
    pub fn move_take(&mut self, id: Uuid, delta: i32) -> Result<()> {
        let tx = self.conn.transaction()?;
        let (project, pos): (String, f64) =
            tx.query_row("SELECT project_id, position FROM takes WHERE id = ?1", [id.to_string()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let neighbour: Option<(String, f64)> = if delta < 0 {
            tx.query_row(
                "SELECT id, position FROM takes WHERE project_id = ?1 AND position < ?2 ORDER BY position DESC LIMIT 1",
                params![project, pos],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
        } else {
            tx.query_row(
                "SELECT id, position FROM takes WHERE project_id = ?1 AND position > ?2 ORDER BY position ASC LIMIT 1",
                params![project, pos],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
        };
        if let Some((other, other_pos)) = neighbour {
            tx.execute("UPDATE takes SET position = ?2 WHERE id = ?1", params![id.to_string(), other_pos])?;
            tx.execute("UPDATE takes SET position = ?2 WHERE id = ?1", params![other, pos])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 重新合成後，讓新 take 取代舊 take 在時間軸上的位置（舊紀錄移除，舊音檔保留）。
    pub fn replace_in_timeline(&mut self, old_id: Uuid, new_id: Uuid) -> Result<()> {
        let tx = self.conn.transaction()?;
        let pos: f64 = tx.query_row("SELECT position FROM takes WHERE id = ?1", [old_id.to_string()], |r| r.get(0))?;
        tx.execute("UPDATE takes SET position = ?2 WHERE id = ?1", params![new_id.to_string(), pos])?;
        tx.execute("DELETE FROM takes WHERE id = ?1", [old_id.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_starred(&self, id: Uuid, starred: bool) -> Result<()> {
        self.conn.execute("UPDATE takes SET starred = ?2 WHERE id = ?1", params![id.to_string(), starred])?;
        Ok(())
    }

    /// 只刪資料庫紀錄，音檔留著（檔案由使用者自己決定要不要刪）。
    pub fn forget_take(&self, id: Uuid) -> Result<()> {
        self.conn.execute("DELETE FROM takes WHERE id = ?1", [id.to_string()])?;
        Ok(())
    }
}

fn fmt(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).expect("RFC 3339 formatting never fails for UTC")
}

fn parse_time(s: String) -> rusqlite::Result<OffsetDateTime> {
    OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn parse_uuid(s: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&s)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::Invalid(e.to_string()))
}

fn row_project(r: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: parse_uuid(r.get(0)?)?,
        name: r.get(1)?,
        script: r.get(2)?,
        created_at: parse_time(r.get(3)?)?,
        updated_at: parse_time(r.get(4)?)?,
    })
}

fn row_voice(r: &Row) -> rusqlite::Result<Voice> {
    let kind_json: String = r.get(3)?;
    let kind = serde_json::from_str(&kind_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;
    Ok(Voice {
        id: parse_uuid(r.get(0)?)?,
        name: r.get(1)?,
        engine_id: r.get(2)?,
        kind,
        notes: r.get(4)?,
        created_at: parse_time(r.get(5)?)?,
    })
}

fn row_take(r: &Row) -> rusqlite::Result<Take> {
    Ok(Take {
        id: parse_uuid(r.get(0)?)?,
        project_id: parse_uuid(r.get(1)?)?,
        text: r.get(2)?,
        voice_name: r.get(3)?,
        engine_id: r.get(4)?,
        path: r.get(5)?,
        duration_sec: r.get(6)?,
        sample_rate: r.get(7)?,
        segments: r.get(8)?,
        elapsed_sec: r.get(9)?,
        loudness_lufs: r.get(10)?,
        starred: r.get(11)?,
        created_at: parse_time(r.get(12)?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Consent, ConsentBasis};

    #[test]
    fn migrations_are_idempotent_and_quick_project_exists() {
        let dir = std::env::temp_dir().join(format!("cantiva-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("t.db");
        {
            let s = Store::open(&db).unwrap();
            assert!(s.project_by_name(QUICK_PROJECT).unwrap().is_some());
        }
        let s = Store::open(&db).unwrap();
        assert_eq!(s.projects().unwrap().len(), 1);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn voices_are_unique_case_insensitive_and_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let kind = VoiceKind::Reference {
            audio_file: "me.wav".into(),
            transcript: "你好".into(),
            consent: Consent { basis: ConsentBasis::OwnVoice, statement: "我本人".into(), recorded_at: now() },
            language: None,
            trained: None,
        };
        let v = s.add_voice("Narrator", "qwen3-tts", kind.clone(), "").unwrap();
        assert!(s.add_voice("narrator", "qwen3-tts", kind, "").is_err());
        assert_eq!(s.find_voice("NARRATOR").unwrap().unwrap().id, v.id);
        assert_eq!(s.find_voice(&v.id.to_string()).unwrap().unwrap().name, "Narrator");
        assert!(matches!(s.voices().unwrap()[0].kind, VoiceKind::Reference { .. }));
        s.delete_voice(v.id).unwrap();
        assert!(s.voices().unwrap().is_empty());
    }

    #[test]
    fn takes_are_listed_newest_first() {
        let s = Store::open_in_memory().unwrap();
        let p = s.ensure_project("廣播劇").unwrap();
        for i in 0..3 {
            let t = Take {
                id: Uuid::new_v4(),
                project_id: p.id,
                text: format!("第{i}段"),
                voice_name: "旁白".into(),
                engine_id: "qwen3-tts".into(),
                path: format!("C:/x/{i}.wav"),
                duration_sec: 1.0,
                sample_rate: 24000,
                segments: 1,
                elapsed_sec: 0.5,
                loudness_lufs: Some(-16.0),
                starred: false,
                created_at: now() + time::Duration::seconds(i),
            };
            s.add_take(&t).unwrap();
        }
        let takes = s.takes(p.id, 10).unwrap();
        assert_eq!(takes.len(), 3);
        assert_eq!(takes[0].text, "第2段");
        s.set_starred(takes[0].id, true).unwrap();
        assert!(s.takes(p.id, 1).unwrap()[0].starred);
        let order: Vec<_> = s.timeline(p.id).unwrap().into_iter().map(|t| t.text).collect();
        assert_eq!(order, vec!["第0段", "第1段", "第2段"]);
        let mut s = s;
        let first = s.timeline(p.id).unwrap()[0].id;
        s.move_take(first, 1).unwrap();
        let order: Vec<_> = s.timeline(p.id).unwrap().into_iter().map(|t| t.text).collect();
        assert_eq!(order, vec!["第1段", "第0段", "第2段"]);
        s.move_take(first, -5).unwrap();
        s.move_take(first, -1).unwrap();
        assert_eq!(s.timeline(p.id).unwrap()[0].id, first);
        s.save_script(p.id, "@旁白：測試").unwrap();
        assert_eq!(s.project(p.id).unwrap().script, "@旁白：測試");
    }
}
