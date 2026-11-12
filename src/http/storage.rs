use std::{fs, path::PathBuf, sync::Mutex};

use rusqlite::{params, Connection};

use super::{HttpHistoryRow, HttpTabRow};

const HISTORY_MAX_ROWS: i64 = 500;
const HISTORY_MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;

pub struct HttpStore {
    connection: Mutex<Connection>,
}

impl HttpStore {
    pub fn new(root_directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root_directory)
            .map_err(|error| format!("创建 HTTP 数据目录失败：{error}"))?;
        let database_path = root_directory.join("http.sqlite3");
        let database_existed = database_path
            .metadata()
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false);
        let connection = Connection::open(&database_path)
            .map_err(|error| format!("打开 HTTP 数据库失败：{error}"))?;
        let needs_migration = database_existed
            && (!has_column(&connection, "http_tabs", "kind")?
                || !has_table(&connection, "http_history")?);
        if needs_migration {
            let backup_path = root_directory.join("http.sqlite3.pre-service-v1.bak");
            if !backup_path.exists() {
                connection
                    .execute_batch("PRAGMA wal_checkpoint(FULL)")
                    .map_err(|error| format!("准备 HTTP 数据库备份失败：{error}"))?;
                fs::copy(&database_path, &backup_path)
                    .map_err(|error| format!("备份 HTTP 数据库失败：{error}"))?;
            }
        }
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS http_tabs (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    order_index INTEGER NOT NULL,
                    is_active INTEGER NOT NULL DEFAULT 0,
                    spec_json TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    kind TEXT NOT NULL DEFAULT 'http'
                );

                CREATE TABLE IF NOT EXISTS http_history (
                    id TEXT PRIMARY KEY,
                    method TEXT NOT NULL,
                    url TEXT NOT NULL,
                    status INTEGER,
                    elapsed_ms INTEGER,
                    body_bytes INTEGER,
                    spec_json TEXT NOT NULL,
                    resp_summary TEXT,
                    created_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_http_history_created
                    ON http_history(created_at DESC);
                "#,
            )
            .map_err(|error| format!("初始化 HTTP 数据库失败：{error}"))?;
        ensure_kind_column(&connection)?;
        let integrity: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|error| format!("检查 HTTP 数据库失败：{error}"))?;
        if integrity != "ok" {
            return Err(format!("HTTP 数据库完整性检查失败：{integrity}"));
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn list_tabs(&self) -> Result<Vec<HttpTabRow>, String> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, title, order_index, is_active, spec_json, updated_at, kind \
                 FROM http_tabs ORDER BY order_index ASC, updated_at ASC",
            )
            .map_err(map_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(HttpTabRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    order_index: row.get(2)?,
                    is_active: row.get::<_, i64>(3)? != 0,
                    spec_json: row.get(4)?,
                    updated_at: row.get(5)?,
                    kind: row.get(6)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn upsert_tab(&self, row: HttpTabRow) -> Result<(), String> {
        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO http_tabs \
                   (id, title, order_index, is_active, spec_json, updated_at, kind) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(id) DO UPDATE SET \
                   title=excluded.title, order_index=excluded.order_index, \
                   is_active=excluded.is_active, spec_json=excluded.spec_json, \
                   updated_at=excluded.updated_at, kind=excluded.kind",
                params![
                    row.id,
                    row.title,
                    row.order_index,
                    row.is_active as i64,
                    row.spec_json,
                    row.updated_at,
                    row.kind,
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    pub fn delete_tab(&self, id: &str) -> Result<(), String> {
        self.connection()
            .execute("DELETE FROM http_tabs WHERE id = ?1", params![id])
            .map_err(map_error)?;
        Ok(())
    }

    pub fn set_active_tab(&self, id: &str) -> Result<(), String> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(map_error)?;
        transaction
            .execute("UPDATE http_tabs SET is_active = 0 WHERE is_active = 1", [])
            .map_err(map_error)?;
        transaction
            .execute(
                "UPDATE http_tabs SET is_active = 1 WHERE id = ?1",
                params![id],
            )
            .map_err(map_error)?;
        transaction.commit().map_err(map_error)
    }

    pub fn list_history(&self, limit: u32) -> Result<Vec<HttpHistoryRow>, String> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, method, url, status, elapsed_ms, body_bytes, spec_json, \
                        resp_summary, created_at \
                 FROM http_history ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(map_error)?;
        let rows = statement
            .query_map([limit.min(HISTORY_MAX_ROWS as u32) as i64], |row| {
                Ok(HttpHistoryRow {
                    id: row.get(0)?,
                    method: row.get(1)?,
                    url: row.get(2)?,
                    status: row.get(3)?,
                    elapsed_ms: row.get(4)?,
                    body_bytes: row.get(5)?,
                    spec_json: row.get(6)?,
                    resp_summary: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn insert_history(&self, row: HttpHistoryRow) -> Result<(), String> {
        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO http_history \
                   (id, method, url, status, elapsed_ms, body_bytes, spec_json, resp_summary, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    row.id,
                    row.method,
                    row.url,
                    row.status,
                    row.elapsed_ms,
                    row.body_bytes,
                    row.spec_json,
                    row.resp_summary,
                    row.created_at,
                ],
            )
            .map_err(map_error)?;
        let cutoff = row.created_at - HISTORY_MAX_AGE_MS;
        connection
            .execute(
                "DELETE FROM http_history WHERE created_at < ?1",
                params![cutoff],
            )
            .map_err(map_error)?;
        connection
            .execute(
                "DELETE FROM http_history WHERE id NOT IN \
                 (SELECT id FROM http_history ORDER BY created_at DESC LIMIT ?1)",
                params![HISTORY_MAX_ROWS],
            )
            .map_err(map_error)?;
        Ok(())
    }

    pub fn delete_history(&self, id: &str) -> Result<(), String> {
        self.connection()
            .execute("DELETE FROM http_history WHERE id = ?1", params![id])
            .map_err(map_error)?;
        Ok(())
    }

    pub fn clear_history(&self) -> Result<(), String> {
        self.connection()
            .execute("DELETE FROM http_history", [])
            .map_err(map_error)?;
        Ok(())
    }

    fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection.lock().expect("HTTP store mutex poisoned")
    }
}

fn ensure_kind_column(connection: &Connection) -> Result<(), String> {
    if !has_column(connection, "http_tabs", "kind")? {
        connection
            .execute(
                "ALTER TABLE http_tabs ADD COLUMN kind TEXT NOT NULL DEFAULT 'http'",
                [],
            )
            .map_err(map_error)?;
    }
    Ok(())
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(map_error)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(map_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_error)?;
    Ok(names.iter().any(|name| name == column))
}

fn has_table(connection: &Connection, table: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(map_error)
}

fn map_error(error: rusqlite::Error) -> String {
    format!("HTTP 数据库操作失败：{error}")
}
