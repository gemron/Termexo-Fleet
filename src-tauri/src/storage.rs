use std::path::PathBuf;
use std::sync::Mutex;

use fleet_protocol::{AccountModelSlot, ExecutionRecord, FleetNode, Mission, MissionTask};
use rusqlite::{params, Connection};

const KEYRING_SERVICE: &str = "dev.termexo.fleet";

pub struct FleetDatabase {
    conn: Mutex<Connection>,
}

impl FleetDatabase {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS fleet_nodes (
                id TEXT PRIMARY KEY,
                payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS account_model_slots (
                id TEXT PRIMARY KEY,
                node_id TEXT NOT NULL,
                payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS missions (
                id TEXT PRIMARY KEY,
                payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS mission_tasks (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS execution_records (
                id TEXT PRIMARY KEY,
                mission_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                payload_json TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn list_nodes(&self) -> Result<Vec<FleetNode>, String> {
        self.list_json("SELECT payload_json FROM fleet_nodes ORDER BY rowid ASC")
    }

    pub fn save_node(&self, node: &FleetNode) -> Result<(), String> {
        let json = serde_json::to_string(node).map_err(|e| e.to_string())?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO fleet_nodes (id, payload_json) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json",
            params![node.id, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_node(&self, node_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("DELETE FROM fleet_nodes WHERE id = ?1", params![node_id])
            .map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM account_model_slots WHERE node_id = ?1",
            params![node_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_slots(&self) -> Result<Vec<AccountModelSlot>, String> {
        self.list_json("SELECT payload_json FROM account_model_slots ORDER BY rowid ASC")
    }

    pub fn save_slot(&self, slot: &AccountModelSlot) -> Result<(), String> {
        let json = serde_json::to_string(slot).map_err(|e| e.to_string())?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO account_model_slots (id, node_id, payload_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET node_id = excluded.node_id, payload_json = excluded.payload_json",
            params![slot.id, slot.node_id, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_missions(&self) -> Result<Vec<Mission>, String> {
        self.list_json("SELECT payload_json FROM missions ORDER BY rowid DESC")
    }

    pub fn save_mission(&self, mission: &Mission) -> Result<(), String> {
        let json = serde_json::to_string(mission).map_err(|e| e.to_string())?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO missions (id, payload_json) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json",
            params![mission.id, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_tasks(&self) -> Result<Vec<MissionTask>, String> {
        self.list_json("SELECT payload_json FROM mission_tasks ORDER BY rowid ASC")
    }

    pub fn save_task(&self, task: &MissionTask) -> Result<(), String> {
        let json = serde_json::to_string(task).map_err(|e| e.to_string())?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO mission_tasks (id, mission_id, payload_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET mission_id = excluded.mission_id, payload_json = excluded.payload_json",
            params![task.id, task.mission_id, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_execution_records(&self) -> Result<Vec<ExecutionRecord>, String> {
        self.list_json("SELECT payload_json FROM execution_records ORDER BY rowid DESC LIMIT 200")
    }

    pub fn save_execution_record(&self, record: &ExecutionRecord) -> Result<(), String> {
        let json = serde_json::to_string(record).map_err(|e| e.to_string())?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO execution_records (id, mission_id, task_id, payload_json) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET payload_json = excluded.payload_json",
            params![record.id, record.mission_id, record.task_id, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn read_setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn
            .prepare("SELECT value FROM app_settings WHERE key = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(params![key]).map_err(|e| e.to_string())?;
        if let Some(row) = rows.next().map_err(|e| e.to_string())? {
            Ok(Some(row.get(0).map_err(|e| e.to_string())?))
        } else {
            Ok(None)
        }
    }

    pub fn write_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn list_json<T: serde::de::DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>, String> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for row in rows {
            let text = row.map_err(|e| e.to_string())?;
            let item = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            items.push(item);
        }
        Ok(items)
    }
}

pub fn save_secret(target: &str, secret: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, target).map_err(|e| e.to_string())?;
    entry.set_password(secret).map_err(|e| e.to_string())
}

pub fn load_secret(target: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, target).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub fn delete_secret(target: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, target).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}
