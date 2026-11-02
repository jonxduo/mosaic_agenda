//! SQLite archive: tokens of devices, and organizations with their
//! authorized devices and administrators. Nothing else is stored.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Org {
    pub devices: Vec<String>,
    pub admins: Vec<String>,
    pub notice_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub device: String,
    pub platform: String,
    pub token: String,
    pub app_id: String,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS devices (
    pubkey TEXT PRIMARY KEY,
    org TEXT NOT NULL,
    platform TEXT NOT NULL,
    token TEXT NOT NULL,
    app_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS orgs (
    id TEXT PRIMARY KEY,
    admins TEXT NOT NULL,
    devices TEXT NOT NULL,
    notice_text TEXT,
    proof TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
";

fn list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn json(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn org(&self, id: &str) -> rusqlite::Result<Option<Org>> {
        self.conn
            .query_row(
                "SELECT admins, devices, notice_text FROM orgs WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Org {
                        admins: list(&r.get::<_, String>(0)?),
                        devices: list(&r.get::<_, String>(1)?),
                        notice_text: r.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn put_org(&self, id: &str, org: &Org, proof: &[String], now: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO orgs (id, admins, devices, notice_text, proof, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET admins = ?2, devices = ?3, notice_text = ?4, proof = ?5, updated_at = ?6",
            params![id, json(&org.admins), json(&org.devices), org.notice_text, json(proof), now],
        )?;
        // Devices no longer in the organization lose their token.
        let mut stmt = self
            .conn
            .prepare("SELECT pubkey FROM devices WHERE org = ?1")?;
        let stale: Vec<String> = stmt
            .query_map(params![id], |r| r.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter(|d| !org.devices.contains(d))
            .collect();
        for d in stale {
            self.conn
                .execute("DELETE FROM devices WHERE pubkey = ?1", params![d])?;
        }
        Ok(())
    }

    pub fn register(
        &self,
        device: &str,
        org: &str,
        platform: &str,
        token: &str,
        app_id: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO devices (pubkey, org, platform, token, app_id, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(pubkey) DO UPDATE SET org = ?2, platform = ?3, token = ?4, app_id = ?5, updated_at = ?6",
            params![device, org, platform, token, app_id, now],
        )?;
        Ok(())
    }

    pub fn unregister(&self, device: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM devices WHERE pubkey = ?1", params![device])?;
        Ok(())
    }

    pub fn target(&self, device: &str, org: &str) -> rusqlite::Result<Option<Target>> {
        self.conn
            .query_row(
                "SELECT platform, token, app_id FROM devices WHERE pubkey = ?1 AND org = ?2",
                params![device, org],
                |r| {
                    Ok(Target {
                        device: device.to_owned(),
                        platform: r.get(0)?,
                        token: r.get(1)?,
                        app_id: r.get(2)?,
                    })
                },
            )
            .optional()
    }

    /// Daily backup: a consistent copy of the database.
    pub fn backup_to(&self, path: &Path) -> rusqlite::Result<()> {
        let _ = std::fs::remove_file(path);
        self.conn
            .execute("VACUUM INTO ?1", params![path.to_string_lossy()])?;
        Ok(())
    }
}
