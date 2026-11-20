use super::encrypted_store::{
    SQL_ENC_VFS_MAGIC, SQL_VFS_MAGIC, decrypt_sql_payload, encrypt_sql_payload,
    parse_sql_encryption_key,
};
use super::sql_parser::*;
use super::*;

impl AirRuntime {
    fn encode_db_payload(
        &mut self,
        db: &SqlDatabase,
        encryption_key: Option<&[u8; 16]>,
        operation: &str,
    ) -> Result<Vec<u8>, AirSqlError> {
        let json_bytes =
            serde_json::to_vec(db).map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        if let Some(key) = encryption_key {
            let seed = self.enc_nonce_counter;
            self.enc_nonce_counter = self.enc_nonce_counter.wrapping_add(1);
            Ok(encrypt_sql_payload(key, seed, &json_bytes))
        } else {
            let mut payload = SQL_VFS_MAGIC.to_vec();
            payload.extend(json_bytes);
            Ok(payload)
        }
    }

    fn load_or_create_db(
        &mut self,
        reference_path: Option<&str>,
        mode: &str,
        encryption_key: Option<[u8; 16]>,
        operation: &str,
        now_ms: f64,
    ) -> Result<AttachedDb, AirSqlError> {
        let mode_lc = mode.to_ascii_lowercase();
        if !matches!(mode_lc.as_str(), "create" | "read" | "update") {
            return Err(AirSqlError::new(
                operation,
                format!("Invalid SQLMode '{mode}'."),
                3112,
            ));
        }
        let read_only = mode_lc == "read";
        let Some(raw_path) = reference_path.filter(|s| !s.is_empty() && *s != ":memory:") else {
            return Ok(AttachedDb {
                vfs_path: None,
                read_only,
                encryption_key,
                db: SqlDatabase::default(),
            });
        };
        let vfs_path = normalize_air_path(raw_path)
            .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        let info = self
            .fs
            .info(&vfs_path)
            .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        if info.exists {
            if info.is_directory {
                return Err(AirSqlError::new(
                    operation,
                    format!("Database path '{vfs_path}' is a directory."),
                    3125,
                ));
            }
            if !read_only && !info.writable {
                return Err(AirSqlError::new(
                    operation,
                    format!("Database '{vfs_path}' is read-only; open with SQLMode.READ."),
                    3132,
                ));
            }
            let bytes = self
                .fs
                .read_bytes(&vfs_path)
                .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
            let db = if bytes.is_empty() {
                let empty_db = SqlDatabase::default();
                if !read_only {
                    let payload =
                        self.encode_db_payload(&empty_db, encryption_key.as_ref(), operation)?;
                    self.fs
                        .write_bytes(&vfs_path, &payload, now_ms)
                        .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
                }
                empty_db
            } else if let Some(enc_body) = bytes.strip_prefix(SQL_ENC_VFS_MAGIC) {
                let Some(key) = encryption_key.as_ref() else {
                    return Err(AirSqlError::new(
                        operation,
                        "File is encrypted or is not a database.",
                        3138,
                    ));
                };
                let json_bytes = decrypt_sql_payload(key, enc_body, operation)?;
                serde_json::from_slice::<SqlDatabase>(&json_bytes).map_err(|_| {
                    AirSqlError::new(
                        operation,
                        "File is not a valid NeoFlash AIR SQL database.",
                        3138,
                    )
                })?
            } else if let Some(json_bytes) = bytes.strip_prefix(SQL_VFS_MAGIC) {
                if encryption_key.is_some() {
                    return Err(AirSqlError::new(
                        operation,
                        "Cannot open an unencrypted database with an encryption key.",
                        3138,
                    ));
                }
                serde_json::from_slice::<SqlDatabase>(json_bytes).map_err(|_| {
                    AirSqlError::new(
                        operation,
                        "File is not a valid NeoFlash AIR SQL database.",
                        3138,
                    )
                })?
            } else {
                return Err(AirSqlError::new(
                    operation,
                    "File is encrypted or is not a database.",
                    3138,
                ));
            };
            Ok(AttachedDb {
                vfs_path: Some(vfs_path),
                read_only,
                encryption_key,
                db,
            })
        } else {
            if mode_lc != "create" {
                return Err(AirSqlError::new(
                    operation,
                    format!("Database file '{vfs_path}' does not exist."),
                    3125,
                ));
            }
            let db = SqlDatabase::default();
            let payload = self.encode_db_payload(&db, encryption_key.as_ref(), operation)?;
            self.fs
                .write_bytes(&vfs_path, &payload, now_ms)
                .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
            Ok(AttachedDb {
                vfs_path: Some(vfs_path),
                read_only: false,
                encryption_key,
                db,
            })
        }
    }

    fn persist_conn(
        &mut self,
        conn_id: u32,
        operation: &str,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        let Some(conn) = self.conns.get(&conn_id) else {
            return Ok(());
        };
        if conn.in_transaction {
            return Ok(());
        }
        let targets: Vec<(String, SqlDatabase, Option<[u8; 16]>)> = conn
            .attached
            .values()
            .filter_map(|att| {
                if att.read_only {
                    return None;
                }
                let path = att.vfs_path.as_ref()?.clone();
                Some((path, att.db.clone(), att.encryption_key))
            })
            .collect();
        for (path, db, key) in targets {
            let payload = self.encode_db_payload(&db, key.as_ref(), operation)?;
            self.fs
                .write_bytes(&path, &payload, now_ms)
                .map_err(|e| AirSqlError::new(operation, e.to_string(), 3125))?;
        }
        Ok(())
    }

    fn conn(&self, conn_id: u32, operation: &str) -> Result<&SqlConnState, AirSqlError> {
        self.conns
            .get(&conn_id)
            .filter(|c| c.connected)
            .ok_or_else(|| AirSqlError::new(operation, "SQLConnection is not open.", 3104))
    }

    fn conn_mut(
        &mut self,
        conn_id: u32,
        operation: &str,
    ) -> Result<&mut SqlConnState, AirSqlError> {
        self.conns
            .get_mut(&conn_id)
            .filter(|c| c.connected)
            .ok_or_else(|| AirSqlError::new(operation, "SQLConnection is not open.", 3104))
    }

    pub fn sql_open(
        &mut self,
        reference_path: Option<&str>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        now_ms: f64,
    ) -> Result<u32, AirSqlError> {
        self.sql_open_with_key(
            reference_path,
            open_mode,
            auto_compact,
            page_size,
            None,
            now_ms,
        )
    }

    pub fn sql_open_with_key(
        &mut self,
        reference_path: Option<&str>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        encryption_key: Option<&[u8]>,
        now_ms: f64,
    ) -> Result<u32, AirSqlError> {
        if !matches!(page_size, 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768) {
            return Err(AirSqlError::new(
                "open",
                format!(
                    "Invalid SQL pageSize {page_size}; must be a power of two from 512 to 32768."
                ),
                3113,
            ));
        }
        let parsed_key = parse_sql_encryption_key(encryption_key, "open")?;
        let main_db =
            self.load_or_create_db(reference_path, open_mode, parsed_key, "open", now_ms)?;
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        let mut attached = BTreeMap::new();
        attached.insert("main".to_owned(), main_db);
        self.conns.insert(
            id,
            SqlConnState {
                connected: true,
                auto_compact,
                page_size,
                cache_size: 2000,
                column_name_style: "default".to_owned(),
                last_insert_rowid: 0,
                total_changes: 0,
                in_transaction: false,
                tx_base: None,
                savepoints: Vec::new(),
                sp_counter: 0,
                attached,
            },
        );
        Ok(id)
    }

    pub fn sql_reencrypt(
        &mut self,
        conn_id: u32,
        new_key_bytes: &[u8],
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        let Some(new_key) = parse_sql_encryption_key(Some(new_key_bytes), "reencrypt")? else {
            return Err(AirSqlError::new(
                "reencrypt",
                "newEncryptionKey must be a 16-byte ByteArray.",
                3143,
            ));
        };
        {
            let conn = self.conn_mut(conn_id, "reencrypt")?;
            if conn.in_transaction {
                return Err(AirSqlError::new(
                    "reencrypt",
                    "Cannot reencrypt database while a transaction is active.",
                    3115,
                ));
            }
            let main_att = conn.attached.get_mut("main").ok_or_else(|| {
                AirSqlError::new("reencrypt", "Main database is not attached.", 3104)
            })?;
            if main_att.read_only {
                return Err(AirSqlError::new(
                    "reencrypt",
                    "Cannot reencrypt a read-only database.",
                    3132,
                ));
            }
            if main_att.encryption_key.is_none() {
                return Err(AirSqlError::new(
                    "reencrypt",
                    "Cannot reencrypt a database that was not created with encryption.",
                    3143,
                ));
            }
            main_att.encryption_key = Some(new_key);
        }
        self.persist_conn(conn_id, "reencrypt", now_ms)
    }

    pub fn sql_close(&mut self, conn_id: u32) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "close")?;
        conn.connected = false;
        conn.in_transaction = false;
        conn.tx_base = None;
        conn.savepoints.clear();
        self.conns.remove(&conn_id);
        Ok(())
    }

    pub fn sql_attach(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<&str>,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        self.sql_attach_with_key(conn_id, name, reference_path, None, now_ms)
    }

    pub fn sql_attach_with_key(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<&str>,
        encryption_key: Option<&[u8]>,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err(AirSqlError::new(
                "attach",
                "Database name must not be empty.",
                3115,
            ));
        }
        let key = clean.to_ascii_lowercase();
        if self.conn(conn_id, "attach")?.attached.contains_key(&key) {
            return Err(AirSqlError::new(
                "attach",
                format!("Database '{clean}' is already attached."),
                3115,
            ));
        }
        let parsed_key = parse_sql_encryption_key(encryption_key, "attach")?;
        let att = self.load_or_create_db(reference_path, "create", parsed_key, "attach", now_ms)?;
        self.conn_mut(conn_id, "attach")?.attached.insert(key, att);
        Ok(())
    }

    pub fn sql_detach(&mut self, conn_id: u32, name: &str) -> Result<(), AirSqlError> {
        let key = name.trim().to_ascii_lowercase();
        if key == "main" || key == "temp" {
            return Err(AirSqlError::new(
                "detach",
                format!("Cannot detach built-in database '{name}'."),
                3115,
            ));
        }
        let conn = self.conn_mut(conn_id, "detach")?;
        if conn.in_transaction {
            return Err(AirSqlError::new(
                "detach",
                "Cannot detach a database while a transaction is active.",
                3115,
            ));
        }
        if conn.attached.remove(&key).is_none() {
            return Err(AirSqlError::new(
                "detach",
                format!("No such attached database '{name}'."),
                3115,
            ));
        }
        Ok(())
    }

    pub fn sql_begin(&mut self, conn_id: u32, option: Option<&str>) -> Result<(), AirSqlError> {
        if let Some(opt) = option.filter(|s| !s.is_empty())
            && !matches!(
                opt.to_ascii_lowercase().as_str(),
                "deferred" | "immediate" | "exclusive"
            )
        {
            return Err(AirSqlError::new(
                "begin",
                format!("Invalid SQLTransactionLockType '{opt}'."),
                3115,
            ));
        }
        let conn = self.conn_mut(conn_id, "begin")?;
        if conn.in_transaction {
            return Err(AirSqlError::new(
                "begin",
                "Cannot start a transaction within an active transaction.",
                3106,
            ));
        }
        let snap: BTreeMap<String, SqlDatabase> = conn
            .attached
            .iter()
            .map(|(k, v)| (k.clone(), v.db.clone()))
            .collect();
        conn.in_transaction = true;
        conn.tx_base = Some(snap);
        conn.savepoints.clear();
        Ok(())
    }

    pub fn sql_commit(&mut self, conn_id: u32, now_ms: f64) -> Result<(), AirSqlError> {
        {
            let conn = self.conn_mut(conn_id, "commit")?;
            if !conn.in_transaction {
                return Err(AirSqlError::new(
                    "commit",
                    "Cannot commit when no transaction is active.",
                    3107,
                ));
            }
            conn.in_transaction = false;
            conn.tx_base = None;
            conn.savepoints.clear();
        }
        self.persist_conn(conn_id, "commit", now_ms)
    }

    pub fn sql_rollback(&mut self, conn_id: u32) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "rollback")?;
        if !conn.in_transaction {
            return Err(AirSqlError::new(
                "rollback",
                "Cannot roll back when no transaction is active.",
                3107,
            ));
        }
        if let Some(base) = conn.tx_base.take() {
            for (k, db) in base {
                if let Some(att) = conn.attached.get_mut(&k) {
                    att.db = db;
                }
            }
        }
        conn.in_transaction = false;
        conn.savepoints.clear();
        Ok(())
    }

    pub fn sql_set_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<&str>,
    ) -> Result<String, AirSqlError> {
        let conn = self.conn_mut(conn_id, "setSavepoint")?;
        let sp_name = match name.map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => s.to_owned(),
            None => {
                conn.sp_counter += 1;
                format!("nf_sp_{}", conn.sp_counter)
            }
        };
        let snap: BTreeMap<String, SqlDatabase> = conn
            .attached
            .iter()
            .map(|(k, v)| (k.clone(), v.db.clone()))
            .collect();
        if !conn.in_transaction {
            conn.in_transaction = true;
            conn.tx_base = Some(snap.clone());
        }
        conn.savepoints.push(SavepointSnap {
            name: sp_name.clone(),
            dbs: snap,
        });
        Ok(sp_name)
    }

    pub fn sql_release_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<&str>,
        now_ms: f64,
    ) -> Result<(), AirSqlError> {
        {
            let conn = self.conn_mut(conn_id, "releaseSavepoint")?;
            let idx = match name.map(str::trim).filter(|s| !s.is_empty()) {
                Some(target) => conn
                    .savepoints
                    .iter()
                    .rposition(|s| s.name.eq_ignore_ascii_case(target)),
                None => conn.savepoints.len().checked_sub(1),
            }
            .ok_or_else(|| {
                AirSqlError::new(
                    "releaseSavepoint",
                    "No matching savepoint to release.",
                    3115,
                )
            })?;
            conn.savepoints.truncate(idx);
            if conn.savepoints.is_empty() && conn.tx_base.is_some() {
                // Savepoint-started implicit transaction remains active only if explicit begin ran;
                // SQLite RELEASE of outermost implicit savepoint commits.
            }
        }
        self.persist_conn(conn_id, "releaseSavepoint", now_ms)
    }

    pub fn sql_rollback_to_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<&str>,
    ) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "rollbackToSavepoint")?;
        let idx = match name.map(str::trim).filter(|s| !s.is_empty()) {
            Some(target) => conn
                .savepoints
                .iter()
                .rposition(|s| s.name.eq_ignore_ascii_case(target)),
            None => conn.savepoints.len().checked_sub(1),
        }
        .ok_or_else(|| {
            AirSqlError::new(
                "rollbackToSavepoint",
                "No matching savepoint to roll back to.",
                3115,
            )
        })?;
        let snap = conn.savepoints[idx].dbs.clone();
        conn.savepoints.truncate(idx + 1);
        for (k, db) in snap {
            if let Some(att) = conn.attached.get_mut(&k) {
                att.db = db;
            }
        }
        Ok(())
    }

    pub fn sql_compact(&mut self, conn_id: u32, now_ms: f64) -> Result<(), AirSqlError> {
        let conn = self.conn(conn_id, "compact")?;
        if conn.in_transaction {
            return Err(AirSqlError::new(
                "compact",
                "Cannot compact database while a transaction is active.",
                3115,
            ));
        }
        self.persist_conn(conn_id, "compact", now_ms)
    }

    pub fn sql_analyze(
        &self,
        conn_id: u32,
        _resource_name: Option<&str>,
    ) -> Result<(), AirSqlError> {
        let _ = self.conn(conn_id, "analyze")?;
        Ok(())
    }

    pub fn sql_deanalyze(&self, conn_id: u32) -> Result<(), AirSqlError> {
        let _ = self.conn(conn_id, "deanalyze")?;
        Ok(())
    }

    pub fn sql_state_json(&self, conn_id: u32) -> Result<String, AirSqlError> {
        let conn = self.conn(conn_id, "open")?;
        Ok(serde_json::json!({
            "connected": conn.connected,
            "inTransaction": conn.in_transaction,
            "lastInsertRowID": conn.last_insert_rowid,
            "totalChanges": conn.total_changes,
            "autoCompact": conn.auto_compact,
            "pageSize": conn.page_size,
            "cacheSize": conn.cache_size,
            "columnNameStyle": conn.column_name_style,
        })
        .to_string())
    }

    pub fn sql_set_config(
        &mut self,
        conn_id: u32,
        cache_size: Option<u32>,
        column_name_style: Option<&str>,
    ) -> Result<(), AirSqlError> {
        let conn = self.conn_mut(conn_id, "execute")?;
        if let Some(cs) = cache_size {
            conn.cache_size = cs.max(1);
        }
        if let Some(style) = column_name_style {
            if !matches!(style, "default" | "long" | "short") {
                return Err(AirSqlError::new(
                    "execute",
                    format!("Invalid SQLColumnNameStyle '{style}'."),
                    3115,
                ));
            }
            conn.column_name_style = style.to_owned();
        }
        Ok(())
    }

    pub fn sql_load_schema_json(
        &self,
        conn_id: u32,
        schema_type: Option<&str>,
        name: Option<&str>,
        database: &str,
        include_columns: bool,
    ) -> Result<String, AirSqlError> {
        let conn = self.conn(conn_id, "schema")?;
        let db_key = if database.trim().is_empty() {
            "main".to_owned()
        } else {
            database.trim().to_ascii_lowercase()
        };
        let att = conn.attached.get(&db_key).ok_or_else(|| {
            AirSqlError::new(
                "schema",
                format!("No such attached database '{database}'."),
                3115,
            )
        })?;
        let want_tables =
            schema_type.is_none_or(|t| matches!(t, "table" | "SQLTableSchema" | "all"));
        let want_views = schema_type.is_none_or(|t| matches!(t, "view" | "SQLViewSchema" | "all"));
        let want_indices =
            schema_type.is_none_or(|t| matches!(t, "index" | "SQLIndexSchema" | "all"));
        let want_triggers =
            schema_type.is_none_or(|t| matches!(t, "trigger" | "SQLTriggerSchema" | "all"));
        let name_filter = name.map(str::trim).filter(|s| !s.is_empty());

        let mut tables = Vec::new();
        if want_tables {
            for tbl in att.db.tables.values() {
                if name_filter.is_some_and(|f| !tbl.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                let cols: Vec<serde_json::Value> = if include_columns {
                    tbl.columns
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "dataType": c.data_type,
                                "allowNull": c.allow_null,
                                "primaryKey": c.primary_key,
                                "autoIncrement": c.auto_increment,
                                "defaultCollationType": c.default_collation_type,
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                tables.push(serde_json::json!({
                    "database": db_key,
                    "name": tbl.name,
                    "sql": tbl.sql,
                    "columns": cols,
                }));
            }
        }
        let mut views = Vec::new();
        if want_views {
            for v in att.db.views.values() {
                if name_filter.is_some_and(|f| !v.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                views.push(serde_json::json!({
                    "database": db_key,
                    "name": v.name,
                    "sql": v.sql,
                }));
            }
        }
        let mut indices = Vec::new();
        if want_indices {
            for idx in att.db.indices.values() {
                if name_filter.is_some_and(|f| !idx.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                indices.push(serde_json::json!({
                    "database": db_key,
                    "name": idx.name,
                    "sql": idx.sql,
                    "table": idx.table,
                }));
            }
        }
        let mut triggers = Vec::new();
        if want_triggers {
            for tr in att.db.triggers.values() {
                if name_filter.is_some_and(|f| !tr.name.eq_ignore_ascii_case(f)) {
                    continue;
                }
                triggers.push(serde_json::json!({
                    "database": db_key,
                    "name": tr.name,
                    "sql": tr.sql,
                    "table": tr.table,
                }));
            }
        }
        if name_filter.is_some()
            && tables.is_empty()
            && views.is_empty()
            && indices.is_empty()
            && triggers.is_empty()
        {
            return Err(AirSqlError::new(
                "schema",
                "No schema objects matched the requested filter.",
                3115,
            ));
        }
        Ok(serde_json::json!({
            "tables": tables,
            "views": views,
            "indices": indices,
            "triggers": triggers,
        })
        .to_string())
    }

    pub fn sql_execute_json(
        &mut self,
        conn_id: u32,
        sql_text: &str,
        params_json: &str,
        now_ms: f64,
    ) -> Result<String, AirSqlError> {
        let params: BTreeMap<String, SqlValue> = if params_json.trim().is_empty() {
            BTreeMap::new()
        } else {
            serde_json::from_str(params_json).map_err(|e| {
                AirSqlError::new("execute", format!("Invalid SQL parameters JSON: {e}"), 3115)
            })?
        };
        let toks = tokenize_sql(sql_text)?;
        let mut p = SqlParser::new(&toks);
        while p.eat(&Tok::Semi) {}
        if p.at_end() {
            return Err(AirSqlError::new(
                "execute",
                "SQLStatement.text must not be empty.",
                3115,
            ));
        }
        let head = match p.peek() {
            Some(Tok::Word(w)) => w.to_ascii_uppercase(),
            _ => {
                return Err(AirSqlError::new(
                    "execute",
                    "SQL statement must begin with a command keyword.",
                    3115,
                ));
            }
        };

        let mut columns: Vec<String> = Vec::new();
        let mut rows_out: Vec<Vec<SqlValue>> = Vec::new();
        let mut rows_affected: usize = 0;
        let mut update_events: Vec<SqlUpdateRecord> = Vec::new();
        let mut mutated = false;

        match head.as_str() {
            "BEGIN" => {
                p.pos += 1;
                let lock = if p.eat_kw("DEFERRED") {
                    Some("deferred")
                } else if p.eat_kw("IMMEDIATE") {
                    Some("immediate")
                } else if p.eat_kw("EXCLUSIVE") {
                    Some("exclusive")
                } else {
                    None
                };
                p.eat_kw("TRANSACTION");
                self.sql_begin(conn_id, lock)?;
            }
            "COMMIT" | "END" => {
                p.pos += 1;
                p.eat_kw("TRANSACTION");
                self.sql_commit(conn_id, now_ms)?;
            }
            "ROLLBACK" => {
                p.pos += 1;
                p.eat_kw("TRANSACTION");
                if p.eat_kw("TO") {
                    p.eat_kw("SAVEPOINT");
                    let sp = p.ident()?;
                    self.sql_rollback_to_savepoint(conn_id, Some(&sp))?;
                } else {
                    self.sql_rollback(conn_id)?;
                }
            }
            "SAVEPOINT" => {
                p.pos += 1;
                let sp = p.ident()?;
                self.sql_set_savepoint(conn_id, Some(&sp))?;
            }
            "RELEASE" => {
                p.pos += 1;
                p.eat_kw("SAVEPOINT");
                let sp = p.ident()?;
                self.sql_release_savepoint(conn_id, Some(&sp), now_ms)?;
            }
            "CREATE" => {
                p.pos += 1;
                let unique = p.eat_kw("UNIQUE");
                p.eat_kw("TEMP");
                p.eat_kw("TEMPORARY");
                if p.eat_kw("TABLE") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, tbl_name) = p.qualified_name()?;
                    p.expect(&Tok::LParen)?;
                    let mut col_defs = Vec::new();
                    let mut table_pk_cols = Vec::new();
                    while !p.eat(&Tok::RParen) {
                        if p.eat_kw("CONSTRAINT") {
                            let _ = p.ident()?;
                        }
                        if p.eat_kw("PRIMARY") {
                            p.expect_kw("KEY")?;
                            p.expect(&Tok::LParen)?;
                            loop {
                                table_pk_cols.push(p.ident()?);
                                if !p.eat(&Tok::Comma) {
                                    break;
                                }
                            }
                            p.expect(&Tok::RParen)?;
                        } else if p.eat_kw("UNIQUE") || p.eat_kw("CHECK") || p.eat_kw("FOREIGN") {
                            let mut depth = 0usize;
                            while p.pos < p.toks.len() {
                                if p.eat(&Tok::LParen) {
                                    depth += 1;
                                } else if p.peek() == Some(&Tok::RParen) {
                                    if depth == 0 {
                                        break;
                                    }
                                    p.pos += 1;
                                    depth -= 1;
                                } else if depth == 0 && p.peek() == Some(&Tok::Comma) {
                                    break;
                                } else {
                                    p.pos += 1;
                                }
                            }
                        } else {
                            col_defs.push(p.parse_column_def()?);
                        }
                        if !p.eat(&Tok::Comma) {
                            p.expect(&Tok::RParen)?;
                            break;
                        }
                    }
                    for pk in table_pk_cols {
                        if let Some(c) = col_defs
                            .iter_mut()
                            .find(|c| c.name.eq_ignore_ascii_case(&pk))
                        {
                            c.primary_key = true;
                        }
                    }
                    if col_defs.is_empty() {
                        return Err(AirSqlError::new(
                            "execute",
                            "CREATE TABLE requires at least one column.",
                            3115,
                        ));
                    }
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let key = tbl_name.to_ascii_lowercase();
                    if att.db.tables.contains_key(&key) {
                        if !if_not_exists {
                            return Err(AirSqlError::new(
                                "execute",
                                format!("Table '{tbl_name}' already exists."),
                                3115,
                            ));
                        }
                    } else {
                        if att.db.tables.len() >= MAX_AIR_SQL_TABLES {
                            return Err(AirSqlError::new(
                                "execute",
                                "SQL database table count limit (256) exceeded.",
                                3115,
                            ));
                        }
                        att.db.tables.insert(
                            key,
                            SqlTable {
                                name: tbl_name,
                                sql: sql_text.trim().trim_end_matches(';').to_owned(),
                                columns: col_defs,
                                rows: Vec::new(),
                                next_rowid: 1,
                            },
                        );
                        mutated = true;
                    }
                } else if p.eat_kw("INDEX") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, idx_name) = p.qualified_name()?;
                    p.expect_kw("ON")?;
                    let tbl_name = p.ident()?;
                    p.expect(&Tok::LParen)?;
                    let mut idx_cols = Vec::new();
                    loop {
                        idx_cols.push(p.ident()?);
                        p.eat_kw("COLLATE");
                        if matches!(p.peek(), Some(Tok::Word(w)) if w.eq_ignore_ascii_case("NOCASE") || w.eq_ignore_ascii_case("BINARY"))
                        {
                            p.pos += 1;
                        }
                        p.eat_kw("ASC");
                        p.eat_kw("DESC");
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    p.expect(&Tok::RParen)?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl_key = tbl_name.to_ascii_lowercase();
                    let actual_tbl = att
                        .db
                        .tables
                        .get(&tbl_key)
                        .map(|t| t.name.clone())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let key = idx_name.to_ascii_lowercase();
                    match att.db.indices.entry(key) {
                        std::collections::btree_map::Entry::Occupied(_) => {
                            if !if_not_exists {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!("Index '{idx_name}' already exists."),
                                    3115,
                                ));
                            }
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(SqlIndexMeta {
                                name: idx_name,
                                table: actual_tbl,
                                sql: sql_text.trim().trim_end_matches(';').to_owned(),
                                unique,
                                columns: idx_cols,
                            });
                            mutated = true;
                        }
                    }
                } else if p.eat_kw("VIEW") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, view_name) = p.qualified_name()?;
                    p.expect_kw("AS")?;
                    let upper_src = sql_text.to_ascii_uppercase();
                    let select_sql = if let Some(as_idx) = upper_src.find(" AS ") {
                        sql_text[as_idx + 4..]
                            .trim()
                            .trim_end_matches(';')
                            .to_owned()
                    } else {
                        "SELECT 1".to_owned()
                    };
                    let _ = p.parse_select()?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let key = view_name.to_ascii_lowercase();
                    match att.db.views.entry(key) {
                        std::collections::btree_map::Entry::Occupied(_) => {
                            if !if_not_exists {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!("View '{view_name}' already exists."),
                                    3115,
                                ));
                            }
                        }
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(SqlViewMeta {
                                name: view_name,
                                sql: sql_text.trim().trim_end_matches(';').to_owned(),
                                select_sql,
                            });
                            mutated = true;
                        }
                    }
                } else if p.eat_kw("TRIGGER") {
                    let if_not_exists = if p.eat_kw("IF") {
                        p.expect_kw("NOT")?;
                        p.expect_kw("EXISTS")?;
                        true
                    } else {
                        false
                    };
                    let (db_opt, trig_name) = p.qualified_name()?;
                    while !p.at_end() && !p.eat_kw("ON") {
                        p.pos += 1;
                    }
                    let tbl_name = p.ident().unwrap_or_default();
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let key = trig_name.to_ascii_lowercase();
                    match att.db.triggers.entry(key) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(SqlTriggerMeta {
                                name: trig_name,
                                table: tbl_name,
                                sql: sql_text.trim().to_owned(),
                            });
                            mutated = true;
                        }
                        std::collections::btree_map::Entry::Occupied(_) if !if_not_exists => {
                            return Err(AirSqlError::new(
                                "execute",
                                "Trigger already exists.",
                                3115,
                            ));
                        }
                        std::collections::btree_map::Entry::Occupied(_) => {}
                    }
                } else {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unsupported CREATE target; expected TABLE, INDEX, VIEW or TRIGGER.",
                        3115,
                    ));
                }
            }
            "DROP" => {
                p.pos += 1;
                let kind = p.ident()?.to_ascii_uppercase();
                let if_exists = if p.eat_kw("IF") {
                    p.expect_kw("EXISTS")?;
                    true
                } else {
                    false
                };
                let (db_opt, obj_name) = p.qualified_name()?;
                let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                let key = obj_name.to_ascii_lowercase();
                let removed = match kind.as_str() {
                    "TABLE" => {
                        let existed = att.db.tables.remove(&key);
                        if let Some(t) = &existed {
                            att.db
                                .indices
                                .retain(|_, idx| !idx.table.eq_ignore_ascii_case(&t.name));
                            att.db
                                .triggers
                                .retain(|_, tr| !tr.table.eq_ignore_ascii_case(&t.name));
                        }
                        existed.is_some()
                    }
                    "INDEX" => att.db.indices.remove(&key).is_some(),
                    "VIEW" => att.db.views.remove(&key).is_some(),
                    "TRIGGER" => att.db.triggers.remove(&key).is_some(),
                    _ => {
                        return Err(AirSqlError::new(
                            "execute",
                            format!("Unsupported DROP {kind}."),
                            3115,
                        ));
                    }
                };
                if !removed && !if_exists {
                    return Err(AirSqlError::new(
                        "execute",
                        format!("No such {} '{obj_name}'.", kind.to_ascii_lowercase()),
                        3115,
                    ));
                }
                mutated = removed;
            }
            "ALTER" => {
                p.pos += 1;
                p.expect_kw("TABLE")?;
                let (db_opt, tbl_name) = p.qualified_name()?;
                if p.eat_kw("RENAME") {
                    p.expect_kw("TO")?;
                    let new_name = p.ident()?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let old_key = tbl_name.to_ascii_lowercase();
                    let new_key = new_name.to_ascii_lowercase();
                    if att.db.tables.contains_key(&new_key) {
                        return Err(AirSqlError::new(
                            "execute",
                            format!("Table '{new_name}' already exists."),
                            3115,
                        ));
                    }
                    let mut tbl = att.db.tables.remove(&old_key).ok_or_else(|| {
                        AirSqlError::new("execute", format!("No such table '{tbl_name}'."), 3115)
                    })?;
                    tbl.name = new_name;
                    att.db.tables.insert(new_key, tbl);
                    mutated = true;
                } else if p.eat_kw("ADD") {
                    p.eat_kw("COLUMN");
                    let col = p.parse_column_def()?;
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    if tbl
                        .columns
                        .iter()
                        .any(|c| c.name.eq_ignore_ascii_case(&col.name))
                    {
                        return Err(AirSqlError::new(
                            "execute",
                            format!("Duplicate column name '{}'.", col.name),
                            3115,
                        ));
                    }
                    let def_val = col.default_value.clone().unwrap_or(SqlValue::Null);
                    for row in &mut tbl.rows {
                        row.values.push(def_val.clone());
                    }
                    tbl.columns.push(col);
                    mutated = true;
                } else {
                    return Err(AirSqlError::new(
                        "execute",
                        "Unsupported ALTER TABLE action.",
                        3115,
                    ));
                }
            }
            "INSERT" | "REPLACE" => {
                let mut conflict = if head == "REPLACE" {
                    "replace"
                } else {
                    "abort"
                };
                p.pos += 1;
                if head == "INSERT" && p.eat_kw("OR") {
                    if p.eat_kw("REPLACE") {
                        conflict = "replace";
                    } else if p.eat_kw("IGNORE") {
                        conflict = "ignore";
                    } else {
                        let _ = p.ident()?;
                    }
                }
                p.expect_kw("INTO")?;
                let (db_opt, tbl_name) = p.qualified_name()?;
                let mut target_cols = Vec::new();
                if p.eat(&Tok::LParen) {
                    loop {
                        target_cols.push(p.ident()?);
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    p.expect(&Tok::RParen)?;
                }
                let mut tuples: Vec<Vec<SqlExpr>> = Vec::new();
                if p.eat_kw("DEFAULT") {
                    p.expect_kw("VALUES")?;
                    tuples.push(Vec::new());
                } else {
                    p.expect_kw("VALUES")?;
                    loop {
                        p.expect(&Tok::LParen)?;
                        let mut tuple = Vec::new();
                        if !p.eat(&Tok::RParen) {
                            loop {
                                tuple.push(p.expr()?);
                                if !p.eat(&Tok::Comma) {
                                    break;
                                }
                            }
                            p.expect(&Tok::RParen)?;
                        }
                        tuples.push(tuple);
                        if !p.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }

                let mut last_rowid = self.conn(conn_id, "execute")?.last_insert_rowid;
                {
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let col_indices: Vec<usize> = if target_cols.is_empty() {
                        (0..tbl.columns.len()).collect()
                    } else {
                        let mut idxs = Vec::with_capacity(target_cols.len());
                        for tc in &target_cols {
                            let idx = tbl
                                .columns
                                .iter()
                                .position(|c| c.name.eq_ignore_ascii_case(tc))
                                .ok_or_else(|| {
                                    AirSqlError::new(
                                        "execute",
                                        format!("Table '{}' has no column named '{tc}'.", tbl.name),
                                        3115,
                                    )
                                })?;
                            idxs.push(idx);
                        }
                        idxs
                    };

                    for tuple in tuples {
                        if !tuple.is_empty() && tuple.len() != col_indices.len() {
                            return Err(AirSqlError::new(
                                "execute",
                                format!(
                                    "{} values for {} columns.",
                                    tuple.len(),
                                    col_indices.len()
                                ),
                                3115,
                            ));
                        }
                        if tbl.rows.len() >= MAX_AIR_SQL_ROWS {
                            return Err(AirSqlError::new(
                                "execute",
                                "SQL table row limit (32768) exceeded.",
                                3115,
                            ));
                        }
                        let mut row_vals: Vec<SqlValue> = tbl
                            .columns
                            .iter()
                            .map(|c| c.default_value.clone().unwrap_or(SqlValue::Null))
                            .collect();
                        for (pos, expr) in tuple.iter().enumerate() {
                            let v = eval_sql_expr(expr, &[], None, None, &params, None)?;
                            let cidx = col_indices[pos];
                            row_vals[cidx] = v.coerce_affinity(&tbl.columns[cidx].data_type);
                        }
                        // Integer PK / rowid assignment
                        let mut assigned_rowid = tbl.next_rowid;
                        for (cidx, col) in tbl.columns.iter().enumerate() {
                            if col.primary_key && col.auto_increment {
                                match &row_vals[cidx] {
                                    SqlValue::Null => {
                                        row_vals[cidx] = SqlValue::Int(assigned_rowid);
                                    }
                                    SqlValue::Int(n) => {
                                        assigned_rowid = *n;
                                    }
                                    _ => {}
                                }
                            }
                            if !col.allow_null && row_vals[cidx].is_null() && !col.auto_increment {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!(
                                        "NOT NULL constraint failed: {}.{}",
                                        tbl.name, col.name
                                    ),
                                    3131,
                                ));
                            }
                        }
                        // Check PK / UNIQUE constraint conflicts
                        let mut conflict_rows = Vec::new();
                        for (r_idx, existing) in tbl.rows.iter().enumerate() {
                            let mut clashes = existing.rowid == assigned_rowid;
                            if !clashes {
                                for (cidx, col) in tbl.columns.iter().enumerate() {
                                    if (col.primary_key || col.unique) && !row_vals[cidx].is_null()
                                    {
                                        let nocase = col.default_collation_type == "NO_CASE";
                                        if existing.values[cidx].cmp_sql(&row_vals[cidx], nocase)
                                            == std::cmp::Ordering::Equal
                                        {
                                            clashes = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if clashes {
                                conflict_rows.push(r_idx);
                            }
                        }
                        if !conflict_rows.is_empty() {
                            match conflict {
                                "ignore" => continue,
                                "replace" => {
                                    for r_idx in conflict_rows.into_iter().rev() {
                                        tbl.rows.remove(r_idx);
                                    }
                                }
                                _ => {
                                    return Err(AirSqlError::new(
                                        "execute",
                                        format!(
                                            "UNIQUE constraint failed on table '{}'.",
                                            tbl.name
                                        ),
                                        3131,
                                    ));
                                }
                            }
                        }
                        if assigned_rowid >= tbl.next_rowid {
                            tbl.next_rowid = assigned_rowid + 1;
                        }
                        tbl.rows.push(SqlRow {
                            rowid: assigned_rowid,
                            values: row_vals,
                        });
                        last_rowid = assigned_rowid;
                        rows_affected += 1;
                        update_events.push(SqlUpdateRecord {
                            action: "insert".to_owned(),
                            table: tbl.name.clone(),
                            row_id: assigned_rowid,
                        });
                    }
                }
                if rows_affected > 0 {
                    let conn = self.conn_mut(conn_id, "execute")?;
                    conn.last_insert_rowid = last_rowid;
                    conn.total_changes += rows_affected as u64;
                    mutated = true;
                }
            }
            "UPDATE" => {
                p.pos += 1;
                let (db_opt, tbl_name) = p.qualified_name()?;
                p.expect_kw("SET")?;
                let mut assignments: Vec<(String, SqlExpr)> = Vec::new();
                loop {
                    let col = p.ident()?;
                    p.expect(&Tok::Eq)?;
                    let val_expr = p.expr()?;
                    assignments.push((col, val_expr));
                    if !p.eat(&Tok::Comma) {
                        break;
                    }
                }
                let where_expr = if p.eat_kw("WHERE") {
                    Some(p.expr()?)
                } else {
                    None
                };
                {
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let eval_cols: Vec<EvalColMeta> = tbl
                        .columns
                        .iter()
                        .map(|c| EvalColMeta {
                            table_name: tbl.name.clone(),
                            table_alias: tbl.name.clone(),
                            col_name: c.name.clone(),
                            nocase: c.default_collation_type == "NO_CASE",
                        })
                        .collect();
                    let mut target_idxs = Vec::with_capacity(assignments.len());
                    for (col_name, _) in &assignments {
                        let cidx = tbl
                            .columns
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case(col_name))
                            .ok_or_else(|| {
                                AirSqlError::new(
                                    "execute",
                                    format!("No such column '{col_name}'."),
                                    3115,
                                )
                            })?;
                        target_idxs.push(cidx);
                    }
                    for row in &mut tbl.rows {
                        let erow = EvalRow {
                            rowid: Some(row.rowid),
                            values: row.values.clone(),
                        };
                        if let Some(w) = &where_expr
                            && !eval_sql_expr(w, &eval_cols, Some(&erow), None, &params, None)?
                                .truthy()
                        {
                            continue;
                        }
                        let mut next_vals = row.values.clone();
                        for (pos, (_, expr)) in assignments.iter().enumerate() {
                            let cidx = target_idxs[pos];
                            let v =
                                eval_sql_expr(expr, &eval_cols, Some(&erow), None, &params, None)?;
                            let coerced = v.coerce_affinity(&tbl.columns[cidx].data_type);
                            if !tbl.columns[cidx].allow_null && coerced.is_null() {
                                return Err(AirSqlError::new(
                                    "execute",
                                    format!(
                                        "NOT NULL constraint failed: {}.{}",
                                        tbl.name, tbl.columns[cidx].name
                                    ),
                                    3131,
                                ));
                            }
                            next_vals[cidx] = coerced;
                        }
                        row.values = next_vals;
                        rows_affected += 1;
                        update_events.push(SqlUpdateRecord {
                            action: "update".to_owned(),
                            table: tbl.name.clone(),
                            row_id: row.rowid,
                        });
                    }
                }
                if rows_affected > 0 {
                    self.conn_mut(conn_id, "execute")?.total_changes += rows_affected as u64;
                    mutated = true;
                }
            }
            "DELETE" => {
                p.pos += 1;
                p.expect_kw("FROM")?;
                let (db_opt, tbl_name) = p.qualified_name()?;
                let where_expr = if p.eat_kw("WHERE") {
                    Some(p.expr()?)
                } else {
                    None
                };
                {
                    let att = self.att_mut(conn_id, db_opt.as_deref(), true)?;
                    let tbl = att
                        .db
                        .tables
                        .get_mut(&tbl_name.to_ascii_lowercase())
                        .ok_or_else(|| {
                            AirSqlError::new(
                                "execute",
                                format!("No such table '{tbl_name}'."),
                                3115,
                            )
                        })?;
                    let eval_cols: Vec<EvalColMeta> = tbl
                        .columns
                        .iter()
                        .map(|c| EvalColMeta {
                            table_name: tbl.name.clone(),
                            table_alias: tbl.name.clone(),
                            col_name: c.name.clone(),
                            nocase: c.default_collation_type == "NO_CASE",
                        })
                        .collect();
                    let mut kept = Vec::with_capacity(tbl.rows.len());
                    for row in tbl.rows.drain(..) {
                        let erow = EvalRow {
                            rowid: Some(row.rowid),
                            values: row.values.clone(),
                        };
                        let should_del = match &where_expr {
                            Some(w) => {
                                eval_sql_expr(w, &eval_cols, Some(&erow), None, &params, None)?
                                    .truthy()
                            }
                            None => true,
                        };
                        if should_del {
                            rows_affected += 1;
                            update_events.push(SqlUpdateRecord {
                                action: "delete".to_owned(),
                                table: tbl.name.clone(),
                                row_id: row.rowid,
                            });
                        } else {
                            kept.push(row);
                        }
                    }
                    tbl.rows = kept;
                }
                if rows_affected > 0 {
                    self.conn_mut(conn_id, "execute")?.total_changes += rows_affected as u64;
                    mutated = true;
                }
            }
            "SELECT" => {
                let plan = p.parse_select()?;
                let (cols, rows) = self.eval_select_plan(conn_id, &plan, &params, 0)?;
                columns = cols;
                rows_out = rows;
            }
            other => {
                return Err(AirSqlError::new(
                    "execute",
                    format!("Unsupported SQL command '{other}'."),
                    3115,
                ));
            }
        }

        if mutated {
            self.persist_conn(conn_id, "execute", now_ms)?;
        }
        let last_rowid = self.conns.get(&conn_id).map_or(0, |c| c.last_insert_rowid);
        Ok(serde_json::json!({
            "columns": columns,
            "rows": rows_out,
            "rowsAffected": rows_affected,
            "lastInsertRowID": last_rowid,
            "updateEvents": update_events,
        })
        .to_string())
    }

    fn att_ref(&self, conn_id: u32, db_opt: Option<&str>) -> Result<&AttachedDb, AirSqlError> {
        let conn = self.conn(conn_id, "execute")?;
        let key = db_opt
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "main".to_owned());
        conn.attached
            .get(&key)
            .ok_or_else(|| AirSqlError::new("execute", format!("No such database '{key}'."), 3115))
    }

    fn att_mut(
        &mut self,
        conn_id: u32,
        db_opt: Option<&str>,
        write: bool,
    ) -> Result<&mut AttachedDb, AirSqlError> {
        let conn = self.conn_mut(conn_id, "execute")?;
        let key = db_opt
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "main".to_owned());
        let att = conn.attached.get_mut(&key).ok_or_else(|| {
            AirSqlError::new("execute", format!("No such database '{key}'."), 3115)
        })?;
        if write && att.read_only {
            return Err(AirSqlError::new(
                "execute",
                "Attempt to write a readonly database.",
                3132,
            ));
        }
        Ok(att)
    }

    fn load_table_or_view(
        &self,
        conn_id: u32,
        db_opt: Option<&str>,
        name: &str,
        alias_opt: Option<&str>,
        params: &BTreeMap<String, SqlValue>,
        depth: usize,
    ) -> Result<(Vec<EvalColMeta>, Vec<EvalRow>), AirSqlError> {
        if depth > 8 {
            return Err(AirSqlError::new(
                "execute",
                "Recursive SQL view depth exceeded.",
                3115,
            ));
        }
        let att = self.att_ref(conn_id, db_opt)?;
        let key = name.to_ascii_lowercase();
        if let Some(tbl) = att.db.tables.get(&key) {
            let alias = alias_opt.unwrap_or(&tbl.name).to_owned();
            let cols = tbl
                .columns
                .iter()
                .map(|c| EvalColMeta {
                    table_name: tbl.name.clone(),
                    table_alias: alias.clone(),
                    col_name: c.name.clone(),
                    nocase: c.default_collation_type == "NO_CASE",
                })
                .collect();
            let rows = tbl
                .rows
                .iter()
                .map(|r| EvalRow {
                    rowid: Some(r.rowid),
                    values: r.values.clone(),
                })
                .collect();
            return Ok((cols, rows));
        }
        if let Some(view) = att.db.views.get(&key) {
            let vtoks = tokenize_sql(&view.select_sql)?;
            let mut vp = SqlParser::new(&vtoks);
            let vplan = vp.parse_select()?;
            let (vcols, vrows) = self.eval_select_plan(conn_id, &vplan, params, depth + 1)?;
            let alias = alias_opt.unwrap_or(&view.name).to_owned();
            let cols = vcols
                .into_iter()
                .map(|cn| EvalColMeta {
                    table_name: view.name.clone(),
                    table_alias: alias.clone(),
                    col_name: cn,
                    nocase: false,
                })
                .collect();
            let rows = vrows
                .into_iter()
                .map(|vals| EvalRow {
                    rowid: None,
                    values: vals,
                })
                .collect();
            return Ok((cols, rows));
        }
        Err(AirSqlError::new(
            "execute",
            format!("No such table '{name}'."),
            3115,
        ))
    }

    fn eval_select_plan(
        &self,
        conn_id: u32,
        plan: &SelectPlan,
        params: &BTreeMap<String, SqlValue>,
        depth: usize,
    ) -> Result<(Vec<String>, Vec<Vec<SqlValue>>), AirSqlError> {
        let long_style = self.conn(conn_id, "execute")?.column_name_style == "long";
        let (mut working_cols, mut working_rows) = if let Some(from_tbl) = &plan.from_table {
            self.load_table_or_view(
                conn_id,
                plan.from_db.as_deref(),
                from_tbl,
                plan.from_alias.as_deref(),
                params,
                depth,
            )?
        } else {
            (
                Vec::new(),
                vec![EvalRow {
                    rowid: None,
                    values: Vec::new(),
                }],
            )
        };

        for join in &plan.joins {
            let (rcols, rrows) = self.load_table_or_view(
                conn_id,
                join.db.as_deref(),
                &join.table,
                join.alias.as_deref(),
                params,
                depth,
            )?;
            let r_width = rcols.len();
            let mut combined_cols = working_cols.clone();
            combined_cols.extend(rcols);
            let mut next_rows = Vec::new();
            for lrow in &working_rows {
                let mut matched_any = false;
                for rrow in &rrows {
                    let mut vals = lrow.values.clone();
                    vals.extend(rrow.values.clone());
                    let candidate = EvalRow {
                        rowid: lrow.rowid,
                        values: vals,
                    };
                    let ok = match &join.on {
                        Some(on_expr) => eval_sql_expr(
                            on_expr,
                            &combined_cols,
                            Some(&candidate),
                            None,
                            params,
                            None,
                        )?
                        .truthy(),
                        None => true,
                    };
                    if ok {
                        matched_any = true;
                        next_rows.push(candidate);
                    }
                }
                if !matched_any && join.left_outer {
                    let mut vals = lrow.values.clone();
                    vals.extend(std::iter::repeat_n(SqlValue::Null, r_width));
                    next_rows.push(EvalRow {
                        rowid: lrow.rowid,
                        values: vals,
                    });
                }
            }
            working_cols = combined_cols;
            working_rows = next_rows;
        }

        if let Some(w) = &plan.where_expr {
            let mut filtered = Vec::with_capacity(working_rows.len());
            for row in working_rows {
                if eval_sql_expr(w, &working_cols, Some(&row), None, params, None)?.truthy() {
                    filtered.push(row);
                }
            }
            working_rows = filtered;
        }

        // Expand projection items (`*`, `alias.*`, expressions)
        let mut proj_names: Vec<String> = Vec::new();
        let mut proj_exprs: Vec<SqlExpr> = Vec::new();
        for item in &plan.items {
            match item {
                SelectItem::All(tbl_filter) => {
                    for c in &working_cols {
                        if let Some(tf) = tbl_filter
                            && !c.table_alias.eq_ignore_ascii_case(tf)
                            && !c.table_name.eq_ignore_ascii_case(tf)
                        {
                            continue;
                        }
                        let out_name = if long_style {
                            format!("{}_{}", c.table_alias, c.col_name)
                        } else {
                            c.col_name.clone()
                        };
                        proj_names.push(out_name);
                        proj_exprs.push(SqlExpr::Col {
                            table: Some(c.table_alias.clone()),
                            name: c.col_name.clone(),
                        });
                    }
                }
                SelectItem::Expr { expr, alias } => {
                    let out_name = alias
                        .clone()
                        .unwrap_or_else(|| default_expr_column_name(expr, long_style));
                    proj_names.push(out_name);
                    proj_exprs.push(expr.clone());
                }
            }
        }

        let is_agg = !plan.group_by.is_empty()
            || plan.having.is_some()
            || proj_exprs.iter().any(expr_contains_aggregate);

        let mut projected_with_ctx: Vec<ProjectedRowCtx> = Vec::new();

        if is_agg {
            let groups: Vec<Vec<EvalRow>> = if plan.group_by.is_empty() {
                vec![working_rows]
            } else {
                let mut buckets: Vec<(Vec<SqlValue>, Vec<EvalRow>)> = Vec::new();
                for row in working_rows {
                    let mut key = Vec::with_capacity(plan.group_by.len());
                    for gexpr in &plan.group_by {
                        key.push(eval_sql_expr(
                            gexpr,
                            &working_cols,
                            Some(&row),
                            None,
                            params,
                            None,
                        )?);
                    }
                    if let Some((_, bucket)) = buckets.iter_mut().find(|(k, _)| {
                        k.len() == key.len()
                            && k.iter()
                                .zip(&key)
                                .all(|(a, b)| a.cmp_sql(b, false) == std::cmp::Ordering::Equal)
                    }) {
                        bucket.push(row);
                    } else {
                        buckets.push((key, vec![row]));
                    }
                }
                buckets.into_iter().map(|(_, b)| b).collect()
            };

            for grp in groups {
                let mut pvals = Vec::with_capacity(proj_exprs.len());
                for pexpr in &proj_exprs {
                    pvals.push(eval_sql_expr(
                        pexpr,
                        &working_cols,
                        grp.first(),
                        Some(&grp),
                        params,
                        None,
                    )?);
                }
                if let Some(h) = &plan.having {
                    let alias_pairs: Vec<(&str, SqlValue)> = proj_names
                        .iter()
                        .zip(&pvals)
                        .map(|(n, v)| (n.as_str(), v.clone()))
                        .collect();
                    if !eval_sql_expr(
                        h,
                        &working_cols,
                        grp.first(),
                        Some(&grp),
                        params,
                        Some(&alias_pairs),
                    )?
                    .truthy()
                    {
                        continue;
                    }
                }
                projected_with_ctx.push((pvals, None, Some(grp)));
            }
        } else {
            for row in working_rows {
                let mut pvals = Vec::with_capacity(proj_exprs.len());
                for pexpr in &proj_exprs {
                    pvals.push(eval_sql_expr(
                        pexpr,
                        &working_cols,
                        Some(&row),
                        None,
                        params,
                        None,
                    )?);
                }
                projected_with_ctx.push((pvals, Some(row), None));
            }
        }

        if plan.distinct {
            let mut unique_rows: Vec<ProjectedRowCtx> = Vec::new();
            for item in projected_with_ctx {
                if !unique_rows.iter().any(|(existing, _, _)| {
                    existing.len() == item.0.len()
                        && existing
                            .iter()
                            .zip(&item.0)
                            .all(|(a, b)| a.cmp_sql(b, false) == std::cmp::Ordering::Equal)
                }) {
                    unique_rows.push(item);
                }
            }
            projected_with_ctx = unique_rows;
        }

        if !plan.order_by.is_empty() {
            let mut keyed: Vec<OrderKeyedRow> = Vec::with_capacity(projected_with_ctx.len());
            for (pvals, srow, sgrp) in projected_with_ctx {
                let alias_pairs: Vec<(&str, SqlValue)> = proj_names
                    .iter()
                    .zip(&pvals)
                    .map(|(n, v)| (n.as_str(), v.clone()))
                    .collect();
                let mut keys = Vec::with_capacity(plan.order_by.len());
                for ob in &plan.order_by {
                    let kv = if let SqlExpr::Lit(SqlValue::Int(pos_1)) = &ob.expr {
                        let idx = (*pos_1 - 1) as usize;
                        pvals.get(idx).cloned().unwrap_or(SqlValue::Null)
                    } else {
                        eval_sql_expr(
                            &ob.expr,
                            &working_cols,
                            srow.as_ref()
                                .or_else(|| sgrp.as_ref().and_then(|g| g.first())),
                            sgrp.as_deref(),
                            params,
                            Some(&alias_pairs),
                        )?
                    };
                    keys.push((kv, ob.desc));
                }
                keyed.push((keys, pvals));
            }
            keyed.sort_by(|(ka, _), (kb, _)| {
                for ((va, desc), (vb, _)) in ka.iter().zip(kb.iter()) {
                    let ord = va.cmp_sql(vb, false);
                    if ord != std::cmp::Ordering::Equal {
                        return if *desc { ord.reverse() } else { ord };
                    }
                }
                std::cmp::Ordering::Equal
            });
            let mut all_rows: Vec<Vec<SqlValue>> = keyed.into_iter().map(|(_, p)| p).collect();
            if plan.offset > 0 {
                if plan.offset >= all_rows.len() {
                    all_rows.clear();
                } else {
                    all_rows.drain(0..plan.offset);
                }
            }
            if let Some(lim) = plan.limit {
                all_rows.truncate(lim);
            }
            return Ok((proj_names, all_rows));
        }

        let mut all_rows: Vec<Vec<SqlValue>> =
            projected_with_ctx.into_iter().map(|(p, _, _)| p).collect();
        if plan.offset > 0 {
            if plan.offset >= all_rows.len() {
                all_rows.clear();
            } else {
                all_rows.drain(0..plan.offset);
            }
        }
        if let Some(lim) = plan.limit {
            all_rows.truncate(lim);
        }
        Ok((proj_names, all_rows))
    }
}
