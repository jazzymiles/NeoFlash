use super::window_process::extract_attr_value;
use super::*;

// ---------------------------------------------------------------------------
// AIR Network Kernel (`DatagramSocket`, `ServerSocket`, `NetworkInfo`, `URLRequestDefaults`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) struct UdpSocketState {
    pub(super) bound: bool,
    pub(super) receiving: bool,
    pub(super) connected: bool,
    pub(super) local_address: String,
    pub(super) local_port: u16,
    pub(super) remote_address: String,
    pub(super) remote_port: u16,
}

#[derive(Debug, Clone)]
pub(super) struct TcpServerState {
    pub(super) bound: bool,
    pub(super) listening: bool,
    pub(super) backlog: u32,
    pub(super) local_address: String,
    pub(super) local_port: u16,
}

#[derive(Debug, Clone)]
pub(super) struct TcpLoopbackEndpoint {
    pub(super) peer_id: u32,
    pub(super) open: bool,
    pub(super) local_address: String,
    pub(super) local_port: u16,
    pub(super) remote_address: String,
    pub(super) remote_port: u16,
    pub(super) inbox: Vec<u8>,
}

#[derive(Debug)]
pub struct AirNetworkKernel {
    pub(super) udp_sockets: BTreeMap<u32, UdpSocketState>,
    pub(super) tcp_servers: BTreeMap<u32, TcpServerState>,
    pub(super) tcp_endpoints: BTreeMap<u32, TcpLoopbackEndpoint>,
    pub(super) host_credentials: BTreeMap<String, (String, String)>,
    pub(super) next_id: u32,
    pub(super) next_ephemeral_port: u16,
}

impl Default for AirNetworkKernel {
    fn default() -> Self {
        Self {
            udp_sockets: BTreeMap::new(),
            tcp_servers: BTreeMap::new(),
            tcp_endpoints: BTreeMap::new(),
            host_credentials: BTreeMap::new(),
            next_id: 1,
            next_ephemeral_port: 49152,
        }
    }
}

pub(super) fn normalize_ip_host(addr: &str) -> String {
    let trimmed = addr.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("localhost") {
        "127.0.0.1".to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(super) fn ip_endpoints_match(bind_addr: &str, target_addr: &str) -> bool {
    if bind_addr == "0.0.0.0" || bind_addr == "::" {
        return true;
    }
    normalize_ip_host(bind_addr) == normalize_ip_host(target_addr)
}

impl AirNetworkKernel {
    fn alloc_ephemeral_port(&mut self) -> u16 {
        for _ in 0..16384 {
            let candidate = self.next_ephemeral_port;
            self.next_ephemeral_port = if self.next_ephemeral_port >= 65530 {
                49152
            } else {
                self.next_ephemeral_port + 1
            };
            let in_use_udp = self
                .udp_sockets
                .values()
                .any(|s| s.bound && s.local_port == candidate);
            let in_use_tcp = self
                .tcp_servers
                .values()
                .any(|s| s.bound && s.local_port == candidate);
            if !in_use_udp && !in_use_tcp {
                return candidate;
            }
        }
        self.next_ephemeral_port
    }

    pub fn interfaces_json(&self) -> String {
        serde_json::json!([
            {
                "name": "lo0",
                "displayName": "Loopback",
                "active": true,
                "mtu": 16384,
                "hardwareAddress": "",
                "addresses": [
                    {
                        "address": "127.0.0.1",
                        "broadcast": "",
                        "ipVersion": "IPv4",
                        "prefixLength": 8
                    },
                    {
                        "address": "::1",
                        "broadcast": "",
                        "ipVersion": "IPv6",
                        "prefixLength": 128
                    }
                ]
            },
            {
                "name": "en0",
                "displayName": "Ethernet",
                "active": true,
                "mtu": 1500,
                "hardwareAddress": "02:00:5E:10:00:01",
                "addresses": [
                    {
                        "address": "192.168.1.100",
                        "broadcast": "192.168.1.255",
                        "ipVersion": "IPv4",
                        "prefixLength": 24
                    },
                    {
                        "address": "fe80::5e:10ff:fe00:1",
                        "broadcast": "",
                        "ipVersion": "IPv6",
                        "prefixLength": 64
                    }
                ]
            }
        ])
        .to_string()
    }

    pub fn set_login_credentials(&mut self, hostname: &str, user: &str, password: &str) -> String {
        let key = hostname.trim().to_ascii_lowercase();
        if key.is_empty() {
            return String::new();
        }
        if user.is_empty() && password.is_empty() {
            self.host_credentials.remove(&key);
            return String::new();
        }
        self.host_credentials
            .insert(key, (user.to_owned(), password.to_owned()));
        BASE64.encode(format!("{user}:{password}").as_bytes())
    }

    pub fn get_login_auth_header(&self, hostname: &str) -> Option<String> {
        let key = hostname.trim().to_ascii_lowercase();
        self.host_credentials
            .get(&key)
            .map(|(u, p)| format!("Basic {}", BASE64.encode(format!("{u}:{p}").as_bytes())))
    }

    pub fn udp_create(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.udp_sockets.insert(
            id,
            UdpSocketState {
                bound: false,
                receiving: false,
                connected: false,
                local_address: String::new(),
                local_port: 0,
                remote_address: String::new(),
                remote_port: 0,
            },
        );
        id
    }

    pub fn udp_bind(&mut self, sock_id: u32, port: u32, address: &str) -> Result<String, AirError> {
        if port > 65535 {
            return Err(AirError::Range(format!(
                "Port {port} is outside valid UDP range 0..65535."
            )));
        }
        let norm_addr = if address.trim().is_empty() {
            "0.0.0.0".to_owned()
        } else {
            normalize_ip_host(address)
        };
        let actual_port = if port == 0 {
            self.alloc_ephemeral_port()
        } else {
            let p = port as u16;
            let collision = self.udp_sockets.iter().any(|(&other_id, s)| {
                other_id != sock_id
                    && s.bound
                    && s.local_port == p
                    && (s.local_address == "0.0.0.0"
                        || norm_addr == "0.0.0.0"
                        || s.local_address == norm_addr)
            });
            if collision {
                return Err(AirError::Io(format!(
                    "UDP address {norm_addr}:{p} is already in use."
                )));
            }
            p
        };
        let sock = self
            .udp_sockets
            .get_mut(&sock_id)
            .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
        if sock.bound {
            return Err(AirError::Io("DatagramSocket is already bound.".into()));
        }
        sock.bound = true;
        sock.local_address = norm_addr.clone();
        sock.local_port = actual_port;
        Ok(serde_json::json!({
            "localAddress": norm_addr,
            "localPort": actual_port,
        })
        .to_string())
    }

    pub fn udp_connect(
        &mut self,
        sock_id: u32,
        remote_address: &str,
        remote_port: u32,
    ) -> Result<String, AirError> {
        if remote_port == 0 || remote_port > 65535 {
            return Err(AirError::Range(format!(
                "Remote port {remote_port} must be in 1..65535."
            )));
        }
        if remote_address.trim().is_empty() {
            return Err(AirError::Argument(
                "remoteAddress must not be empty.".into(),
            ));
        }
        let norm_remote = normalize_ip_host(remote_address);
        let sock = self
            .udp_sockets
            .get_mut(&sock_id)
            .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
        sock.connected = true;
        sock.remote_address = norm_remote.clone();
        sock.remote_port = remote_port as u16;
        Ok(serde_json::json!({
            "remoteAddress": norm_remote,
            "remotePort": remote_port,
        })
        .to_string())
    }

    pub fn udp_receive(&mut self, sock_id: u32) -> Result<(), AirError> {
        let sock = self
            .udp_sockets
            .get_mut(&sock_id)
            .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
        if !sock.bound {
            return Err(AirError::Io(
                "DatagramSocket must be bound before receive().".into(),
            ));
        }
        sock.receiving = true;
        Ok(())
    }

    pub fn udp_send(
        &mut self,
        sock_id: u32,
        target_address: Option<&str>,
        target_port: u32,
    ) -> Result<String, AirError> {
        let need_ephemeral = {
            let sock = self
                .udp_sockets
                .get(&sock_id)
                .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
            !sock.bound
        };
        if need_ephemeral {
            let p = self.alloc_ephemeral_port();
            if let Some(sock) = self.udp_sockets.get_mut(&sock_id) {
                sock.bound = true;
                sock.local_address = "127.0.0.1".to_owned();
                sock.local_port = p;
            }
        }
        let (src_addr, src_port, dst_addr, dst_port) = {
            let sock = self
                .udp_sockets
                .get(&sock_id)
                .ok_or_else(|| AirError::Io("DatagramSocket is closed.".into()))?;
            let (da, dp) = if sock.connected {
                let explicit_addr = target_address.map(str::trim).filter(|s| !s.is_empty());
                if explicit_addr.is_some() || target_port != 0 {
                    let check_a = explicit_addr
                        .map(normalize_ip_host)
                        .unwrap_or_else(|| sock.remote_address.clone());
                    let check_p = if target_port == 0 {
                        sock.remote_port
                    } else {
                        target_port as u16
                    };
                    if check_a != sock.remote_address || check_p != sock.remote_port {
                        return Err(AirError::Argument(
                            "Connected DatagramSocket cannot send to a different address or port."
                                .into(),
                        ));
                    }
                }
                (sock.remote_address.clone(), sock.remote_port)
            } else {
                let addr_str = target_address
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        AirError::Argument(
                            "targetAddress is required when DatagramSocket is not connected."
                                .into(),
                        )
                    })?;
                if target_port == 0 || target_port > 65535 {
                    return Err(AirError::Range(format!(
                        "UDP target port {target_port} must be in 1..65535."
                    )));
                }
                (normalize_ip_host(addr_str), target_port as u16)
            };
            let sa = if sock.local_address == "0.0.0.0" {
                "127.0.0.1".to_owned()
            } else {
                sock.local_address.clone()
            };
            (sa, sock.local_port, da, dp)
        };

        let recipients: Vec<serde_json::Value> = self
            .udp_sockets
            .iter()
            .filter(|(_, s)| {
                s.bound
                    && s.receiving
                    && s.local_port == dst_port
                    && ip_endpoints_match(&s.local_address, &dst_addr)
                    && (!s.connected
                        || (ip_endpoints_match(&s.remote_address, &src_addr)
                            && s.remote_port == src_port))
            })
            .map(|(&target_id, s)| {
                let reported_dst = if s.local_address == "0.0.0.0" {
                    dst_addr.clone()
                } else {
                    s.local_address.clone()
                };
                serde_json::json!({
                    "targetSockId": target_id,
                    "srcAddress": src_addr,
                    "srcPort": src_port,
                    "dstAddress": reported_dst,
                    "dstPort": dst_port,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "localAddress": src_addr,
            "localPort": src_port,
            "deliveries": recipients,
        })
        .to_string())
    }

    pub fn udp_close(&mut self, sock_id: u32) {
        self.udp_sockets.remove(&sock_id);
    }

    pub fn tcp_server_create(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.tcp_servers.insert(
            id,
            TcpServerState {
                bound: false,
                listening: false,
                backlog: 0,
                local_address: String::new(),
                local_port: 0,
            },
        );
        id
    }

    pub fn tcp_server_bind(
        &mut self,
        server_id: u32,
        port: u32,
        address: &str,
    ) -> Result<String, AirError> {
        if port > 65535 {
            return Err(AirError::Range(format!(
                "Port {port} is outside valid TCP range 0..65535."
            )));
        }
        let norm_addr = if address.trim().is_empty() {
            "0.0.0.0".to_owned()
        } else {
            normalize_ip_host(address)
        };
        let actual_port = if port == 0 {
            self.alloc_ephemeral_port()
        } else {
            let p = port as u16;
            let collision = self.tcp_servers.iter().any(|(&other_id, s)| {
                other_id != server_id
                    && s.bound
                    && s.local_port == p
                    && (s.local_address == "0.0.0.0"
                        || norm_addr == "0.0.0.0"
                        || s.local_address == norm_addr)
            });
            if collision {
                return Err(AirError::Io(format!(
                    "TCP address {norm_addr}:{p} is already in use."
                )));
            }
            p
        };
        let srv = self
            .tcp_servers
            .get_mut(&server_id)
            .ok_or_else(|| AirError::Io("ServerSocket is closed.".into()))?;
        if srv.bound {
            return Err(AirError::Io("ServerSocket is already bound.".into()));
        }
        srv.bound = true;
        srv.local_address = norm_addr.clone();
        srv.local_port = actual_port;
        Ok(serde_json::json!({
            "localAddress": norm_addr,
            "localPort": actual_port,
        })
        .to_string())
    }

    pub fn tcp_server_listen(&mut self, server_id: u32, backlog: u32) -> Result<(), AirError> {
        let srv = self
            .tcp_servers
            .get_mut(&server_id)
            .ok_or_else(|| AirError::Io("ServerSocket is closed.".into()))?;
        if !srv.bound {
            return Err(AirError::Io(
                "ServerSocket must be bound before listen().".into(),
            ));
        }
        srv.listening = true;
        srv.backlog = backlog;
        Ok(())
    }

    pub fn tcp_server_close(&mut self, server_id: u32) {
        self.tcp_servers.remove(&server_id);
    }

    pub fn tcp_connect_loopback(&mut self, host: &str, port: u32) -> String {
        if port == 0 || port > 65535 {
            return serde_json::json!({ "matched": false }).to_string();
        }
        let target_host = normalize_ip_host(host);
        let p = port as u16;
        let matched_server = self
            .tcp_servers
            .iter()
            .find(|(_, srv)| {
                srv.bound
                    && srv.listening
                    && srv.local_port == p
                    && ip_endpoints_match(&srv.local_address, &target_host)
            })
            .map(|(&sid, srv)| {
                let addr = if srv.local_address == "0.0.0.0" {
                    target_host.clone()
                } else {
                    srv.local_address.clone()
                };
                (sid, addr)
            });

        let Some((server_id, srv_addr)) = matched_server else {
            return serde_json::json!({ "matched": false }).to_string();
        };
        let client_port = self.alloc_ephemeral_port();
        let client_id = self.next_id;
        self.next_id += 1;
        let accepted_id = self.next_id;
        self.next_id += 1;

        self.tcp_endpoints.insert(
            client_id,
            TcpLoopbackEndpoint {
                peer_id: accepted_id,
                open: true,
                local_address: "127.0.0.1".to_owned(),
                local_port: client_port,
                remote_address: srv_addr.clone(),
                remote_port: p,
                inbox: Vec::new(),
            },
        );
        self.tcp_endpoints.insert(
            accepted_id,
            TcpLoopbackEndpoint {
                peer_id: client_id,
                open: true,
                local_address: srv_addr.clone(),
                local_port: p,
                remote_address: "127.0.0.1".to_owned(),
                remote_port: client_port,
                inbox: Vec::new(),
            },
        );

        serde_json::json!({
            "matched": true,
            "serverId": server_id,
            "clientConnId": client_id,
            "acceptedConnId": accepted_id,
            "clientAddress": "127.0.0.1",
            "clientPort": client_port,
            "serverAddress": srv_addr,
            "serverPort": p,
        })
        .to_string()
    }

    pub fn tcp_write(&mut self, conn_id: u32, data: &[u8]) -> Result<u32, AirError> {
        let peer_id = self
            .tcp_endpoints
            .get(&conn_id)
            .filter(|e| e.open)
            .map(|e| e.peer_id)
            .ok_or_else(|| AirError::Io("Socket is not connected.".into()))?;
        let peer = self
            .tcp_endpoints
            .get_mut(&peer_id)
            .filter(|e| e.open)
            .ok_or_else(|| AirError::Io("Peer socket is closed.".into()))?;
        peer.inbox.extend_from_slice(data);
        Ok(peer_id)
    }

    pub fn tcp_read(&mut self, conn_id: u32) -> Vec<u8> {
        self.tcp_endpoints
            .get_mut(&conn_id)
            .map(|ep| std::mem::take(&mut ep.inbox))
            .unwrap_or_default()
    }

    pub fn tcp_endpoint_json(&self, conn_id: u32) -> String {
        let Some(ep) = self.tcp_endpoints.get(&conn_id) else {
            return serde_json::json!({ "connected": false }).to_string();
        };
        serde_json::json!({
            "connected": ep.open,
            "localAddress": ep.local_address,
            "localPort": ep.local_port,
            "remoteAddress": ep.remote_address,
            "remotePort": ep.remote_port,
            "bytesAvailable": ep.inbox.len(),
        })
        .to_string()
    }

    pub fn tcp_close(&mut self, conn_id: u32) -> u32 {
        let Some(mut ep) = self.tcp_endpoints.remove(&conn_id) else {
            return 0;
        };
        ep.open = false;
        if let Some(peer) = self.tcp_endpoints.get_mut(&ep.peer_id)
            && peer.open
        {
            peer.open = false;
            return ep.peer_id;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// AIR XML Digital Signature Validator (`flash.security.*`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) struct StoredCert {
    pub(super) raw_bytes: Vec<u8>,
    pub(super) fingerprint: [u8; 32],
    pub(super) trusted: bool,
}

#[derive(Debug, Default)]
pub struct AirXmlSigEngine {
    pub(super) certs: Vec<StoredCert>,
}

pub(super) struct ParsedCertMeta {
    pub(super) cn: String,
    pub(super) dn: String,
    pub(super) issuer_cn: String,
    pub(super) issuer_dn: String,
    pub(super) ekus: Vec<String>,
    pub(super) trust_settings: Vec<String>,
    pub(super) system_trusted: bool,
    pub(super) rsa_public_key: Option<(Vec<u8>, Vec<u8>)>,
}

pub(super) fn decode_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let padded = if clean.len() % 2 == 1 {
        format!("0{clean}")
    } else {
        clean
    };
    (0..padded.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&padded[i..i + 2], 16).ok())
        .collect()
}

pub(super) fn parse_der_length(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let first = *bytes.get(pos)?;
    if (first & 0x80) == 0 {
        Some((first as usize, pos + 1))
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || pos + 1 + count > bytes.len() {
            return None;
        }
        let mut len = 0usize;
        for &b in &bytes[pos + 1..pos + 1 + count] {
            len = (len << 8) | (b as usize);
        }
        Some((len, pos + 1 + count))
    }
}

pub(super) fn extract_der_rsa_public_key(der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // OID 1.2.840.113549.1.1.1 (rsaEncryption): 06 09 2a 86 48 86 f7 0d 01 01 01
    let rsa_oid = [
        0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01,
    ];
    let pos = der.windows(rsa_oid.len()).position(|w| w == rsa_oid)?;
    let mut cur = pos + rsa_oid.len();
    // Skip optional NULL (05 00) and find BIT STRING (0x03) or SEQUENCE (0x30)
    while cur < der.len() && der[cur] != 0x03 && der[cur] != 0x30 {
        cur += 1;
    }
    if cur >= der.len() {
        return None;
    }
    if der[cur] == 0x03 {
        let (_bit_len, next_pos) = parse_der_length(der, cur + 1)?;
        cur = next_pos + 1; // skip unused-bits byte
    }
    if *der.get(cur)? != 0x30 {
        return None;
    }
    let (_seq_len, seq_body) = parse_der_length(der, cur + 1)?;
    cur = seq_body;
    if *der.get(cur)? != 0x02 {
        return None;
    }
    let (mod_len, mod_start) = parse_der_length(der, cur + 1)?;
    let mod_end = mod_start.checked_add(mod_len)?;
    let mut modulus = der.get(mod_start..mod_end)?.to_vec();
    while modulus.first() == Some(&0) && modulus.len() > 1 {
        modulus.remove(0);
    }
    cur = mod_end;
    if *der.get(cur)? != 0x02 {
        return None;
    }
    let (exp_len, exp_start) = parse_der_length(der, cur + 1)?;
    let exp_end = exp_start.checked_add(exp_len)?;
    let mut exponent = der.get(exp_start..exp_end)?.to_vec();
    while exponent.first() == Some(&0) && exponent.len() > 1 {
        exponent.remove(0);
    }
    if modulus.is_empty() || exponent.is_empty() {
        None
    } else {
        Some((modulus, exponent))
    }
}

pub(super) fn parse_cert_metadata(cert_bytes: &[u8]) -> ParsedCertMeta {
    let text = String::from_utf8_lossy(cert_bytes);
    if text.contains("CN=") || text.trim_start().starts_with('{') {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(text.trim()) {
            let cn = val["cn"]
                .as_str()
                .unwrap_or("NeoFlash Certificate")
                .to_owned();
            let dn = val["dn"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("CN={cn}"));
            let ekus = val["eku"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec!["1.3.6.1.5.5.7.3.3".to_owned()]);
            let trust = val["trust"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec!["signing".to_owned(), "codeSigning".to_owned()]);
            let sys = val["systemTrusted"].as_bool().unwrap_or(false);
            let rsa_pk = val["rsaModulusHex"]
                .as_str()
                .and_then(decode_hex_bytes)
                .zip(
                    val["rsaExponentHex"]
                        .as_str()
                        .and_then(decode_hex_bytes)
                        .or_else(|| Some(vec![0x01, 0x00, 0x01])),
                );
            return ParsedCertMeta {
                issuer_cn: val["issuerCn"].as_str().unwrap_or(&cn).to_owned(),
                issuer_dn: val["issuerDn"].as_str().unwrap_or(&dn).to_owned(),
                cn,
                dn,
                ekus,
                trust_settings: trust,
                system_trusted: sys,
                rsa_public_key: rsa_pk,
            };
        }
        let mut cn = String::new();
        let mut org = String::new();
        let mut ekus = Vec::new();
        let mut sys = false;
        let mut mod_hex: Option<Vec<u8>> = None;
        let mut exp_hex: Option<Vec<u8>> = None;
        for part in text.split([';', ',', '\n']) {
            let p = part.trim();
            if let Some(v) = p.strip_prefix("CN=") {
                cn = v.trim().to_owned();
            } else if let Some(v) = p.strip_prefix("O=") {
                org = v.trim().to_owned();
            } else if let Some(v) = p.strip_prefix("EKU=") {
                for oid in v.split('|') {
                    if !oid.trim().is_empty() {
                        ekus.push(oid.trim().to_owned());
                    }
                }
            } else if p.eq_ignore_ascii_case("SYSTEM_TRUSTED=true") {
                sys = true;
            } else if let Some(v) = p.strip_prefix("MOD=") {
                mod_hex = decode_hex_bytes(v);
            } else if let Some(v) = p.strip_prefix("EXP=") {
                exp_hex = decode_hex_bytes(v);
            }
        }
        if cn.is_empty() {
            cn = "NeoFlash Signer".to_owned();
        }
        let dn = if org.is_empty() {
            format!("CN={cn}")
        } else {
            format!("CN={cn}, O={org}")
        };
        if ekus.is_empty() {
            ekus.push("1.3.6.1.5.5.7.3.3".to_owned());
        }
        let rsa_public_key =
            mod_hex.map(|m| (m, exp_hex.unwrap_or_else(|| vec![0x01, 0x00, 0x01])));
        return ParsedCertMeta {
            issuer_cn: cn.clone(),
            issuer_dn: dn.clone(),
            cn,
            dn,
            ekus,
            trust_settings: vec!["signing".to_owned(), "codeSigning".to_owned()],
            system_trusted: sys,
            rsa_public_key,
        };
    }

    // Scan ASN.1 DER X.509 OID 2.5.4.3 (55 04 03) CommonName & 2.5.4.10 (55 04 0A) Organization.
    // X.509 TBSCertificate encodes issuer RDNSequence first and subject RDNSequence second.
    let issuer_cn = extract_der_oid_string(cert_bytes, &[0x55, 0x04, 0x03])
        .unwrap_or_else(|| "X509 Signer".to_owned());
    let issuer_org = extract_der_oid_string(cert_bytes, &[0x55, 0x04, 0x0a]);
    let issuer_dn = match issuer_org {
        Some(o) => format!("CN={issuer_cn}, O={o}"),
        None => format!("CN={issuer_cn}"),
    };
    let cn = extract_der_oid_string_last(cert_bytes, &[0x55, 0x04, 0x03])
        .unwrap_or_else(|| "X509 Signer".to_owned());
    let org = extract_der_oid_string_last(cert_bytes, &[0x55, 0x04, 0x0a]);
    let dn = match org {
        Some(o) => format!("CN={cn}, O={o}"),
        None => format!("CN={cn}"),
    };
    ParsedCertMeta {
        cn,
        dn,
        issuer_cn,
        issuer_dn,
        ekus: vec!["1.3.6.1.5.5.7.3.3".to_owned()],
        trust_settings: vec!["signing".to_owned(), "codeSigning".to_owned()],
        system_trusted: false,
        rsa_public_key: extract_der_rsa_public_key(cert_bytes),
    }
}

// Pure-Rust BigUint modular exponentiation (`base^exp mod modulus`) for RSA PKCS#1 v1.5 verification
pub(super) fn big_from_be(bytes: &[u8]) -> Vec<u32> {
    let mut limbs = Vec::with_capacity(bytes.len().div_ceil(4));
    let mut idx = bytes.len();
    while idx > 0 {
        let start = idx.saturating_sub(4);
        let mut limb = 0u32;
        for &b in &bytes[start..idx] {
            limb = (limb << 8) | (b as u32);
        }
        limbs.push(limb);
        idx = start;
    }
    big_trim(&mut limbs);
    limbs
}

pub(super) fn big_to_be(limbs: &[u32], target_len: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity(limbs.len() * 4);
    for &limb in limbs.iter().rev() {
        raw.extend_from_slice(&limb.to_be_bytes());
    }
    let first_nz = raw.iter().position(|&b| b != 0).unwrap_or(raw.len());
    let trimmed = &raw[first_nz..];
    if trimmed.len() >= target_len {
        trimmed.to_vec()
    } else {
        let mut out = vec![0u8; target_len - trimmed.len()];
        out.extend_from_slice(trimmed);
        out
    }
}

pub(super) fn big_trim(a: &mut Vec<u32>) {
    while a.last() == Some(&0) {
        a.pop();
    }
}

pub(super) fn big_cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for (x, y) in a.iter().rev().zip(b.iter().rev()) {
        if x != y {
            return x.cmp(y);
        }
    }
    std::cmp::Ordering::Equal
}

pub(super) fn big_shl1(a: &mut Vec<u32>) {
    let mut carry = 0u32;
    for limb in a.iter_mut() {
        let next = (*limb >> 31) & 1;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    if carry != 0 {
        a.push(carry);
    }
}

pub(super) fn big_sub_assign(a: &mut Vec<u32>, b: &[u32]) {
    let mut borrow = 0i64;
    for (i, limb_a) in a.iter_mut().enumerate() {
        let limb_b = *b.get(i).unwrap_or(&0) as i64;
        let diff = (*limb_a as i64) - limb_b - borrow;
        if diff < 0 {
            *limb_a = (diff + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            *limb_a = diff as u32;
            borrow = 0;
        }
    }
    big_trim(a);
}

pub(super) fn big_mod(a: &[u32], m: &[u32]) -> Vec<u32> {
    if m.is_empty() {
        return Vec::new();
    }
    if big_cmp(a, m).is_lt() {
        return a.to_vec();
    }
    let mut rem: Vec<u32> = Vec::new();
    let total_bits = a.len() * 32;
    for bit_idx in (0..total_bits).rev() {
        let limb_idx = bit_idx / 32;
        let bit_off = bit_idx % 32;
        let bit = (a[limb_idx] >> bit_off) & 1;
        big_shl1(&mut rem);
        if bit != 0 {
            if rem.is_empty() {
                rem.push(1);
            } else {
                rem[0] |= 1;
            }
        }
        if big_cmp(&rem, m).is_ge() {
            big_sub_assign(&mut rem, m);
        }
    }
    rem
}

pub(super) fn big_mul_mod(a: &[u32], b: &[u32], m: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() || m.is_empty() {
        return Vec::new();
    }
    let mut prod = vec![0u32; a.len() + b.len()];
    for (i, &xa) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &yb) in b.iter().enumerate() {
            let acc = (prod[i + j] as u64) + (xa as u64) * (yb as u64) + carry;
            prod[i + j] = acc as u32;
            carry = acc >> 32;
        }
        prod[i + b.len()] = carry as u32;
    }
    big_trim(&mut prod);
    big_mod(&prod, m)
}

pub fn rsa_mod_exp_be(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
    let m = big_from_be(modulus);
    if m.is_empty() {
        return Vec::new();
    }
    let mut result = vec![1u32];
    let mut b = big_mod(&big_from_be(base), &m);
    for &byte in exp.iter().rev() {
        for bit in 0..8 {
            if ((byte >> bit) & 1) != 0 {
                result = big_mul_mod(&result, &b, &m);
            }
            b = big_mul_mod(&b, &b, &m);
        }
    }
    big_to_be(&result, modulus.len())
}

pub(super) fn verify_rsa_pkcs1_v15(
    sig_bytes: &[u8],
    modulus: &[u8],
    exponent: &[u8],
    message: &[u8],
) -> bool {
    if sig_bytes.is_empty() || modulus.len() < 16 || exponent.is_empty() {
        return false;
    }
    let em = rsa_mod_exp_be(sig_bytes, exponent, modulus);
    let sha256_digest = sha256_bytes(message);
    let sha1_digest = sha1_bytes(message);
    // Standard PKCS#1 v1.5 DigestInfo prefixes for SHA-256 and SHA-1:
    const SHA256_DER_PREFIX: &[u8] = &[
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    const SHA1_DER_PREFIX: &[u8] = &[
        0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14,
    ];

    // Check 0x00 0x01 0xFF* 0x00 || payload structure (or compact 0x01 ... 0x00 || digest)
    let without_leading_zero = em.strip_prefix(&[0x00]).unwrap_or(&em);
    let Some(rest) = without_leading_zero.strip_prefix(&[0x01]) else {
        return false;
    };
    let ff_count = rest.iter().take_while(|&&b| b == 0xff).count();
    let Some(payload) = rest.get(ff_count..).and_then(|s| s.strip_prefix(&[0x00])) else {
        return false;
    };

    let mut sha256_info = Vec::with_capacity(SHA256_DER_PREFIX.len() + 32);
    sha256_info.extend_from_slice(SHA256_DER_PREFIX);
    sha256_info.extend_from_slice(&sha256_digest);

    let mut sha1_info = Vec::with_capacity(SHA1_DER_PREFIX.len() + 20);
    sha1_info.extend_from_slice(SHA1_DER_PREFIX);
    sha1_info.extend_from_slice(&sha1_digest);

    payload == sha256_info.as_slice()
        || payload == sha256_digest.as_slice()
        || payload == sha1_info.as_slice()
        || payload == sha1_digest.as_slice()
}

pub(super) fn extract_der_oid_strings(der: &[u8], oid_bytes: &[u8]) -> Vec<String> {
    let mut matches_out = Vec::new();
    if oid_bytes.is_empty() || der.len() < oid_bytes.len() + 4 {
        return matches_out;
    }
    for idx in 2..=der.len() - oid_bytes.len() - 2 {
        if der[idx - 2] == 0x06
            && der[idx - 1] as usize == oid_bytes.len()
            && &der[idx..idx + oid_bytes.len()] == oid_bytes
        {
            let after = idx + oid_bytes.len();
            let str_tag = der[after];
            if matches!(str_tag, 0x0c | 0x13 | 0x16)
                && let Some((str_len, val_start)) = parse_der_length(der, after + 1)
                && let Some(val_end) = val_start.checked_add(str_len)
                && val_end <= der.len()
            {
                matches_out.push(String::from_utf8_lossy(&der[val_start..val_end]).into_owned());
            }
        }
    }
    matches_out
}

pub(super) fn extract_der_oid_string(der: &[u8], oid_bytes: &[u8]) -> Option<String> {
    extract_der_oid_strings(der, oid_bytes).into_iter().next()
}

pub(super) fn extract_der_oid_string_last(der: &[u8], oid_bytes: &[u8]) -> Option<String> {
    extract_der_oid_strings(der, oid_bytes)
        .into_iter()
        .next_back()
}

pub(super) fn extract_xml_tag_body(xml: &str, local_name: &str) -> Option<String> {
    let blocks = extract_all_xml_tag_blocks(xml, local_name);
    blocks.into_iter().next().map(|(_, inner)| inner)
}

pub(super) fn extract_all_xml_tag_blocks(xml: &str, local_name: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let bytes = xml.as_bytes();
    let mut pos = 0usize;
    while let Some(lt_rel) = xml[pos..].find('<') {
        let lt = pos + lt_rel;
        if xml[lt..].starts_with("<!--") {
            pos = xml[lt + 4..]
                .find("-->")
                .map_or(bytes.len(), |r| lt + 4 + r + 3);
            continue;
        }
        let Some(gt_rel) = xml[lt + 1..].find('>') else {
            break;
        };
        let gt = lt + 1 + gt_rel;
        let head = xml[lt + 1..gt].trim();
        if head.starts_with('/') || head.starts_with('?') || head.starts_with('!') {
            pos = gt + 1;
            continue;
        }
        let self_closing = head.ends_with('/');
        let clean = head.trim_end_matches('/').trim();
        let (full_tag, attrs) = match clean.find(char::is_whitespace) {
            Some(sp) => (&clean[..sp], &clean[sp..]),
            None => (clean, ""),
        };
        let tag_local = full_tag.rsplit(':').next().unwrap_or(full_tag);
        if tag_local == local_name {
            if self_closing {
                results.push((attrs.to_owned(), String::new()));
                pos = gt + 1;
                continue;
            }
            let mut scan = gt + 1;
            let mut depth = 1usize;
            let mut matched_end: Option<(usize, usize)> = None;
            while let Some(inner_lt_rel) = xml[scan..].find('<') {
                let inner_lt = scan + inner_lt_rel;
                if xml[inner_lt..].starts_with("<!--") {
                    scan = xml[inner_lt + 4..]
                        .find("-->")
                        .map_or(bytes.len(), |r| inner_lt + 4 + r + 3);
                    continue;
                }
                let Some(inner_gt_rel) = xml[inner_lt + 1..].find('>') else {
                    break;
                };
                let inner_gt = inner_lt + 1 + inner_gt_rel;
                let inner_head = xml[inner_lt + 1..inner_gt].trim();
                if let Some(close_rest) = inner_head.strip_prefix('/') {
                    let close_name = close_rest.trim();
                    let close_local = close_name.rsplit(':').next().unwrap_or(close_name);
                    if close_name == full_tag || close_local == local_name {
                        depth -= 1;
                        if depth == 0 {
                            matched_end = Some((inner_lt, inner_gt + 1));
                            break;
                        }
                    }
                } else if !inner_head.starts_with('?')
                    && !inner_head.starts_with('!')
                    && !inner_head.ends_with('/')
                {
                    let inner_clean = inner_head.trim();
                    let inner_tag = inner_clean
                        .split(char::is_whitespace)
                        .next()
                        .unwrap_or(inner_clean);
                    let inner_local = inner_tag.rsplit(':').next().unwrap_or(inner_tag);
                    if inner_tag == full_tag || inner_local == local_name {
                        depth += 1;
                    }
                }
                scan = inner_gt + 1;
            }
            if let Some((close_lt, after_close)) = matched_end {
                let inner = xml[gt + 1..close_lt].to_owned();
                results.push((attrs.to_owned(), inner));
                pos = after_close;
                continue;
            }
        }
        pos = gt + 1;
    }
    results
}

pub(super) fn parse_and_sort_c14n_attrs(attr_str: &str) -> Vec<(String, String)> {
    let mut ns_attrs: Vec<(String, String)> = Vec::new();
    let mut normal_attrs: Vec<(String, String)> = Vec::new();
    let chars: Vec<char> = attr_str.chars().collect();
    let n = chars.len();
    let mut i = 0usize;
    while i < n {
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let name_start = i;
        while i < n && !chars[i].is_whitespace() && chars[i] != '=' {
            i += 1;
        }
        let name: String = chars[name_start..i].iter().collect();
        if name.is_empty() {
            break;
        }
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= n || chars[i] != '=' {
            continue;
        }
        i += 1; // skip '='
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let quote = chars[i];
        let mut val = String::new();
        if quote == '"' || quote == '\'' {
            i += 1;
            while i < n && chars[i] != quote {
                let c = chars[i];
                if matches!(c, '\t' | '\n' | '\r') {
                    val.push(' ');
                } else if c == '"' {
                    val.push_str("&quot;");
                } else {
                    val.push(c);
                }
                i += 1;
            }
            if i < n && chars[i] == quote {
                i += 1;
            }
        } else {
            while i < n && !chars[i].is_whitespace() {
                val.push(chars[i]);
                i += 1;
            }
        }
        // Omit redundant default XMLDSig namespace attributes injected by E4X re-serialization
        if name == "xmlns" && val == "http://www.w3.org/2000/09/xmldsig#" {
            continue;
        }
        if name == "xmlns" || name.starts_with("xmlns:") {
            ns_attrs.push((name, val));
        } else {
            normal_attrs.push((name, val));
        }
    }
    ns_attrs.sort_by(|a, b| a.0.cmp(&b.0));
    normal_attrs.sort_by(|a, b| a.0.cmp(&b.0));
    ns_attrs.extend(normal_attrs);
    ns_attrs
}

/// W3C Canonical XML 1.0 (`http://www.w3.org/TR/2001/REC-xml-c14n-20010315`) canonicalizer:
/// - Normalizes line endings (`\r\n` and `\r` -> `\n`) and removes XML comments / PI headers
/// - Sorts namespace declarations and attributes lexicographically with double-quoted values
/// - Expands self-closing empty elements `<tag .../>` into `<tag ...></tag>`
/// - Normalizes inter-element whitespace while preserving character content
pub fn canonicalize_xml_fragment(xml_frag: &str) -> String {
    let normalized = xml_frag.replace("\r\n", "\n").replace('\r', "\n");
    let src = normalized.trim();
    let mut out = String::with_capacity(src.len() + 64);
    let mut pos = 0usize;
    while pos < src.len() {
        if let Some(lt_rel) = src[pos..].find('<') {
            let lt = pos + lt_rel;
            let text_segment = &src[pos..lt];
            if !text_segment.trim().is_empty() {
                let mut prev_ws = false;
                for ch in text_segment.trim().chars() {
                    if ch.is_whitespace() {
                        if !prev_ws {
                            out.push(' ');
                            prev_ws = true;
                        }
                    } else {
                        out.push(ch);
                        prev_ws = false;
                    }
                }
            }
            if src[lt..].starts_with("<!--") {
                pos = src[lt + 4..]
                    .find("-->")
                    .map_or(src.len(), |r| lt + 4 + r + 3);
                continue;
            }
            if src[lt..].starts_with("<?") {
                pos = src[lt + 2..]
                    .find("?>")
                    .map_or(src.len(), |r| lt + 2 + r + 2);
                continue;
            }
            let Some(gt_rel) = src[lt + 1..].find('>') else {
                break;
            };
            let gt = lt + 1 + gt_rel;
            let raw_tag = src[lt + 1..gt].trim();
            if let Some(close_name) = raw_tag.strip_prefix('/') {
                let clean_name = close_name.trim();
                out.push_str("</");
                out.push_str(clean_name);
                out.push('>');
            } else {
                let self_closing = raw_tag.ends_with('/');
                let body = raw_tag.trim_end_matches('/').trim();
                let (tag_name, attr_part) = match body.find(char::is_whitespace) {
                    Some(sp) => (&body[..sp], &body[sp..]),
                    None => (body, ""),
                };
                out.push('<');
                out.push_str(tag_name);
                for (k, v) in parse_and_sort_c14n_attrs(attr_part) {
                    out.push(' ');
                    out.push_str(&k);
                    out.push_str("=\"");
                    out.push_str(&v);
                    out.push('"');
                }
                out.push('>');
                if self_closing {
                    out.push_str("</");
                    out.push_str(tag_name);
                    out.push('>');
                }
            }
            pos = gt + 1;
        } else {
            let tail = src[pos..].trim();
            if !tail.is_empty() {
                out.push_str(tail);
            }
            break;
        }
    }
    out
}

impl AirXmlSigEngine {
    pub fn add_certificate(&mut self, cert_bytes: &[u8], trusted: bool) -> Result<(), AirError> {
        if cert_bytes.is_empty() {
            return Err(AirError::Argument(
                "Certificate ByteArray must not be empty.".into(),
            ));
        }
        let fp = sha256_bytes(cert_bytes);
        if let Some(existing) = self.certs.iter_mut().find(|c| c.fingerprint == fp) {
            existing.trusted = trusted;
            existing.raw_bytes = cert_bytes.to_vec();
        } else {
            self.certs.push(StoredCert {
                raw_bytes: cert_bytes.to_vec(),
                fingerprint: fp,
                trusted,
            });
        }
        Ok(())
    }

    pub fn verify_json(
        &self,
        signature_xml: &str,
        deref_map_json: &str,
        references_setting: &str,
        revocation_setting: &str,
        use_system_trust_store: bool,
    ) -> String {
        let deref_map: BTreeMap<String, String> =
            serde_json::from_str(deref_map_json).unwrap_or_default();

        let Some(signed_info_inner) = extract_xml_tag_body(signature_xml, "SignedInfo") else {
            return serde_json::json!({
                "error": "Signature XML is missing <SignedInfo>.",
                "digestStatus": "invalid",
                "referencesStatus": "invalid",
                "identityStatus": "invalid",
                "validityStatus": "invalid",
            })
            .to_string();
        };
        let Some(sig_val_b64) = extract_xml_tag_body(signature_xml, "SignatureValue") else {
            return serde_json::json!({
                "error": "Signature XML is missing <SignatureValue>.",
                "digestStatus": "invalid",
                "referencesStatus": "invalid",
                "identityStatus": "invalid",
                "validityStatus": "invalid",
            })
            .to_string();
        };
        let compact_sig_b64: String = sig_val_b64.chars().filter(|c| !c.is_whitespace()).collect();
        let sig_bytes =
            if let Some(b) = decode_hex_bytes(&compact_sig_b64).filter(|v| v.len() >= 16) {
                b
            } else if let Ok(b) = BASE64.decode(compact_sig_b64.as_bytes()) {
                b
            } else {
                return serde_json::json!({
                    "error": "SignatureValue is not valid Base64.",
                    "digestStatus": "invalid",
                    "referencesStatus": "invalid",
                    "identityStatus": "invalid",
                    "validityStatus": "invalid",
                })
                .to_string();
            };

        let cert_bytes_opt = extract_xml_tag_body(signature_xml, "X509Certificate")
            .and_then(|b64| {
                let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
                BASE64.decode(clean.as_bytes()).ok()
            })
            .filter(|b| !b.is_empty())
            .or_else(|| self.certs.last().map(|c| c.raw_bytes.clone()));
        let Some(cert_bytes) = cert_bytes_opt else {
            return serde_json::json!({
                "error": "Signature XML is missing <X509Certificate>.",
                "digestStatus": "invalid",
                "referencesStatus": "unknown",
                "identityStatus": "invalid",
                "validityStatus": "invalid",
            })
            .to_string();
        };

        let meta = parse_cert_metadata(&cert_bytes);
        let cert_fp = sha256_bytes(&cert_bytes);
        let canonical_si =
            canonicalize_xml_fragment(&format!("<SignedInfo>{signed_info_inner}</SignedInfo>"));

        let expected_hmac = hmac_sha256(&cert_bytes, canonical_si.as_bytes());
        let mut concat_buf = Vec::with_capacity(cert_bytes.len() + canonical_si.len());
        concat_buf.extend_from_slice(&cert_bytes);
        concat_buf.extend_from_slice(canonical_si.as_bytes());
        let expected_sha256 = sha256_bytes(&concat_buf);
        let rsa_valid = meta
            .rsa_public_key
            .as_ref()
            .is_some_and(|(modulus, exponent)| {
                verify_rsa_pkcs1_v15(&sig_bytes, modulus, exponent, canonical_si.as_bytes())
            });

        let digest_valid = sig_bytes.as_slice() == expected_hmac.as_slice()
            || sig_bytes.as_slice() == expected_sha256.as_slice()
            || rsa_valid;
        let digest_status = if digest_valid { "valid" } else { "invalid" };

        let stored = self.certs.iter().find(|c| c.fingerprint == cert_fp);
        let mut identity_status = match stored {
            Some(c) if c.trusted => "valid",
            Some(_) => "invalid",
            None if use_system_trust_store && meta.system_trusted => "valid",
            None => "unknown",
        };
        if revocation_setting == "alwaysRequired" && identity_status != "valid" {
            identity_status = "invalid";
        }

        let refs = extract_all_xml_tag_blocks(&signed_info_inner, "Reference");
        let references_status = if digest_status == "invalid"
            || references_setting == "never"
            || (references_setting == "validIdentity" && identity_status != "valid")
        {
            "unknown"
        } else if refs.is_empty() {
            "invalid"
        } else {
            let mut all_ok = true;
            for (ref_attrs, ref_inner) in &refs {
                let uri = extract_attr_value(ref_attrs, "URI").unwrap_or_default();
                let Some(payload_b64) = deref_map.get(&uri) else {
                    all_ok = false;
                    break;
                };
                let Ok(payload_bytes) = BASE64.decode(payload_b64.as_bytes()) else {
                    all_ok = false;
                    break;
                };
                let alg = extract_all_xml_tag_blocks(ref_inner, "DigestMethod")
                    .first()
                    .and_then(|(a, _)| extract_attr_value(a, "Algorithm"))
                    .unwrap_or_else(|| "http://www.w3.org/2001/04/xmlenc#sha256".to_owned());
                let Some(dv_b64) = extract_xml_tag_body(ref_inner, "DigestValue") else {
                    all_ok = false;
                    break;
                };
                let clean_dv: String = dv_b64.chars().filter(|c| !c.is_whitespace()).collect();
                let expected_digest = if (clean_dv.len() == 64 || clean_dv.len() == 40)
                    && clean_dv.chars().all(|c| c.is_ascii_hexdigit())
                {
                    (0..clean_dv.len())
                        .step_by(2)
                        .filter_map(|i| u8::from_str_radix(&clean_dv[i..i + 2], 16).ok())
                        .collect::<Vec<u8>>()
                } else if let Ok(d) = BASE64.decode(clean_dv.as_bytes()) {
                    d
                } else {
                    all_ok = false;
                    break;
                };
                let computed: Vec<u8> = if alg.to_ascii_lowercase().contains("sha1") {
                    sha1_bytes(&payload_bytes).to_vec()
                } else {
                    sha256_bytes(&payload_bytes).to_vec()
                };
                if computed != expected_digest {
                    all_ok = false;
                    break;
                }
            }
            if all_ok { "valid" } else { "invalid" }
        };

        let validity_status = if digest_status == "invalid"
            || references_status == "invalid"
            || identity_status == "invalid"
        {
            "invalid"
        } else if digest_status == "valid"
            && references_status == "valid"
            && identity_status == "valid"
        {
            "valid"
        } else {
            "unknown"
        };

        let trust_out = if identity_status == "valid" {
            meta.trust_settings
        } else {
            Vec::new()
        };

        serde_json::json!({
            "digestStatus": digest_status,
            "referencesStatus": references_status,
            "identityStatus": identity_status,
            "validityStatus": validity_status,
            "signerCN": meta.cn,
            "signerDN": meta.dn,
            "issuerCN": meta.issuer_cn,
            "issuerDN": meta.issuer_dn,
            "signerExtendedKeyUsages": meta.ekus,
            "signerTrustSettings": trust_out,
        })
        .to_string()
    }
}
