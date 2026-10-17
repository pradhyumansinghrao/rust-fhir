use crate::model::bundle::{Bundle, BundleEntry, BundleResponse, BundleSearch, BundleType};
use crate::model::outcome::OperationOutcome;
use crate::model::resource::{generate_uuid, is_valid_id, StoredResource};
use crate::search::engine::{SearchEngine, SearchResult};
use crate::search::params::SearchParams;
use crate::storage::{extract_search_indices, FhirStorage, StorageError};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{PgPool, Row, SqlitePool};
use std::str::FromStr;

enum DbPool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

pub struct SqlStorage {
    pool: DbPool,
}

impl SqlStorage {
    pub async fn connect(url: &str) -> Result<Self, StorageError> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let pool = PgPool::connect(url)
                .await
                .map_err(|e| StorageError::Internal(format!("Failed to connect to PostgreSQL: {e}")))?;

            let storage = Self {
                pool: DbPool::Postgres(pool),
            };
            storage.init_postgres_schema().await?;
            Ok(storage)
        } else {
            // SQLite connection
            let clean_url = if url.starts_with("sqlite://") {
                url.to_string()
            } else if url.starts_with(':') {
                format!("sqlite://{url}")
            } else {
                format!("sqlite://{url}")
            };

            let options = SqliteConnectOptions::from_str(&clean_url)
                .map_err(|e| StorageError::Internal(format!("Invalid SQLite URL '{clean_url}': {e}")))?
                .create_if_missing(true);

            let pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(options)
                .await
                .map_err(|e| StorageError::Internal(format!("Failed to connect to SQLite: {e}")))?;

            let storage = Self {
                pool: DbPool::Sqlite(pool),
            };
            storage.init_sqlite_schema().await?;
            Ok(storage)
        }
    }

    async fn init_sqlite_schema(&self) -> Result<(), StorageError> {
        let pool = match &self.pool {
            DbPool::Sqlite(p) => p,
            _ => return Ok(()),
        };

        sqlx::query("PRAGMA journal_mode = WAL;")
            .execute(pool)
            .await
            .map_err(|e| StorageError::Internal(format!("Failed to set WAL mode: {e}")))?;

        sqlx::query("PRAGMA synchronous = NORMAL;")
            .execute(pool)
            .await
            .map_err(|e| StorageError::Internal(format!("Failed to set synchronous mode: {e}")))?;

        let ddl = r#"
        CREATE TABLE IF NOT EXISTS fhir_resources (
            resource_type TEXT NOT NULL,
            id TEXT NOT NULL,
            version_id TEXT NOT NULL,
            last_updated TEXT NOT NULL,
            deleted INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL,
            PRIMARY KEY (resource_type, id)
        );

        CREATE TABLE IF NOT EXISTS fhir_resource_history (
            resource_type TEXT NOT NULL,
            id TEXT NOT NULL,
            version_id TEXT NOT NULL,
            last_updated TEXT NOT NULL,
            deleted INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL,
            PRIMARY KEY (resource_type, id, version_id)
        );

        CREATE TABLE IF NOT EXISTS fhir_search_index (
            resource_type TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            param_name TEXT NOT NULL,
            param_value TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_res_type_del ON fhir_resources(resource_type, deleted);
        CREATE INDEX IF NOT EXISTS idx_res_updated ON fhir_resources(last_updated);
        CREATE INDEX IF NOT EXISTS idx_hist_type_id ON fhir_resource_history(resource_type, id, last_updated DESC);
        CREATE INDEX IF NOT EXISTS idx_hist_updated ON fhir_resource_history(last_updated DESC);
        CREATE INDEX IF NOT EXISTS idx_search_param ON fhir_search_index(resource_type, param_name, param_value);
        CREATE INDEX IF NOT EXISTS idx_search_res ON fhir_search_index(resource_type, resource_id);
        "#;

        for statement in ddl.split(';') {
            let stmt = statement.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(format!("Failed to init SQLite schema: {e}")))?;
            }
        }

        Ok(())
    }

    async fn init_postgres_schema(&self) -> Result<(), StorageError> {
        let pool = match &self.pool {
            DbPool::Postgres(p) => p,
            _ => return Ok(()),
        };

        let ddl = r#"
        CREATE TABLE IF NOT EXISTS fhir_resources (
            resource_type VARCHAR(64) NOT NULL,
            id VARCHAR(64) NOT NULL,
            version_id VARCHAR(32) NOT NULL,
            last_updated VARCHAR(64) NOT NULL,
            deleted INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL,
            PRIMARY KEY (resource_type, id)
        );

        CREATE TABLE IF NOT EXISTS fhir_resource_history (
            resource_type VARCHAR(64) NOT NULL,
            id VARCHAR(64) NOT NULL,
            version_id VARCHAR(32) NOT NULL,
            last_updated VARCHAR(64) NOT NULL,
            deleted INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL,
            PRIMARY KEY (resource_type, id, version_id)
        );

        CREATE TABLE IF NOT EXISTS fhir_search_index (
            resource_type VARCHAR(64) NOT NULL,
            resource_id VARCHAR(64) NOT NULL,
            param_name VARCHAR(64) NOT NULL,
            param_value TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_res_type_del ON fhir_resources(resource_type, deleted);
        CREATE INDEX IF NOT EXISTS idx_res_updated ON fhir_resources(last_updated);
        CREATE INDEX IF NOT EXISTS idx_hist_type_id ON fhir_resource_history(resource_type, id, last_updated DESC);
        CREATE INDEX IF NOT EXISTS idx_hist_updated ON fhir_resource_history(last_updated DESC);
        CREATE INDEX IF NOT EXISTS idx_search_param ON fhir_search_index(resource_type, param_name, param_value);
        CREATE INDEX IF NOT EXISTS idx_search_res ON fhir_search_index(resource_type, resource_id);
        "#;

        for statement in ddl.split(';') {
            let stmt = statement.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(format!("Failed to init PostgreSQL schema: {e}")))?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl FhirStorage for SqlStorage {
    async fn create(&self, resource_type: &str, resource: Value) -> Result<StoredResource, StorageError> {
        let id = match resource.get("id").and_then(|i| i.as_str()) {
            Some(existing_id) => {
                if !is_valid_id(existing_id) {
                    return Err(StorageError::BadRequest(format!(
                        "Invalid resource ID '{existing_id}'. ID must match [A-Za-z0-9\\-\\.]{{1,64}}"
                    )));
                }
                existing_id.to_string()
            }
            None => generate_uuid(),
        };

        // Check if active resource exists
        let check_active_sql = "SELECT deleted FROM fhir_resources WHERE resource_type = $1 AND id = $2";
        let is_active_conflict = match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(check_active_sql)
                    .bind(resource_type)
                    .bind(&id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.map(|r| r.get::<i64, _>("deleted") == 0).unwrap_or(false)
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(check_active_sql)
                    .bind(resource_type)
                    .bind(&id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.map(|r| r.get::<i32, _>("deleted") == 0).unwrap_or(false)
            }
        };

        if is_active_conflict {
            return Err(StorageError::Conflict(format!(
                "Resource {resource_type}/{id} already exists"
            )));
        }

        // Determine next version from history count
        let count_sql = "SELECT COUNT(*) as cnt FROM fhir_resource_history WHERE resource_type = $1 AND id = $2";
        let hist_count: i64 = match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(count_sql)
                    .bind(resource_type)
                    .bind(&id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.get("cnt")
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(count_sql)
                    .bind(resource_type)
                    .bind(&id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.get("cnt")
            }
        };

        let version_id = (hist_count + 1).to_string();
        let stored = StoredResource::new(resource_type.to_string(), id.clone(), version_id, resource);
        let data_str = stored.data.to_string();

        self.upsert_active_and_history(&stored, &data_str).await?;
        self.update_search_index(resource_type, &id, &stored.data).await?;

        Ok(stored)
    }

    async fn read(&self, resource_type: &str, id: &str) -> Result<StoredResource, StorageError> {
        let read_sql = "SELECT version_id, last_updated, deleted, data FROM fhir_resources WHERE resource_type = $1 AND id = $2";

        let result = match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(read_sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                row.map(|r| {
                    (
                        r.get::<String, _>("version_id"),
                        r.get::<String, _>("last_updated"),
                        r.get::<i64, _>("deleted") != 0,
                        r.get::<String, _>("data"),
                    )
                })
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(read_sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                row.map(|r| {
                    (
                        r.get::<String, _>("version_id"),
                        r.get::<String, _>("last_updated"),
                        r.get::<i32, _>("deleted") != 0,
                        r.get::<String, _>("data"),
                    )
                })
            }
        };

        match result {
            Some((version_id, last_updated, deleted, data_str)) => {
                if deleted {
                    Err(StorageError::Deleted(format!(
                        "Resource {resource_type}/{id} has been deleted"
                    )))
                } else {
                    let data: Value = serde_json::from_str(&data_str)
                        .map_err(|e| StorageError::Internal(e.to_string()))?;
                    Ok(StoredResource {
                        resource_type: resource_type.to_string(),
                        id: id.to_string(),
                        version_id,
                        last_updated,
                        deleted: false,
                        data,
                    })
                }
            }
            None => {
                // Check if deleted in history
                if self.is_deleted_in_history(resource_type, id).await? {
                    Err(StorageError::Deleted(format!(
                        "Resource {resource_type}/{id} has been deleted"
                    )))
                } else {
                    Err(StorageError::NotFound(format!(
                        "Resource {resource_type}/{id} not found"
                    )))
                }
            }
        }
    }

    async fn vread(&self, resource_type: &str, id: &str, version_id: &str) -> Result<StoredResource, StorageError> {
        let vread_sql = "SELECT last_updated, deleted, data FROM fhir_resource_history WHERE resource_type = $1 AND id = $2 AND version_id = $3";

        let result = match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(vread_sql)
                    .bind(resource_type)
                    .bind(id)
                    .bind(version_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                row.map(|r| {
                    (
                        r.get::<String, _>("last_updated"),
                        r.get::<i64, _>("deleted") != 0,
                        r.get::<String, _>("data"),
                    )
                })
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(vread_sql)
                    .bind(resource_type)
                    .bind(id)
                    .bind(version_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                row.map(|r| {
                    (
                        r.get::<String, _>("last_updated"),
                        r.get::<i32, _>("deleted") != 0,
                        r.get::<String, _>("data"),
                    )
                })
            }
        };

        match result {
            Some((last_updated, deleted, data_str)) => {
                if deleted {
                    Err(StorageError::Deleted(format!(
                        "Resource {resource_type}/{id}/_history/{version_id} has been deleted"
                    )))
                } else {
                    let data: Value = serde_json::from_str(&data_str)
                        .map_err(|e| StorageError::Internal(e.to_string()))?;
                    Ok(StoredResource {
                        resource_type: resource_type.to_string(),
                        id: id.to_string(),
                        version_id: version_id.to_string(),
                        last_updated,
                        deleted: false,
                        data,
                    })
                }
            }
            None => Err(StorageError::NotFound(format!(
                "Resource {resource_type}/{id} with version {version_id} not found"
            ))),
        }
    }

    async fn update(
        &self,
        resource_type: &str,
        id: &str,
        resource: Value,
        if_match: Option<&str>,
    ) -> Result<StoredResource, StorageError> {
        if !is_valid_id(id) {
            return Err(StorageError::BadRequest(format!("Invalid resource ID '{id}'")));
        }

        // Check optimistic lock If-Match
        if let Some(match_header) = if_match {
            let clean_match = match_header.trim().trim_start_matches("W/").trim_matches('"');
            let current = self.read(resource_type, id).await.ok();
            if let Some(existing) = current {
                if existing.version_id != clean_match {
                    return Err(StorageError::PreconditionFailed(format!(
                        "Precondition failed: current version is '{}', but If-Match was '{}'",
                        existing.version_id, clean_match
                    )));
                }
            }
        }

        // Next version
        let count_sql = "SELECT COUNT(*) as cnt FROM fhir_resource_history WHERE resource_type = $1 AND id = $2";
        let hist_count: i64 = match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(count_sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.get("cnt")
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(count_sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.get("cnt")
            }
        };

        let next_version = (hist_count + 1).to_string();
        let stored = StoredResource::new(resource_type.to_string(), id.to_string(), next_version, resource);
        let data_str = stored.data.to_string();

        self.upsert_active_and_history(&stored, &data_str).await?;
        self.update_search_index(resource_type, id, &stored.data).await?;

        Ok(stored)
    }

    async fn delete(&self, resource_type: &str, id: &str) -> Result<(), StorageError> {
        // Ensure active resource exists
        let _current = self.read(resource_type, id).await?;

        let count_sql = "SELECT COUNT(*) as cnt FROM fhir_resource_history WHERE resource_type = $1 AND id = $2";
        let hist_count: i64 = match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(count_sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.get("cnt")
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(count_sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                row.get("cnt")
            }
        };

        let next_version = (hist_count + 1).to_string();
        let tombstone = StoredResource::new_deleted(resource_type.to_string(), id.to_string(), next_version);
        let data_str = tombstone.data.to_string();

        // Mark active resource as deleted and append tombstone to history
        self.upsert_active_and_history(&tombstone, &data_str).await?;

        // Remove from search index
        let del_idx_sql = "DELETE FROM fhir_search_index WHERE resource_type = $1 AND resource_id = $2";
        match &self.pool {
            DbPool::Sqlite(pool) => {
                sqlx::query(del_idx_sql).bind(resource_type).bind(id).execute(pool).await.ok();
            }
            DbPool::Postgres(pool) => {
                sqlx::query(del_idx_sql).bind(resource_type).bind(id).execute(pool).await.ok();
            }
        };

        Ok(())
    }

    async fn history_instance(&self, resource_type: &str, id: &str) -> Result<Vec<StoredResource>, StorageError> {
        let sql = "SELECT version_id, last_updated, deleted, data FROM fhir_resource_history WHERE resource_type = $1 AND id = $2 ORDER BY last_updated DESC";
        self.fetch_history(sql, Some(resource_type), Some(id)).await
    }

    async fn history_type(&self, resource_type: &str) -> Result<Vec<StoredResource>, StorageError> {
        let sql = "SELECT version_id, last_updated, deleted, data, id FROM fhir_resource_history WHERE resource_type = $1 ORDER BY last_updated DESC";
        self.fetch_history(sql, Some(resource_type), None).await
    }

    async fn history_system(&self) -> Result<Vec<StoredResource>, StorageError> {
        let sql = "SELECT resource_type, id, version_id, last_updated, deleted, data FROM fhir_resource_history ORDER BY last_updated DESC";
        self.fetch_history(sql, None, None).await
    }

    async fn search(&self, resource_type: &str, params: &SearchParams) -> Result<SearchResult, StorageError> {
        // Fetch all non-deleted resources of this type
        let sql = "SELECT id, version_id, last_updated, data FROM fhir_resources WHERE resource_type = $1 AND deleted = 0";

        let resources = match &self.pool {
            DbPool::Sqlite(pool) => {
                let rows = sqlx::query(sql)
                    .bind(resource_type)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                rows.into_iter()
                    .filter_map(|r| {
                        let id: String = r.get("id");
                        let version_id: String = r.get("version_id");
                        let last_updated: String = r.get("last_updated");
                        let data_str: String = r.get("data");
                        let data: Value = serde_json::from_str(&data_str).ok()?;
                        Some(StoredResource {
                            resource_type: resource_type.to_string(),
                            id,
                            version_id,
                            last_updated,
                            deleted: false,
                            data,
                        })
                    })
                    .collect::<Vec<_>>()
            }
            DbPool::Postgres(pool) => {
                let rows = sqlx::query(sql)
                    .bind(resource_type)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                rows.into_iter()
                    .filter_map(|r| {
                        let id: String = r.get("id");
                        let version_id: String = r.get("version_id");
                        let last_updated: String = r.get("last_updated");
                        let data_str: String = r.get("data");
                        let data: Value = serde_json::from_str(&data_str).ok()?;
                        Some(StoredResource {
                            resource_type: resource_type.to_string(),
                            id,
                            version_id,
                            last_updated,
                            deleted: false,
                            data,
                        })
                    })
                    .collect::<Vec<_>>()
            }
        };

        Ok(SearchEngine::evaluate(&resources, params))
    }

    async fn process_bundle(&self, bundle: Value) -> Result<Value, StorageError> {
        let bundle_type = bundle
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        let entries = bundle
            .get("entry")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();

        match bundle_type {
            "transaction" => self.execute_transaction(&entries).await,
            "batch" => self.execute_batch(&entries).await,
            _ => Err(StorageError::BadRequest(format!(
                "Unsupported bundle type '{bundle_type}'. Server accepts 'transaction' or 'batch'."
            ))),
        }
    }
}

impl SqlStorage {
    async fn upsert_active_and_history(&self, stored: &StoredResource, data_str: &str) -> Result<(), StorageError> {
        let deleted_val = if stored.deleted { 1 } else { 0 };

        match &self.pool {
            DbPool::Sqlite(pool) => {
                // Upsert into active resources table
                let upsert_sql = r#"
                INSERT INTO fhir_resources (resource_type, id, version_id, last_updated, deleted, data)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT(resource_type, id) DO UPDATE SET
                    version_id = excluded.version_id,
                    last_updated = excluded.last_updated,
                    deleted = excluded.deleted,
                    data = excluded.data;
                "#;

                sqlx::query(upsert_sql)
                    .bind(&stored.resource_type)
                    .bind(&stored.id)
                    .bind(&stored.version_id)
                    .bind(&stored.last_updated)
                    .bind(deleted_val as i64)
                    .bind(data_str)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                // Insert into history table
                let hist_sql = r#"
                INSERT INTO fhir_resource_history (resource_type, id, version_id, last_updated, deleted, data)
                VALUES ($1, $2, $3, $4, $5, $6);
                "#;

                sqlx::query(hist_sql)
                    .bind(&stored.resource_type)
                    .bind(&stored.id)
                    .bind(&stored.version_id)
                    .bind(&stored.last_updated)
                    .bind(deleted_val as i64)
                    .bind(data_str)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
            }
            DbPool::Postgres(pool) => {
                let upsert_sql = r#"
                INSERT INTO fhir_resources (resource_type, id, version_id, last_updated, deleted, data)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT(resource_type, id) DO UPDATE SET
                    version_id = EXCLUDED.version_id,
                    last_updated = EXCLUDED.last_updated,
                    deleted = EXCLUDED.deleted,
                    data = EXCLUDED.data;
                "#;

                sqlx::query(upsert_sql)
                    .bind(&stored.resource_type)
                    .bind(&stored.id)
                    .bind(&stored.version_id)
                    .bind(&stored.last_updated)
                    .bind(deleted_val as i32)
                    .bind(data_str)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                let hist_sql = r#"
                INSERT INTO fhir_resource_history (resource_type, id, version_id, last_updated, deleted, data)
                VALUES ($1, $2, $3, $4, $5, $6);
                "#;

                sqlx::query(hist_sql)
                    .bind(&stored.resource_type)
                    .bind(&stored.id)
                    .bind(&stored.version_id)
                    .bind(&stored.last_updated)
                    .bind(deleted_val as i32)
                    .bind(data_str)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
            }
        }

        Ok(())
    }

    async fn update_search_index(&self, resource_type: &str, id: &str, data: &Value) -> Result<(), StorageError> {
        let indices = extract_search_indices(data);

        let del_sql = "DELETE FROM fhir_search_index WHERE resource_type = $1 AND resource_id = $2";
        let insert_sql = "INSERT INTO fhir_search_index (resource_type, resource_id, param_name, param_value) VALUES ($1, $2, $3, $4)";

        match &self.pool {
            DbPool::Sqlite(pool) => {
                sqlx::query(del_sql)
                    .bind(resource_type)
                    .bind(id)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                for (name, val) in indices {
                    sqlx::query(insert_sql)
                        .bind(resource_type)
                        .bind(id)
                        .bind(name)
                        .bind(val)
                        .execute(pool)
                        .await
                        .map_err(|e| StorageError::Internal(e.to_string()))?;
                }
            }
            DbPool::Postgres(pool) => {
                sqlx::query(del_sql)
                    .bind(resource_type)
                    .bind(id)
                    .execute(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;

                for (name, val) in indices {
                    sqlx::query(insert_sql)
                        .bind(resource_type)
                        .bind(id)
                        .bind(name)
                        .bind(val)
                        .execute(pool)
                        .await
                        .map_err(|e| StorageError::Internal(e.to_string()))?;
                }
            }
        }

        Ok(())
    }

    async fn is_deleted_in_history(&self, resource_type: &str, id: &str) -> Result<bool, StorageError> {
        let sql = "SELECT deleted FROM fhir_resource_history WHERE resource_type = $1 AND id = $2 ORDER BY last_updated DESC LIMIT 1";
        match &self.pool {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                Ok(row.map(|r| r.get::<i64, _>("deleted") != 0).unwrap_or(false))
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(sql)
                    .bind(resource_type)
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| StorageError::Internal(e.to_string()))?;
                Ok(row.map(|r| r.get::<i32, _>("deleted") != 0).unwrap_or(false))
            }
        }
    }

    async fn fetch_history(
        &self,
        sql: &str,
        filter_type: Option<&str>,
        filter_id: Option<&str>,
    ) -> Result<Vec<StoredResource>, StorageError> {
        let mut results = Vec::new();

        match &self.pool {
            DbPool::Sqlite(pool) => {
                let mut query = sqlx::query(sql);
                if let Some(t) = filter_type {
                    query = query.bind(t);
                }
                if let Some(i) = filter_id {
                    query = query.bind(i);
                }

                let rows = query.fetch_all(pool).await.map_err(|e| StorageError::Internal(e.to_string()))?;
                for r in rows {
                    let r_type: String = filter_type.map(|s| s.to_string()).unwrap_or_else(|| r.get("resource_type"));
                    let r_id: String = filter_id.map(|s| s.to_string()).unwrap_or_else(|| r.get("id"));
                    let version_id: String = r.get("version_id");
                    let last_updated: String = r.get("last_updated");
                    let deleted: bool = r.get::<i64, _>("deleted") != 0;
                    let data_str: String = r.get("data");
                    let data: Value = serde_json::from_str(&data_str).unwrap_or(Value::Null);

                    results.push(StoredResource {
                        resource_type: r_type,
                        id: r_id,
                        version_id,
                        last_updated,
                        deleted,
                        data,
                    });
                }
            }
            DbPool::Postgres(pool) => {
                let mut query = sqlx::query(sql);
                if let Some(t) = filter_type {
                    query = query.bind(t);
                }
                if let Some(i) = filter_id {
                    query = query.bind(i);
                }

                let rows = query.fetch_all(pool).await.map_err(|e| StorageError::Internal(e.to_string()))?;
                for r in rows {
                    let r_type: String = filter_type.map(|s| s.to_string()).unwrap_or_else(|| r.get("resource_type"));
                    let r_id: String = filter_id.map(|s| s.to_string()).unwrap_or_else(|| r.get("id"));
                    let version_id: String = r.get("version_id");
                    let last_updated: String = r.get("last_updated");
                    let deleted: bool = r.get::<i32, _>("deleted") != 0;
                    let data_str: String = r.get("data");
                    let data: Value = serde_json::from_str(&data_str).unwrap_or(Value::Null);

                    results.push(StoredResource {
                        resource_type: r_type,
                        id: r_id,
                        version_id,
                        last_updated,
                        deleted,
                        data,
                    });
                }
            }
        }

        Ok(results)
    }

    async fn execute_transaction(&self, entries: &[Value]) -> Result<Value, StorageError> {
        let mut response_entries = Vec::new();

        // Validate all entries before executing
        for entry in entries {
            if entry.get("request").is_none() {
                return Err(StorageError::BadRequest("Transaction entry missing 'request'".to_string()));
            }
        }

        for entry in entries {
            let request = entry.get("request").unwrap();
            let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("").to_uppercase();
            let url = request.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let if_match = request.get("ifMatch").and_then(|m| m.as_str());

            let res_entry = self.execute_entry_operation(&method, url, entry.get("resource").cloned(), if_match).await?;
            response_entries.push(res_entry);
        }

        let resp_bundle = Bundle {
            resource_type: "Bundle".to_string(),
            id: Some(generate_uuid()),
            bundle_type: BundleType::TransactionResponse,
            total: None,
            link: None,
            entry: response_entries,
        };

        Ok(resp_bundle.to_value())
    }

    async fn execute_batch(&self, entries: &[Value]) -> Result<Value, StorageError> {
        let mut response_entries = Vec::new();

        for entry in entries {
            let request = match entry.get("request") {
                Some(r) => r,
                None => {
                    response_entries.push(BundleEntry {
                        full_url: None,
                        resource: Some(OperationOutcome::bad_request("Missing request in entry").to_value()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status: "400 Bad Request".to_string(),
                            location: None,
                            etag: None,
                            last_modified: None,
                            outcome: None,
                        }),
                    });
                    continue;
                }
            };

            let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("").to_uppercase();
            let url = request.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let if_match = request.get("ifMatch").and_then(|m| m.as_str());

            match self.execute_entry_operation(&method, url, entry.get("resource").cloned(), if_match).await {
                Ok(resp_entry) => response_entries.push(resp_entry),
                Err(err) => {
                    let status = format!("{} {}", err.status_code().as_u16(), err.status_code().canonical_reason().unwrap_or(""));
                    response_entries.push(BundleEntry {
                        full_url: None,
                        resource: Some(err.to_outcome().to_value()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status,
                            location: None,
                            etag: None,
                            last_modified: None,
                            outcome: Some(err.to_outcome().to_value()),
                        }),
                    });
                }
            }
        }

        let resp_bundle = Bundle {
            resource_type: "Bundle".to_string(),
            id: Some(generate_uuid()),
            bundle_type: BundleType::BatchResponse,
            total: None,
            link: None,
            entry: response_entries,
        };

        Ok(resp_bundle.to_value())
    }

    async fn execute_entry_operation(
        &self,
        method: &str,
        url: &str,
        resource: Option<Value>,
        if_match: Option<&str>,
    ) -> Result<BundleEntry, StorageError> {
        let trimmed_url = url.trim().trim_start_matches('/');
        let url_parts: Vec<&str> = trimmed_url.split('?').collect();
        let path = url_parts[0];
        let query_str = url_parts.get(1).copied().unwrap_or("");
        let path_segments: Vec<&str> = path.split('/').collect();

        match method {
            "POST" => {
                let resource_type = path_segments.first().ok_or_else(|| {
                    StorageError::BadRequest("Missing resource type in POST url".to_string())
                })?;
                let body = resource.ok_or_else(|| {
                    StorageError::BadRequest("Missing resource body for POST".to_string())
                })?;

                let stored = self.create(resource_type, body).await?;
                let location = format!("{resource_type}/{}/_history/{}", stored.id, stored.version_id);

                Ok(BundleEntry {
                    full_url: Some(location.clone()),
                    resource: Some(stored.to_json()),
                    search: None,
                    request: None,
                    response: Some(BundleResponse {
                        status: "201 Created".to_string(),
                        location: Some(location),
                        etag: Some(stored.etag()),
                        last_modified: Some(stored.last_updated.clone()),
                        outcome: None,
                    }),
                })
            }
            "GET" => {
                if path_segments.len() == 2 {
                    let (res_type, id) = (path_segments[0], path_segments[1]);
                    let stored = self.read(res_type, id).await?;
                    Ok(BundleEntry {
                        full_url: Some(format!("{res_type}/{id}")),
                        resource: Some(stored.to_json()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status: "200 OK".to_string(),
                            location: None,
                            etag: Some(stored.etag()),
                            last_modified: Some(stored.last_updated.clone()),
                            outcome: None,
                        }),
                    })
                } else if path_segments.len() == 1 {
                    let res_type = path_segments[0];
                    let params = SearchParams::parse_query(query_str, 20, 100);
                    let result = self.search(res_type, &params).await?;

                    let entries: Vec<BundleEntry> = result
                        .items
                        .into_iter()
                        .map(|item| BundleEntry {
                            full_url: Some(format!("{res_type}/{}", item.id)),
                            resource: Some(item.to_json()),
                            search: Some(BundleSearch {
                                mode: "match".to_string(),
                            }),
                            request: None,
                            response: None,
                        })
                        .collect();

                    let bundle = Bundle::new_searchset(result.total, entries, Vec::new());
                    Ok(BundleEntry {
                        full_url: None,
                        resource: Some(bundle.to_value()),
                        search: None,
                        request: None,
                        response: Some(BundleResponse {
                            status: "200 OK".to_string(),
                            location: None,
                            etag: None,
                            last_modified: None,
                            outcome: None,
                        }),
                    })
                } else {
                    Err(StorageError::BadRequest(format!("Unsupported GET path: {path}")))
                }
            }
            "PUT" => {
                if path_segments.len() != 2 {
                    return Err(StorageError::BadRequest("PUT url must be [resourceType]/[id]".to_string()));
                }
                let (res_type, id) = (path_segments[0], path_segments[1]);
                let body = resource.ok_or_else(|| {
                    StorageError::BadRequest("Missing resource body for PUT".to_string())
                })?;

                let stored = self.update(res_type, id, body, if_match).await?;
                Ok(BundleEntry {
                    full_url: Some(format!("{res_type}/{id}")),
                    resource: Some(stored.to_json()),
                    search: None,
                    request: None,
                    response: Some(BundleResponse {
                        status: "200 OK".to_string(),
                        location: Some(format!("{res_type}/{id}/_history/{}", stored.version_id)),
                        etag: Some(stored.etag()),
                        last_modified: Some(stored.last_updated.clone()),
                        outcome: None,
                    }),
                })
            }
            "DELETE" => {
                if path_segments.len() != 2 {
                    return Err(StorageError::BadRequest("DELETE url must be [resourceType]/[id]".to_string()));
                }
                let (res_type, id) = (path_segments[0], path_segments[1]);
                self.delete(res_type, id).await?;
                Ok(BundleEntry {
                    full_url: Some(format!("{res_type}/{id}")),
                    resource: None,
                    search: None,
                    request: None,
                    response: Some(BundleResponse {
                        status: "204 No Content".to_string(),
                        location: None,
                        etag: None,
                        last_modified: None,
                        outcome: None,
                    }),
                })
            }
            _ => Err(StorageError::BadRequest(format!("Unsupported method: {method}"))),
        }
    }
}
