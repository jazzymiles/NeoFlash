//! WASM bindings for AS3 networking, workers, security, and system services (`As3NetSysKernel`).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct As3NetSysKernel {
    inner: neoflash_core::as3_net_sys::As3NetSysKernel,
}

impl Default for As3NetSysKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl As3NetSysKernel {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: neoflash_core::as3_net_sys::As3NetSysKernel::new(),
        }
    }

    pub fn reset(&mut self) {
        self.inner.reset();
    }

    pub fn execute_json(&mut self, request_json: &str) -> Result<String, JsValue> {
        self.inner
            .execute_json(request_json)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn random_bytes(&mut self, count: u32, entropy: &[u8]) -> Result<Vec<u8>, JsValue> {
        self.inner
            .random_bytes(count, entropy)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn flv_append(&mut self, stream_id: u32, bytes: &[u8]) -> String {
        self.inner.flv_append(stream_id, bytes)
    }

    pub fn xml_socket_feed(&mut self, socket_id: u32, bytes: &[u8]) -> String {
        self.inner.xml_socket_feed(socket_id, bytes)
    }

    pub fn parse_x509(&self, bytes: &[u8]) -> Result<String, JsValue> {
        self.inner
            .parse_x509(bytes)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn loopback_x509_der(&self, host: &str) -> Vec<u8> {
        self.inner.loopback_x509_der(host)
    }
}
