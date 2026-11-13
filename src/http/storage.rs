use std::{fs, path::PathBuf, sync::Mutex};

use rusqlite::{params, Connection};

use super::{
    HttpCollectionFolderRow, HttpCollectionRequestRow, HttpCollectionRow, HttpEnvRow,
    HttpEnvVarRow, HttpHistoryRow, HttpTabRow,
};

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
        if database_existed {
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

                CREATE TABLE IF NOT EXISTS http_envs (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    is_active INTEGER NOT NULL DEFAULT 0,
                    order_index INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS http_env_vars (
                    id TEXT PRIMARY KEY,
                    env_id TEXT NOT NULL,
                    key TEXT NOT NULL,
                    value TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    order_index INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_http_env_vars_env ON http_env_vars(env_id);

                CREATE TABLE IF NOT EXISTS http_collections (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    order_index INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    source_url TEXT,
                    source_headers_json TEXT,
                    sync_interval_secs INTEGER,
                    last_synced_at INTEGER,
                    last_sync_error TEXT,
                    base_url TEXT
                );
                CREATE TABLE IF NOT EXISTS http_collection_folders (
                    id TEXT PRIMARY KEY,
                    collection_id TEXT NOT NULL,
                    parent_id TEXT,
                    name TEXT NOT NULL,
                    order_index INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_http_collection_folders_collection
                    ON http_collection_folders(collection_id);
                CREATE TABLE IF NOT EXISTS http_collection_requests (
                    id TEXT PRIMARY KEY,
                    collection_id TEXT NOT NULL,
                    folder_id TEXT,
                    name TEXT NOT NULL,
                    method TEXT NOT NULL,
                    spec_json TEXT NOT NULL,
                    order_index INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    source_key TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_http_collection_requests_collection
                    ON http_collection_requests(collection_id);
                CREATE INDEX IF NOT EXISTS idx_http_collection_requests_folder
                    ON http_collection_requests(folder_id);
                "#,
            )
            .map_err(|error| format!("初始化 HTTP 数据库失败：{error}"))?;
        ensure_kind_column(&connection)?;
        ensure_columns(
            &connection,
            "http_collections",
            &[
                ("source_url", "source_url TEXT"),
                ("source_headers_json", "source_headers_json TEXT"),
                ("sync_interval_secs", "sync_interval_secs INTEGER"),
                ("last_synced_at", "last_synced_at INTEGER"),
                ("last_sync_error", "last_sync_error TEXT"),
                ("base_url", "base_url TEXT"),
            ],
        )?;
        ensure_columns(
            &connection,
            "http_collection_requests",
            &[("source_key", "source_key TEXT")],
        )?;
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

    pub fn list_envs(&self) -> Result<Vec<HttpEnvRow>, String> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, name, is_active, order_index, updated_at FROM http_envs \
                 ORDER BY order_index ASC, updated_at ASC",
            )
            .map_err(map_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(HttpEnvRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    is_active: row.get::<_, i64>(2)? != 0,
                    order_index: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn upsert_env(&self, row: HttpEnvRow) -> Result<(), String> {
        self.connection()
            .execute(
                "INSERT INTO http_envs (id, name, is_active, order_index, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET \
                 name=excluded.name, is_active=excluded.is_active, \
                 order_index=excluded.order_index, updated_at=excluded.updated_at",
                params![
                    row.id,
                    row.name,
                    row.is_active as i64,
                    row.order_index,
                    row.updated_at
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    pub fn delete_env(&self, id: &str) -> Result<(), String> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(map_error)?;
        transaction
            .execute("DELETE FROM http_env_vars WHERE env_id = ?1", params![id])
            .map_err(map_error)?;
        transaction
            .execute("DELETE FROM http_envs WHERE id = ?1", params![id])
            .map_err(map_error)?;
        transaction.commit().map_err(map_error)
    }

    pub fn set_active_env(&self, id: &str) -> Result<(), String> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(map_error)?;
        transaction
            .execute("UPDATE http_envs SET is_active = 0 WHERE is_active = 1", [])
            .map_err(map_error)?;
        transaction
            .execute(
                "UPDATE http_envs SET is_active = 1 WHERE id = ?1",
                params![id],
            )
            .map_err(map_error)?;
        transaction.commit().map_err(map_error)
    }

    pub fn list_env_vars(&self, env_id: &str) -> Result<Vec<HttpEnvVarRow>, String> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, env_id, key, value, enabled, order_index, updated_at \
                 FROM http_env_vars WHERE env_id = ?1 \
                 ORDER BY order_index ASC, updated_at ASC",
            )
            .map_err(map_error)?;
        let rows = statement
            .query_map([env_id], |row| {
                Ok(HttpEnvVarRow {
                    id: row.get(0)?,
                    env_id: row.get(1)?,
                    key: row.get(2)?,
                    value: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                    order_index: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn upsert_env_var(&self, row: HttpEnvVarRow) -> Result<(), String> {
        self.connection()
            .execute(
                "INSERT INTO http_env_vars \
                 (id, env_id, key, value, enabled, order_index, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO UPDATE SET \
                 env_id=excluded.env_id, key=excluded.key, value=excluded.value, \
                 enabled=excluded.enabled, order_index=excluded.order_index, \
                 updated_at=excluded.updated_at",
                params![
                    row.id,
                    row.env_id,
                    row.key,
                    row.value,
                    row.enabled as i64,
                    row.order_index,
                    row.updated_at
                ],
            )
            .map_err(map_error)?;
        Ok(())
    }

    pub fn delete_env_var(&self, id: &str) -> Result<(), String> {
        self.connection()
            .execute("DELETE FROM http_env_vars WHERE id = ?1", params![id])
            .map_err(map_error)?;
        Ok(())
    }

    pub fn list_collections(&self) -> Result<Vec<HttpCollectionRow>, String> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, name, order_index, updated_at, source_url, source_headers_json, \
                    sync_interval_secs, last_synced_at, last_sync_error, base_url \
             FROM http_collections ORDER BY order_index ASC, updated_at ASC",
            )
            .map_err(map_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(HttpCollectionRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    order_index: row.get(2)?,
                    updated_at: row.get(3)?,
                    source_url: row.get(4)?,
                    source_headers_json: row.get(5)?,
                    sync_interval_secs: row.get(6)?,
                    last_synced_at: row.get(7)?,
                    last_sync_error: row.get(8)?,
                    base_url: row.get(9)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn list_collection_folders(&self) -> Result<Vec<HttpCollectionFolderRow>, String> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, collection_id, parent_id, name, order_index, updated_at \
             FROM http_collection_folders ORDER BY collection_id, order_index, updated_at",
            )
            .map_err(map_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(HttpCollectionFolderRow {
                    id: row.get(0)?,
                    collection_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    name: row.get(3)?,
                    order_index: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn list_collection_requests(&self) -> Result<Vec<HttpCollectionRequestRow>, String> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, collection_id, folder_id, name, method, spec_json, order_index, updated_at, source_key \
             FROM http_collection_requests ORDER BY collection_id, folder_id, order_index, updated_at",
        ).map_err(map_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(HttpCollectionRequestRow {
                    id: row.get(0)?,
                    collection_id: row.get(1)?,
                    folder_id: row.get(2)?,
                    name: row.get(3)?,
                    method: row.get(4)?,
                    spec_json: row.get(5)?,
                    order_index: row.get(6)?,
                    updated_at: row.get(7)?,
                    source_key: row.get(8)?,
                })
            })
            .map_err(map_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_error)?;
        Ok(rows)
    }

    pub fn upsert_collection(&self, row: HttpCollectionRow) -> Result<(), String> {
        self.connection().execute(
            "INSERT INTO http_collections (id, name, order_index, updated_at, source_url, source_headers_json, \
             sync_interval_secs, last_synced_at, last_sync_error, base_url) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(id) DO UPDATE SET \
             name=excluded.name, order_index=excluded.order_index, updated_at=excluded.updated_at, \
             source_url=excluded.source_url, source_headers_json=excluded.source_headers_json, \
             sync_interval_secs=excluded.sync_interval_secs, last_synced_at=excluded.last_synced_at, \
             last_sync_error=excluded.last_sync_error, base_url=excluded.base_url",
            params![row.id, row.name, row.order_index, row.updated_at, row.source_url,
                row.source_headers_json, row.sync_interval_secs, row.last_synced_at,
                row.last_sync_error, row.base_url],
        ).map_err(map_error)?;
        Ok(())
    }

    pub fn upsert_collection_folder(&self, row: HttpCollectionFolderRow) -> Result<(), String> {
        self.connection().execute(
            "INSERT INTO http_collection_folders (id, collection_id, parent_id, name, order_index, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET \
             collection_id=excluded.collection_id, parent_id=excluded.parent_id, name=excluded.name, \
             order_index=excluded.order_index, updated_at=excluded.updated_at",
            params![row.id, row.collection_id, row.parent_id, row.name, row.order_index, row.updated_at],
        ).map_err(map_error)?;
        Ok(())
    }

    pub fn upsert_collection_request(&self, row: HttpCollectionRequestRow) -> Result<(), String> {
        self.connection().execute(
            "INSERT INTO http_collection_requests (id, collection_id, folder_id, name, method, spec_json, \
             order_index, updated_at, source_key) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(id) DO UPDATE SET collection_id=excluded.collection_id, folder_id=excluded.folder_id, \
             name=excluded.name, method=excluded.method, spec_json=excluded.spec_json, \
             order_index=excluded.order_index, updated_at=excluded.updated_at, source_key=excluded.source_key",
            params![row.id, row.collection_id, row.folder_id, row.name, row.method, row.spec_json,
                row.order_index, row.updated_at, row.source_key],
        ).map_err(map_error)?;
        Ok(())
    }

    pub fn delete_collection(&self, id: &str) -> Result<(), String> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(map_error)?;
        transaction
            .execute(
                "DELETE FROM http_collection_requests WHERE collection_id = ?1",
                params![id],
            )
            .map_err(map_error)?;
        transaction
            .execute(
                "DELETE FROM http_collection_folders WHERE collection_id = ?1",
                params![id],
            )
            .map_err(map_error)?;
        transaction
            .execute("DELETE FROM http_collections WHERE id = ?1", params![id])
            .map_err(map_error)?;
        transaction.commit().map_err(map_error)
    }

    pub fn delete_collection_request(&self, id: &str) -> Result<(), String> {
        self.connection()
            .execute(
                "DELETE FROM http_collection_requests WHERE id = ?1",
                params![id],
            )
            .map_err(map_error)?;
        Ok(())
    }

    pub fn delete_collection_folder(&self, id: &str) -> Result<(), String> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(map_error)?;
        transaction
            .execute(
                "DELETE FROM http_collection_requests WHERE folder_id = ?1",
                params![id],
            )
            .map_err(map_error)?;
        transaction
            .execute(
                "DELETE FROM http_collection_folders WHERE id = ?1",
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

fn ensure_columns(
    connection: &Connection,
    table: &str,
    columns: &[(&str, &str)],
) -> Result<(), String> {
    for (column, definition) in columns {
        if !has_column(connection, table, column)? {
            connection
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {definition}"), [])
                .map_err(map_error)?;
        }
    }
    Ok(())
}

fn map_error(error: rusqlite::Error) -> String {
    format!("HTTP 数据库操作失败：{error}")
}

#[cfg(test)]
mod tests {
    use super::HttpStore;

    #[test]
    fn backs_up_an_existing_current_schema_on_first_service_takeover() {
        let directory = tempfile::tempdir().unwrap();
        drop(HttpStore::new(directory.path().to_path_buf()).unwrap());
        let backup = directory.path().join("http.sqlite3.pre-service-v1.bak");
        assert!(!backup.exists());

        drop(HttpStore::new(directory.path().to_path_buf()).unwrap());

        assert!(backup.exists());
        assert!(backup.metadata().unwrap().len() > 0);
    }
}
