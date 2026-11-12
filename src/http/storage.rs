use std::{fs, path::PathBuf, sync::Mutex};

use rusqlite::{params, Connection};

use super::HttpTabRow;

pub struct HttpStore {
    connection: Mutex<Connection>,
}

impl HttpStore {
    pub fn new(root_directory: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root_directory)
            .map_err(|error| format!("创建 HTTP 数据目录失败：{error}"))?;
        let database_path = root_directory.join("http.sqlite3");
        let connection = Connection::open(&database_path)
            .map_err(|error| format!("打开 HTTP 数据库失败：{error}"))?;
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
                "#,
            )
            .map_err(|error| format!("初始化 HTTP 数据库失败：{error}"))?;
        if !has_column(&connection, "http_tabs", "kind")? {
            let backup_path = root_directory.join("http.sqlite3.pre-service-v1.bak");
            if !backup_path.exists() {
                connection
                    .execute_batch("PRAGMA wal_checkpoint(FULL)")
                    .map_err(|error| format!("准备 HTTP 数据库备份失败：{error}"))?;
                fs::copy(&database_path, &backup_path)
                    .map_err(|error| format!("备份 HTTP 数据库失败：{error}"))?;
            }
        }
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

fn map_error(error: rusqlite::Error) -> String {
    format!("HTTP 数据库操作失败：{error}")
}
