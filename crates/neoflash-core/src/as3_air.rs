//! Shared ActionScript 3 AIR runtime services: virtual sandboxed filesystem,
//! AIR URI/path resolution, EncryptedLocalStore, application descriptor
//! synthesis, and a bounded relational SQL engine for `flash.data.*`.
//!
//! Browser hosts marshal inputs/outputs and dispatch AS3 events without
//! duplicating path normalization or SQL execution logic.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const MAX_AIR_PATH_BYTES: usize = 4096;
pub const MAX_AIR_FS_ENTRIES: usize = 4096;
pub const MAX_AIR_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_AIR_TOTAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_AIR_ELS_ITEMS: usize = 1024;
pub const MAX_AIR_SQL_ROWS: usize = 32_768;
pub const MAX_AIR_SQL_TABLES: usize = 256;
pub const MAX_AIR_SQL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AirError {
    #[error("SecurityError: {0}")]
    Security(String),
    #[error("IOError: {0}")]
    Io(String),
    #[error("ArgumentError: {0}")]
    Argument(String),
    #[error("RangeError: {0}")]
    Range(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("SQLError({operation}, #{error_id}): {details}")]
pub struct AirSqlError {
    pub operation: String,
    pub details: String,
    pub error_id: i32,
    pub detail_id: i32,
    pub detail_args: Vec<String>,
}

impl AirSqlError {
    pub fn new(operation: &str, details: impl Into<String>, error_id: i32) -> Self {
        Self {
            operation: operation.to_owned(),
            details: details.into(),
            error_id,
            detail_id: 0,
            detail_args: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::json!({
            "operation": self.operation,
            "details": self.details,
            "errorId": self.error_id,
            "detailId": self.detail_id,
            "detailArguments": self.detail_args,
        })
        .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirResourceInit {
    pub path: String,
    #[serde(default)]
    pub data_base64: String,
}

mod encrypted_store;
mod filesystem;
mod network_security;
mod sql;
mod sql_parser;
#[cfg(test)]
mod tests;
mod window_process;

pub use encrypted_store::*;
pub use filesystem::*;
pub use network_security::*;
use sql_parser::SqlConnState;
pub use sql_parser::{SqlColumnDef, SqlUpdateRecord, SqlValue};
pub use window_process::*;

pub struct AirRuntime {
    pub app_id: String,
    pub descriptor_xml: String,
    pub fs: AirFileSystem,
    pub els: AirEncryptedStore,
    pub html: AirHtmlEngine,
    pub net: AirNetworkKernel,
    pub xml_sig: AirXmlSigEngine,
    conns: BTreeMap<u32, SqlConnState>,
    next_conn_id: u32,
    enc_nonce_counter: u64,
}

impl AirRuntime {
    pub fn new(
        project_name: &str,
        width: u32,
        height: u32,
        resources: &[AirResourceInit],
        now_ms: f64,
    ) -> Result<Self, AirError> {
        Ok(Self {
            app_id: synthesize_app_id(project_name),
            descriptor_xml: synthesize_app_descriptor_xml(project_name, width, height),
            fs: AirFileSystem::new(resources, now_ms)?,
            els: AirEncryptedStore::default(),
            html: AirHtmlEngine::default(),
            net: AirNetworkKernel::default(),
            xml_sig: AirXmlSigEngine::default(),
            conns: BTreeMap::new(),
            next_conn_id: 1,
            enc_nonce_counter: 1,
        })
    }
}
