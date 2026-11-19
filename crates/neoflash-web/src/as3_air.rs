//! WASM bindings for the shared Rust AIR runtime (`AirKernel`).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct AirKernel {
    inner: neoflash_core::as3_air::AirRuntime,
}

fn air_js_err(err: neoflash_core::as3_air::AirError) -> JsValue {
    JsValue::from_str(&err.to_string())
}

fn air_sql_js_err(err: neoflash_core::as3_air::AirSqlError) -> JsValue {
    JsValue::from_str(&err.to_json())
}

#[wasm_bindgen]
impl AirKernel {
    #[wasm_bindgen(constructor)]
    pub fn new(
        project_name: &str,
        width: u32,
        height: u32,
        resources_json: &str,
        now_ms: f64,
    ) -> Result<AirKernel, JsValue> {
        let resources: Vec<neoflash_core::as3_air::AirResourceInit> =
            if resources_json.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(resources_json)
                    .map_err(|e| JsValue::from_str(&format!("Invalid AIR resources JSON: {e}")))?
            };
        let inner = neoflash_core::as3_air::AirRuntime::new(
            project_name,
            width,
            height,
            &resources,
            now_ms,
        )
        .map_err(air_js_err)?;
        Ok(Self { inner })
    }

    pub fn app_id(&self) -> String {
        self.inner.app_id.clone()
    }

    pub fn descriptor_xml(&self) -> String {
        self.inner.descriptor_xml.clone()
    }

    pub fn normalize_path(&self, input: &str) -> Result<String, JsValue> {
        neoflash_core::as3_air::normalize_air_path(input).map_err(air_js_err)
    }

    pub fn resolve_path(&self, base_path: &str, relative: &str) -> Result<String, JsValue> {
        neoflash_core::as3_air::resolve_air_path(base_path, relative).map_err(air_js_err)
    }

    pub fn relative_path(
        &self,
        from_path: &str,
        to_path: &str,
        use_dot_dot: bool,
    ) -> Result<Option<String>, JsValue> {
        neoflash_core::as3_air::relative_air_path(from_path, to_path, use_dot_dot)
            .map_err(air_js_err)
    }

    pub fn canonicalize_path(&self, path: &str) -> Result<String, JsValue> {
        self.inner.fs.canonicalize(path).map_err(air_js_err)
    }

    pub fn path_info_json(&self, path: &str) -> Result<String, JsValue> {
        let info = self.inner.fs.info(path).map_err(air_js_err)?;
        serde_json::to_string(&info).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn fs_create_directory(&mut self, path: &str, now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .fs
            .create_directory(path, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_create_temp(&mut self, is_directory: bool, now_ms: f64) -> Result<String, JsValue> {
        self.inner
            .fs
            .create_temp(is_directory, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_list_directory_json(&self, path: &str) -> Result<String, JsValue> {
        let items = self.inner.fs.list_directory(path).map_err(air_js_err)?;
        serde_json::to_string(&items).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn fs_delete_file(&mut self, path: &str) -> Result<(), JsValue> {
        self.inner.fs.delete_file(path).map_err(air_js_err)
    }

    pub fn fs_delete_directory(
        &mut self,
        path: &str,
        delete_contents: bool,
    ) -> Result<(), JsValue> {
        self.inner
            .fs
            .delete_directory(path, delete_contents)
            .map_err(air_js_err)
    }

    pub fn fs_copy(
        &mut self,
        src: &str,
        dst: &str,
        overwrite: bool,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .fs
            .copy_to(src, dst, overwrite, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_move(
        &mut self,
        src: &str,
        dst: &str,
        overwrite: bool,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .fs
            .move_to(src, dst, overwrite, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_open_stream(
        &mut self,
        path: &str,
        mode: &str,
        now_ms: f64,
    ) -> Result<Vec<u8>, JsValue> {
        self.inner
            .fs
            .open_stream(path, mode, now_ms)
            .map_err(air_js_err)
    }

    pub fn fs_write_bytes(&mut self, path: &str, data: &[u8], now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .fs
            .write_bytes(path, data, now_ms)
            .map_err(air_js_err)
    }

    pub fn els_set(&mut self, name: &str, bytes: &[u8]) -> Result<(), JsValue> {
        self.inner.els.set_item(name, bytes).map_err(air_js_err)
    }

    pub fn els_get(&self, name: &str) -> Result<Option<Vec<u8>>, JsValue> {
        self.inner.els.get_item(name).map_err(air_js_err)
    }

    pub fn els_remove(&mut self, name: &str) -> Result<(), JsValue> {
        self.inner.els.remove_item(name).map_err(air_js_err)
    }

    pub fn els_reset(&mut self) {
        self.inner.els.reset();
    }

    pub fn sql_open(
        &mut self,
        reference_path: Option<String>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        now_ms: f64,
    ) -> Result<u32, JsValue> {
        self.inner
            .sql_open(
                reference_path.as_deref(),
                open_mode,
                auto_compact,
                page_size,
                now_ms,
            )
            .map_err(air_sql_js_err)
    }

    pub fn sql_close(&mut self, conn_id: u32) -> Result<(), JsValue> {
        self.inner.sql_close(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_attach(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<String>,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_attach(conn_id, name, reference_path.as_deref(), now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_detach(&mut self, conn_id: u32, name: &str) -> Result<(), JsValue> {
        self.inner.sql_detach(conn_id, name).map_err(air_sql_js_err)
    }

    pub fn sql_begin(&mut self, conn_id: u32, option: Option<String>) -> Result<(), JsValue> {
        self.inner
            .sql_begin(conn_id, option.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_commit(&mut self, conn_id: u32, now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .sql_commit(conn_id, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_rollback(&mut self, conn_id: u32) -> Result<(), JsValue> {
        self.inner.sql_rollback(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_set_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<String>,
    ) -> Result<String, JsValue> {
        self.inner
            .sql_set_savepoint(conn_id, name.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_release_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<String>,
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_release_savepoint(conn_id, name.as_deref(), now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_rollback_to_savepoint(
        &mut self,
        conn_id: u32,
        name: Option<String>,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_rollback_to_savepoint(conn_id, name.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_compact(&mut self, conn_id: u32, now_ms: f64) -> Result<(), JsValue> {
        self.inner
            .sql_compact(conn_id, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_analyze(&self, conn_id: u32, resource_name: Option<String>) -> Result<(), JsValue> {
        self.inner
            .sql_analyze(conn_id, resource_name.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_deanalyze(&self, conn_id: u32) -> Result<(), JsValue> {
        self.inner.sql_deanalyze(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_state_json(&self, conn_id: u32) -> Result<String, JsValue> {
        self.inner.sql_state_json(conn_id).map_err(air_sql_js_err)
    }

    pub fn sql_set_config(
        &mut self,
        conn_id: u32,
        cache_size: Option<u32>,
        column_name_style: Option<String>,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_set_config(conn_id, cache_size, column_name_style.as_deref())
            .map_err(air_sql_js_err)
    }

    pub fn sql_load_schema_json(
        &self,
        conn_id: u32,
        schema_type: Option<String>,
        name: Option<String>,
        database: &str,
        include_columns: bool,
    ) -> Result<String, JsValue> {
        self.inner
            .sql_load_schema_json(
                conn_id,
                schema_type.as_deref(),
                name.as_deref(),
                database,
                include_columns,
            )
            .map_err(air_sql_js_err)
    }

    pub fn sql_execute_json(
        &mut self,
        conn_id: u32,
        sql_text: &str,
        params_json: &str,
        now_ms: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .sql_execute_json(conn_id, sql_text, params_json, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_open_encrypted(
        &mut self,
        reference_path: Option<String>,
        open_mode: &str,
        auto_compact: bool,
        page_size: u32,
        encryption_key: &[u8],
        now_ms: f64,
    ) -> Result<u32, JsValue> {
        let key_opt = if encryption_key.is_empty() {
            None
        } else {
            Some(encryption_key)
        };
        self.inner
            .sql_open_with_key(
                reference_path.as_deref(),
                open_mode,
                auto_compact,
                page_size,
                key_opt,
                now_ms,
            )
            .map_err(air_sql_js_err)
    }

    pub fn sql_attach_encrypted(
        &mut self,
        conn_id: u32,
        name: &str,
        reference_path: Option<String>,
        encryption_key: &[u8],
        now_ms: f64,
    ) -> Result<(), JsValue> {
        let key_opt = if encryption_key.is_empty() {
            None
        } else {
            Some(encryption_key)
        };
        self.inner
            .sql_attach_with_key(conn_id, name, reference_path.as_deref(), key_opt, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn sql_reencrypt(
        &mut self,
        conn_id: u32,
        new_encryption_key: &[u8],
        now_ms: f64,
    ) -> Result<(), JsValue> {
        self.inner
            .sql_reencrypt(conn_id, new_encryption_key, now_ms)
            .map_err(air_sql_js_err)
    }

    pub fn html_create(&mut self, width: f64, height: f64) -> u32 {
        self.inner.html.create_session(width, height)
    }

    pub fn html_dispose(&mut self, id: u32) {
        self.inner.html.dispose_session(id);
    }

    pub fn html_load_string(
        &mut self,
        id: u32,
        html: &str,
        base_url: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .html
            .load_string(id, html, base_url)
            .map_err(air_js_err)
    }

    pub fn html_load_url(
        &mut self,
        id: u32,
        url: &str,
        is_post: bool,
        fallback_html: Option<String>,
    ) -> Result<String, JsValue> {
        self.inner
            .html_load_url(id, url, is_post, fallback_html.as_deref())
            .map_err(air_js_err)
    }

    pub fn html_history_go(&mut self, id: u32, steps: i32) -> Result<String, JsValue> {
        self.inner.html.history_go(id, steps).map_err(air_js_err)
    }

    pub fn html_reload(&mut self, id: u32) -> Result<String, JsValue> {
        self.inner.html.reload(id).map_err(air_js_err)
    }

    pub fn html_cancel(&mut self, id: u32) -> Result<String, JsValue> {
        self.inner.html.cancel(id).map_err(air_js_err)
    }

    pub fn html_set_viewport(
        &mut self,
        id: u32,
        width: f64,
        height: f64,
        scroll_h: f64,
        scroll_v: f64,
    ) -> Result<String, JsValue> {
        self.inner
            .html
            .set_viewport(id, width, height, scroll_h, scroll_v)
            .map_err(air_js_err)
    }

    pub fn html_state_json(&self, id: u32) -> Result<String, JsValue> {
        self.inner.html.state_json(id).map_err(air_js_err)
    }

    pub fn net_interfaces_json(&self) -> String {
        self.inner.net.interfaces_json()
    }

    pub fn net_set_login_credentials(
        &mut self,
        hostname: &str,
        user: &str,
        password: &str,
    ) -> String {
        self.inner
            .net
            .set_login_credentials(hostname, user, password)
    }

    pub fn net_get_login_auth_header(&self, hostname: &str) -> Option<String> {
        self.inner.net.get_login_auth_header(hostname)
    }

    pub fn udp_create(&mut self) -> u32 {
        self.inner.net.udp_create()
    }

    pub fn udp_bind(&mut self, sock_id: u32, port: u32, address: &str) -> Result<String, JsValue> {
        self.inner
            .net
            .udp_bind(sock_id, port, address)
            .map_err(air_js_err)
    }

    pub fn udp_connect(
        &mut self,
        sock_id: u32,
        remote_address: &str,
        remote_port: u32,
    ) -> Result<String, JsValue> {
        self.inner
            .net
            .udp_connect(sock_id, remote_address, remote_port)
            .map_err(air_js_err)
    }

    pub fn udp_receive(&mut self, sock_id: u32) -> Result<(), JsValue> {
        self.inner.net.udp_receive(sock_id).map_err(air_js_err)
    }

    pub fn udp_send(
        &mut self,
        sock_id: u32,
        target_address: Option<String>,
        target_port: u32,
    ) -> Result<String, JsValue> {
        self.inner
            .net
            .udp_send(sock_id, target_address.as_deref(), target_port)
            .map_err(air_js_err)
    }

    pub fn udp_close(&mut self, sock_id: u32) {
        self.inner.net.udp_close(sock_id);
    }

    pub fn tcp_server_create(&mut self) -> u32 {
        self.inner.net.tcp_server_create()
    }

    pub fn tcp_server_bind(
        &mut self,
        server_id: u32,
        port: u32,
        address: &str,
    ) -> Result<String, JsValue> {
        self.inner
            .net
            .tcp_server_bind(server_id, port, address)
            .map_err(air_js_err)
    }

    pub fn tcp_server_listen(&mut self, server_id: u32, backlog: u32) -> Result<(), JsValue> {
        self.inner
            .net
            .tcp_server_listen(server_id, backlog)
            .map_err(air_js_err)
    }

    pub fn tcp_server_close(&mut self, server_id: u32) {
        self.inner.net.tcp_server_close(server_id);
    }

    pub fn tcp_connect_loopback(&mut self, host: &str, port: u32) -> String {
        self.inner.net.tcp_connect_loopback(host, port)
    }

    pub fn tcp_write(&mut self, conn_id: u32, data: &[u8]) -> Result<u32, JsValue> {
        self.inner.net.tcp_write(conn_id, data).map_err(air_js_err)
    }

    pub fn tcp_read(&mut self, conn_id: u32) -> Vec<u8> {
        self.inner.net.tcp_read(conn_id)
    }

    pub fn tcp_endpoint_json(&self, conn_id: u32) -> String {
        self.inner.net.tcp_endpoint_json(conn_id)
    }

    pub fn tcp_close(&mut self, conn_id: u32) -> u32 {
        self.inner.net.tcp_close(conn_id)
    }

    pub fn xml_sig_add_cert(&mut self, cert_bytes: &[u8], trusted: bool) -> Result<(), JsValue> {
        self.inner
            .xml_sig
            .add_certificate(cert_bytes, trusted)
            .map_err(air_js_err)
    }

    pub fn xml_sig_verify_json(
        &self,
        signature_xml: &str,
        deref_map_json: &str,
        references_setting: &str,
        revocation_setting: &str,
        use_system_trust_store: bool,
    ) -> String {
        self.inner.xml_sig.verify_json(
            signature_xml,
            deref_map_json,
            references_setting,
            revocation_setting,
            use_system_trust_store,
        )
    }

    pub fn sha256(&self, data: &[u8]) -> Vec<u8> {
        neoflash_core::as3_air::sha256_bytes(data).to_vec()
    }

    pub fn sha1(&self, data: &[u8]) -> Vec<u8> {
        neoflash_core::as3_air::sha1_bytes(data).to_vec()
    }

    pub fn hmac_sha256(&self, key: &[u8], data: &[u8]) -> Vec<u8> {
        neoflash_core::as3_air::hmac_sha256(key, data).to_vec()
    }

    pub fn xml_c14n(&self, xml_fragment: &str) -> String {
        neoflash_core::as3_air::canonicalize_xml_fragment(xml_fragment)
    }

    pub fn device_rotation_quaternion(
        &self,
        alpha_deg: f64,
        beta_deg: f64,
        gamma_deg: f64,
    ) -> Vec<f64> {
        neoflash_core::as3_air::device_rotation_quaternion(alpha_deg, beta_deg, gamma_deg)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rasterize_text_rgba(
        &self,
        width: u32,
        height: u32,
        text: &str,
        fg_rgb: u32,
        bg_argb: u32,
        font_size: u32,
        flags_align: &str,
    ) -> Vec<u8> {
        neoflash_core::as3_air::rasterize_text_rgba(
            width,
            height,
            text,
            fg_rgb,
            bg_argb,
            font_size,
            flags_align,
        )
    }
}
