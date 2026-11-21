use super::*;

// ---------------------------------------------------------------------------
// GroupSpecifier & NetGroup P2P Mesh
// ---------------------------------------------------------------------------

pub(super) fn encode_tlv(tag: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(tag);
    let len = payload.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        out.push(0x80 | (((len >> 8) & 0x7f) as u8));
        out.push((len & 0xff) as u8);
    }
    out.extend_from_slice(payload);
}

pub fn encode_posting_authorization(password: &str) -> String {
    let mut buf = Vec::new();
    let digest = sha256(format!("post-auth:{password}").as_bytes());
    encode_tlv(0x15, &digest[..16], &mut buf);
    hex_encode(&buf)
}

pub fn encode_publish_authorization(password: &str) -> String {
    let mut buf = Vec::new();
    let digest = sha256(format!("pub-auth:{password}").as_bytes());
    encode_tlv(0x13, &digest[..16], &mut buf);
    hex_encode(&buf)
}

pub fn encode_bootstrap_peer_id_spec(peer_id: &str) -> Result<String, String> {
    if peer_id.trim().is_empty() {
        return Err("Error #2004: PeerID must not be empty.".into());
    }
    let mut buf = Vec::new();
    let peer_bytes = hex_decode(peer_id).unwrap_or_else(|_| sha256(peer_id.as_bytes()).to_vec());
    encode_tlv(0x0b, &peer_bytes, &mut buf);
    Ok(hex_encode(&buf))
}

pub fn encode_ip_multicast_address_spec(
    address: &str,
    port: Option<u32>,
    source: Option<&str>,
) -> Result<String, String> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err("Error #2004: Multicast address must not be empty.".into());
    }
    let (host, resolved_port) = if let Some((h, p)) = trimmed.rsplit_once(':') {
        if !h.contains(':') {
            let parsed_p = p
                .parse::<u32>()
                .map_err(|_| "Error #2004: Invalid multicast port.".to_owned())?;
            (h, port.unwrap_or(parsed_p))
        } else {
            (trimmed, port.unwrap_or(0))
        }
    } else {
        (trimmed, port.unwrap_or(0))
    };
    if resolved_port > 65535 {
        return Err("Error #2004: Multicast port must be 0..65535.".into());
    }
    let mut payload = format!("{host}:{resolved_port}").into_bytes();
    if let Some(src) = source.filter(|s| !s.is_empty()) {
        payload.push(b'@');
        payload.extend_from_slice(src.as_bytes());
    }
    let mut buf = Vec::new();
    encode_tlv(0x0d, &payload, &mut buf);
    Ok(hex_encode(&buf))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSpecifierState {
    pub name: String,
    pub ip_multicast_member_updates_enabled: bool,
    pub min_groupspec_version: u32,
    pub multicast_enabled: bool,
    pub object_replication_enabled: bool,
    pub peer_to_peer_disabled: bool,
    pub posting_enabled: bool,
    pub routing_enabled: bool,
    pub server_channel_enabled: bool,
    unique_nonce: Option<String>,
    posting_password_hash: Option<[u8; 32]>,
    posting_auth_hex: Option<String>,
    publish_password_hash: Option<[u8; 32]>,
    publish_auth_hex: Option<String>,
    bootstrap_specs: Vec<String>,
    multicast_specs: Vec<String>,
}

impl GroupSpecifierState {
    pub const MAX_SUPPORTED_VERSION: u32 = 2;

    pub fn new(name: &str, _counter: u64) -> Result<Self, String> {
        if name.is_empty() {
            return Err(
                "ArgumentError: Error #2004: GroupSpecifier name must not be empty.".into(),
            );
        }
        Ok(Self {
            name: name.to_owned(),
            ip_multicast_member_updates_enabled: false,
            min_groupspec_version: 1,
            multicast_enabled: false,
            object_replication_enabled: false,
            peer_to_peer_disabled: false,
            posting_enabled: false,
            routing_enabled: false,
            server_channel_enabled: false,
            unique_nonce: None,
            posting_password_hash: None,
            posting_auth_hex: None,
            publish_password_hash: None,
            publish_auth_hex: None,
            bootstrap_specs: Vec::new(),
            multicast_specs: Vec::new(),
        })
    }

    pub fn set_min_version(&mut self, version: u32) -> Result<(), String> {
        if !(1..=Self::MAX_SUPPORTED_VERSION).contains(&version) {
            return Err("RangeError: minGroupspecVersion must be between 1 and 2.".into());
        }
        self.min_groupspec_version = version;
        Ok(())
    }

    pub fn make_unique(&mut self, entropy_counter: &mut u64) {
        *entropy_counter = entropy_counter.wrapping_add(1);
        let digest = sha256(format!("unique:{}:{}", self.name, *entropy_counter).as_bytes());
        self.unique_nonce = Some(hex_encode(&digest[..16]));
    }

    pub fn set_posting_password(&mut self, password: Option<&str>, salt: Option<&str>) {
        match password.filter(|p| !p.is_empty()) {
            Some(pw) => {
                let material = format!("post:{}:{}:{pw}", self.name, salt.unwrap_or(""));
                let token = sha256(format!("post-auth:{material}").as_bytes());
                let mut verify_in = Vec::from(&b"post-verify:"[..]);
                verify_in.extend_from_slice(&token[..16]);
                self.posting_password_hash = Some(sha256(&verify_in));
                self.posting_auth_hex = Some(encode_posting_authorization(&material));
            }
            None => {
                self.posting_password_hash = None;
                self.posting_auth_hex = None;
            }
        }
    }

    pub fn set_publish_password(&mut self, password: Option<&str>, salt: Option<&str>) {
        match password.filter(|p| !p.is_empty()) {
            Some(pw) => {
                let material = format!("pub:{}:{}:{pw}", self.name, salt.unwrap_or(""));
                let token = sha256(format!("pub-auth:{material}").as_bytes());
                let mut verify_in = Vec::from(&b"pub-verify:"[..]);
                verify_in.extend_from_slice(&token[..16]);
                self.publish_password_hash = Some(sha256(&verify_in));
                self.publish_auth_hex = Some(encode_publish_authorization(&material));
            }
            None => {
                self.publish_password_hash = None;
                self.publish_auth_hex = None;
            }
        }
    }

    pub fn add_bootstrap_peer(&mut self, peer_id: &str) -> Result<(), String> {
        let spec = encode_bootstrap_peer_id_spec(peer_id)?;
        if !self.bootstrap_specs.contains(&spec) {
            self.bootstrap_specs.push(spec);
        }
        Ok(())
    }

    pub fn add_ip_multicast_address(
        &mut self,
        address: &str,
        port: Option<u32>,
        source: Option<&str>,
    ) -> Result<(), String> {
        let spec = encode_ip_multicast_address_spec(address, port, source)?;
        if !self.multicast_specs.contains(&spec) {
            self.multicast_specs.push(spec);
        }
        Ok(())
    }

    pub fn authorizations(&self) -> String {
        let mut out = String::new();
        if let Some(ref pub_hex) = self.publish_auth_hex {
            out.push_str(pub_hex);
        }
        if let Some(ref post_hex) = self.posting_auth_hex {
            out.push_str(post_hex);
        }
        out
    }

    pub fn groupspec_without_authorizations(&self) -> String {
        let mut tlv = Vec::new();
        tlv.push(if self.min_groupspec_version >= 2 {
            0x02
        } else {
            0x01
        });
        encode_tlv(0x01, self.name.as_bytes(), &mut tlv);
        if let Some(ref nonce) = self.unique_nonce {
            encode_tlv(0x02, nonce.as_bytes(), &mut tlv);
        }
        let mut flags = 0u8;
        if self.posting_enabled {
            flags |= 1 << 0;
        }
        if self.multicast_enabled {
            flags |= 1 << 1;
        }
        if self.object_replication_enabled {
            flags |= 1 << 2;
        }
        if self.routing_enabled {
            flags |= 1 << 3;
        }
        if self.server_channel_enabled {
            flags |= 1 << 4;
        }
        if self.peer_to_peer_disabled {
            flags |= 1 << 5;
        }
        if self.ip_multicast_member_updates_enabled {
            flags |= 1 << 6;
        }
        encode_tlv(0x03, &[flags], &mut tlv);
        if let Some(ref h) = self.publish_password_hash {
            encode_tlv(0x05, &h[..16], &mut tlv);
        }
        if let Some(ref h) = self.posting_password_hash {
            encode_tlv(0x07, &h[..16], &mut tlv);
        }
        let mut hex = format!("G:{}", hex_encode(&tlv));
        for spec in &self.bootstrap_specs {
            hex.push_str(spec);
        }
        for spec in &self.multicast_specs {
            hex.push_str(spec);
        }
        hex
    }

    pub fn groupspec_with_authorizations(&self) -> String {
        format!(
            "{}{}",
            self.groupspec_without_authorizations(),
            self.authorizations()
        )
    }
}

pub fn peer_id_to_group_address(peer_id: &str) -> String {
    hex_encode(&sha256(format!("group-addr:{peer_id}").as_bytes()))
}

#[derive(Clone, Debug)]
pub struct ParsedGroupspec {
    pub canonical_base: String,
    pub posting_authorized: bool,
    pub publish_authorized: bool,
    pub flags: u8,
}

pub fn parse_groupspec_hex(groupspec: &str) -> ParsedGroupspec {
    let trimmed = groupspec.trim();
    let Some(hex_body) = trimmed.strip_prefix("G:") else {
        return ParsedGroupspec {
            canonical_base: trimmed.to_owned(),
            posting_authorized: true,
            publish_authorized: true,
            flags: 0xFF,
        };
    };
    let Ok(bytes) = hex_decode(hex_body) else {
        return ParsedGroupspec {
            canonical_base: trimmed.to_owned(),
            posting_authorized: true,
            publish_authorized: true,
            flags: 0xFF,
        };
    };
    if bytes.is_empty() {
        return ParsedGroupspec {
            canonical_base: trimmed.to_owned(),
            posting_authorized: true,
            publish_authorized: true,
            flags: 0,
        };
    }

    let mut base_bytes = vec![bytes[0]];
    let mut pos = 1usize;
    let mut flags = 0u8;
    let mut pub_hash: Option<Vec<u8>> = None;
    let mut post_hash: Option<Vec<u8>> = None;
    let mut pub_token: Option<Vec<u8>> = None;
    let mut post_token: Option<Vec<u8>> = None;

    while pos + 2 <= bytes.len() {
        let tag_start = pos;
        let tag = bytes[pos];
        let len_b = bytes[pos + 1];
        pos += 2;
        let len = if (len_b & 0x80) == 0 {
            len_b as usize
        } else {
            if pos >= bytes.len() {
                break;
            }
            let lo = bytes[pos] as usize;
            pos += 1;
            (((len_b & 0x7f) as usize) << 8) | lo
        };
        if pos + len > bytes.len() {
            break;
        }
        let payload = &bytes[pos..pos + len];
        pos += len;
        match tag {
            0x03 if !payload.is_empty() => {
                flags = payload[0];
                base_bytes.extend_from_slice(&bytes[tag_start..pos]);
            }
            0x05 => {
                pub_hash = Some(payload.to_vec());
                base_bytes.extend_from_slice(&bytes[tag_start..pos]);
            }
            0x07 => {
                post_hash = Some(payload.to_vec());
                base_bytes.extend_from_slice(&bytes[tag_start..pos]);
            }
            0x13 => {
                pub_token = Some(payload.to_vec());
            }
            0x15 => {
                post_token = Some(payload.to_vec());
            }
            _ => {
                base_bytes.extend_from_slice(&bytes[tag_start..pos]);
            }
        }
    }

    let publish_authorized = match pub_hash {
        None => true,
        Some(ref expected) => match pub_token {
            None => false,
            Some(ref tok) => {
                let mut verify_in = Vec::from(&b"pub-verify:"[..]);
                verify_in.extend_from_slice(tok);
                &sha256(&verify_in)[..expected.len().min(32)] == expected.as_slice()
            }
        },
    };
    let posting_authorized = match post_hash {
        None => true,
        Some(ref expected) => match post_token {
            None => false,
            Some(ref tok) => {
                let mut verify_in = Vec::from(&b"post-verify:"[..]);
                verify_in.extend_from_slice(tok);
                &sha256(&verify_in)[..expected.len().min(32)] == expected.as_slice()
            }
        },
    };

    ParsedGroupspec {
        canonical_base: format!("G:{}", hex_encode(&base_bytes)),
        posting_authorized,
        publish_authorized,
        flags,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetGroupInfoSnapshot {
    pub object_replication_receive_bytes_per_second: f64,
    pub object_replication_send_bytes_per_second: f64,
    pub posting_receive_control_bytes_per_second: f64,
    pub posting_receive_data_bytes_per_second: f64,
    pub posting_send_control_bytes_per_second: f64,
    pub posting_send_data_bytes_per_second: f64,
    pub routing_receive_bytes_per_second: f64,
    pub routing_send_bytes_per_second: f64,
}

#[derive(Clone, Debug)]
pub(super) struct NetGroupMember {
    pub(super) id: u32,
    pub(super) groupspec_base: String,
    pub(super) peer_id: String,
    pub(super) address: String,
    pub(super) posting_authorized: bool,
    pub(super) publish_authorized: bool,
    pub(super) receive_mode: String,
    pub(super) replication_strategy: String,
    pub(super) neighbors: BTreeSet<String>,
    pub(super) member_hints: BTreeSet<String>,
    pub(super) have_objects: BTreeSet<u64>,
    pub(super) want_objects: BTreeSet<u64>,
    pub(super) in_flight_wants: BTreeSet<u64>,
    pub(super) seen_posts: HashSet<String>,
    pub(super) info: NetGroupInfoSnapshot,
    pub(super) closed: bool,
}

// ---------------------------------------------------------------------------
// FLV Stream Parser & NetStream Metrics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetStreamInfoSnapshot {
    #[serde(rename = "SRTT")]
    pub srtt: f64,
    pub audio_buffer_byte_length: f64,
    pub audio_buffer_length: f64,
    pub audio_byte_count: f64,
    pub audio_bytes_per_second: f64,
    pub audio_loss_rate: f64,
    pub byte_count: f64,
    pub current_bytes_per_second: f64,
    pub data_buffer_byte_length: f64,
    pub data_buffer_length: f64,
    pub data_byte_count: f64,
    pub data_bytes_per_second: f64,
    pub dropped_frames: u32,
    pub is_live: bool,
    pub max_bytes_per_second: f64,
    pub meta_data: Option<Value>,
    pub playback_bytes_per_second: f64,
    pub resource_name: Option<String>,
    pub uri: Option<String>,
    pub video_buffer_byte_length: f64,
    pub video_buffer_length: f64,
    pub video_byte_count: f64,
    pub video_bytes_per_second: f64,
    pub video_loss_rate: f64,
    pub xmp_data: Option<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetStreamMulticastInfoSnapshot {
    pub bytes_pushed_from_peers: f64,
    pub bytes_pushed_to_peers: f64,
    pub bytes_received_from_ip_multicast: f64,
    pub bytes_received_from_server: f64,
    pub bytes_requested_by_peers: f64,
    pub bytes_requested_from_peers: f64,
    pub fragments_pushed_from_peers: f64,
    pub fragments_pushed_to_peers: f64,
    pub fragments_received_from_ip_multicast: f64,
    pub fragments_received_from_server: f64,
    pub fragments_requested_by_peers: f64,
    pub fragments_requested_from_peers: f64,
    pub receive_control_bytes_per_second: f64,
    pub receive_data_bytes_per_second: f64,
    pub receive_data_bytes_per_second_from_ip_multicast: f64,
    pub receive_data_bytes_per_second_from_server: f64,
    pub send_control_bytes_per_second: f64,
    pub send_control_bytes_per_second_to_server: f64,
    pub send_data_bytes_per_second: f64,
}

#[derive(Clone, Debug, Default)]
pub struct FlvStreamState {
    buffer: Vec<u8>,
    header_parsed: bool,
    pub video_width: u32,
    pub video_height: u32,
    pub decoded_frames: u32,
    pub time_seconds: f64,
    pub back_buffer_byte_length: f64,
    pub back_buffer_length: f64,
    pub info: NetStreamInfoSnapshot,
    pub multicast: NetStreamMulticastInfoSnapshot,
}

/// Minimum `ByteArray` length required for `ApplicationDomain.domainMemory`
/// per Adobe Alchemy / CrossBridge / FlasCC specification.
pub const MIN_DOMAIN_MEMORY_LENGTH: usize = 1024;

pub fn fast_mem_sxi1(val: i32) -> i32 {
    if (val & 1) != 0 { -1 } else { 0 }
}

pub fn fast_mem_sxi8(val: i32) -> i32 {
    (val as i8) as i32
}

pub fn fast_mem_sxi16(val: i32) -> i32 {
    (val as i16) as i32
}

pub fn fast_mem_read(mem: &[u8], op: &str, addr: usize) -> Result<f64, String> {
    if mem.len() < MIN_DOMAIN_MEMORY_LENGTH {
        return Err(
            "Error #1504: End of file or domainMemory smaller than MIN_DOMAIN_MEMORY_LENGTH."
                .into(),
        );
    }
    let width = match op {
        "li8" => 1,
        "li16" => 2,
        "li32" | "lf32" => 4,
        "lf64" => 8,
        _ => return Err(format!("Unsupported fast_mem_read opcode '{op}'")),
    };
    if addr.checked_add(width).is_none_or(|end| end > mem.len()) {
        return Err("RangeError: Error #1506: The specified range is invalid.".into());
    }
    let val = match op {
        "li8" => f64::from(mem[addr]),
        "li16" => f64::from(u16::from_le_bytes([mem[addr], mem[addr + 1]])),
        "li32" => f64::from(i32::from_le_bytes([
            mem[addr],
            mem[addr + 1],
            mem[addr + 2],
            mem[addr + 3],
        ])),
        "lf32" => f64::from(f32::from_le_bytes([
            mem[addr],
            mem[addr + 1],
            mem[addr + 2],
            mem[addr + 3],
        ])),
        "lf64" => f64::from_le_bytes([
            mem[addr],
            mem[addr + 1],
            mem[addr + 2],
            mem[addr + 3],
            mem[addr + 4],
            mem[addr + 5],
            mem[addr + 6],
            mem[addr + 7],
        ]),
        _ => 0.0,
    };
    Ok(val)
}

pub fn fast_mem_write(mem: &mut [u8], op: &str, addr: usize, value: f64) -> Result<(), String> {
    if mem.len() < MIN_DOMAIN_MEMORY_LENGTH {
        return Err(
            "Error #1504: End of file or domainMemory smaller than MIN_DOMAIN_MEMORY_LENGTH."
                .into(),
        );
    }
    let width = match op {
        "si8" => 1,
        "si16" => 2,
        "si32" | "sf32" => 4,
        "sf64" => 8,
        _ => return Err(format!("Unsupported fast_mem_write opcode '{op}'")),
    };
    if addr.checked_add(width).is_none_or(|end| end > mem.len()) {
        return Err("RangeError: Error #1506: The specified range is invalid.".into());
    }
    match op {
        "si8" => {
            mem[addr] = (value as i64) as u8;
        }
        "si16" => {
            let bytes = ((value as i64) as u16).to_le_bytes();
            mem[addr..addr + 2].copy_from_slice(&bytes);
        }
        "si32" => {
            let bytes = ((value as i64) as i32).to_le_bytes();
            mem[addr..addr + 4].copy_from_slice(&bytes);
        }
        "sf32" => {
            let bytes = (value as f32).to_le_bytes();
            mem[addr..addr + 4].copy_from_slice(&bytes);
        }
        "sf64" => {
            let bytes = value.to_le_bytes();
            mem[addr..addr + 8].copy_from_slice(&bytes);
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn parse_amf0_simple(data: &[u8], pos: &mut usize, depth: usize) -> Option<Value> {
    if depth > 16 || *pos >= data.len() {
        return None;
    }
    let marker = data[*pos];
    *pos += 1;
    match marker {
        0x00 => {
            if *pos + 8 > data.len() {
                return None;
            }
            let n = f64::from_be_bytes(data[*pos..*pos + 8].try_into().ok()?);
            *pos += 8;
            Some(if n.is_finite() { json!(n) } else { Value::Null })
        }
        0x01 => {
            if *pos >= data.len() {
                return None;
            }
            let b = data[*pos] != 0;
            *pos += 1;
            Some(Value::Bool(b))
        }
        0x02 => {
            if *pos + 2 > data.len() {
                return None;
            }
            let len = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
            *pos += 2;
            if *pos + len > data.len() {
                return None;
            }
            let s = String::from_utf8_lossy(&data[*pos..*pos + len]).into_owned();
            *pos += len;
            Some(Value::String(s))
        }
        0x03 | 0x08 => {
            if marker == 0x08 {
                if *pos + 4 > data.len() {
                    return None;
                }
                *pos += 4; // ECMA array count hint
            }
            let mut map = Map::new();
            while *pos + 2 <= data.len() {
                let klen = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
                *pos += 2;
                if klen == 0 && *pos < data.len() && data[*pos] == 0x09 {
                    *pos += 1;
                    break;
                }
                if *pos + klen > data.len() {
                    return None;
                }
                let key = String::from_utf8_lossy(&data[*pos..*pos + klen]).into_owned();
                *pos += klen;
                let val = parse_amf0_simple(data, pos, depth + 1)?;
                map.insert(key, val);
            }
            Some(Value::Object(map))
        }
        0x05 | 0x06 => Some(Value::Null),
        0x0a => {
            if *pos + 4 > data.len() {
                return None;
            }
            let count = u32::from_be_bytes(data[*pos..*pos + 4].try_into().ok()?) as usize;
            *pos += 4;
            let mut arr = Vec::with_capacity(count.min(256));
            for _ in 0..count.min(1024) {
                arr.push(parse_amf0_simple(data, pos, depth + 1)?);
            }
            Some(Value::Array(arr))
        }
        _ => None,
    }
}

impl FlvStreamState {
    pub fn apply_action(&mut self, action: &str) -> Result<Value, String> {
        match action {
            "resetBegin" => {
                self.buffer.clear();
                self.header_parsed = false;
                Ok(json!({ "status": "NetStream.Buffer.Flush" }))
            }
            "resetSeek" => {
                self.buffer.clear();
                // Keep header_parsed = true so tag-aligned appends can follow immediately after seek!
                self.header_parsed = true;
                Ok(json!({ "status": "NetStream.Seek.Notify" }))
            }
            "endSequence" => {
                self.buffer.clear();
                Ok(json!({
                    "status": "NetStream.Buffer.Flush",
                    "ended": true,
                    "playStatus": {
                        "code": "NetStream.Play.Complete",
                        "level": "status",
                        "bytes": self.info.byte_count,
                        "duration": self.info.video_buffer_length.max(self.info.audio_buffer_length),
                    }
                }))
            }
            _ => Err(format!(
                "ArgumentError: Invalid NetStreamAppendBytesAction '{action}'."
            )),
        }
    }

    pub fn append_bytes(&mut self, chunk: &[u8]) -> Value {
        self.info.byte_count += chunk.len() as f64;
        self.info.current_bytes_per_second = chunk.len() as f64;
        if self.info.current_bytes_per_second > self.info.max_bytes_per_second {
            self.info.max_bytes_per_second = self.info.current_bytes_per_second;
        }
        self.buffer.extend_from_slice(chunk);

        let mut callbacks = Vec::new();

        if !self.header_parsed {
            if self.buffer.len() >= 13 && &self.buffer[0..3] == b"FLV" {
                let data_offset = u32::from_be_bytes([
                    self.buffer[5],
                    self.buffer[6],
                    self.buffer[7],
                    self.buffer[8],
                ]) as usize;
                let start = data_offset.saturating_add(4); // skip header + PreviousTagSize0
                if self.buffer.len() >= start {
                    self.buffer.drain(0..start);
                    self.header_parsed = true;
                }
            } else if self.buffer.len() >= 11 && matches!(self.buffer[0], 8 | 9 | 18) {
                // Tag-only stream after RESET_SEEK
                self.header_parsed = true;
            }
        }

        while self.header_parsed && self.buffer.len() >= 11 {
            let tag_type = self.buffer[0] & 0x1f;
            let data_size = ((self.buffer[1] as usize) << 16)
                | ((self.buffer[2] as usize) << 8)
                | (self.buffer[3] as usize);
            let ts_low = ((self.buffer[4] as u32) << 16)
                | ((self.buffer[5] as u32) << 8)
                | (self.buffer[6] as u32);
            let ts_ext = (self.buffer[7] as u32) << 24;
            let timestamp = (ts_ext | ts_low) as f64;
            let total_tag_bytes = 11 + data_size + 4;
            if self.buffer.len() < 11 + data_size {
                break;
            }
            let payload = &self.buffer[11..11 + data_size];
            match tag_type {
                8 => {
                    self.info.audio_byte_count += data_size as f64;
                    self.info.audio_buffer_byte_length += data_size as f64;
                    self.info.audio_buffer_length =
                        (timestamp / 1000.0).max(self.info.audio_buffer_length);
                    self.time_seconds = (timestamp / 1000.0).max(self.time_seconds);
                    self.back_buffer_byte_length += data_size as f64;
                    self.back_buffer_length = (timestamp / 1000.0).max(self.back_buffer_length);
                }
                9 => {
                    self.info.video_byte_count += data_size as f64;
                    self.info.video_buffer_byte_length += data_size as f64;
                    self.info.video_buffer_length =
                        (timestamp / 1000.0).max(self.info.video_buffer_length);
                    self.time_seconds = (timestamp / 1000.0).max(self.time_seconds);
                    self.decoded_frames = self.decoded_frames.saturating_add(1);
                    if payload.len() >= 5 && (payload[0] & 0x0f) == 3 {
                        // Screen Video v1 header: 4-bit blockWidth, 12-bit imageWidth, 4-bit blockHeight, 12-bit imageHeight
                        let w = (((payload[1] as u32) & 0x0f) << 8) | (payload[2] as u32);
                        let h = (((payload[3] as u32) & 0x0f) << 8) | (payload[4] as u32);
                        if w > 0 && h > 0 {
                            self.video_width = w;
                            self.video_height = h;
                        }
                    }
                    if self.video_width == 0 {
                        self.video_width = 320;
                    }
                    if self.video_height == 0 {
                        self.video_height = 240;
                    }
                    self.back_buffer_byte_length += data_size as f64;
                    self.back_buffer_length = (timestamp / 1000.0).max(self.back_buffer_length);
                    if !payload.is_empty() && ((payload[0] >> 4) & 0x0f) == 1 {
                        callbacks.push(json!({
                            "handler": "onSeekPoint",
                            "timestamp": timestamp,
                            "info": {
                                "time": timestamp / 1000.0,
                                "offset": self.info.byte_count - (self.buffer.len() as f64),
                            },
                        }));
                    }
                }
                18 => {
                    self.info.data_byte_count += data_size as f64;
                    self.info.data_buffer_byte_length += data_size as f64;
                    self.time_seconds = (timestamp / 1000.0).max(self.time_seconds);
                    self.info.data_buffer_length =
                        (timestamp / 1000.0).max(self.info.data_buffer_length);
                    let mut pos = 0usize;
                    if let Some(Value::String(handler)) = parse_amf0_simple(payload, &mut pos, 0) {
                        let data_val = parse_amf0_simple(payload, &mut pos, 0).unwrap_or(json!({}));
                        if handler == "onMetaData" {
                            if let Some(w) = data_val.get("width").and_then(|v| v.as_f64())
                                && w > 0.0
                            {
                                self.video_width = w as u32;
                            }
                            if let Some(h) = data_val.get("height").and_then(|v| v.as_f64())
                                && h > 0.0
                            {
                                self.video_height = h as u32;
                            }
                            self.info.meta_data = Some(data_val.clone());
                        } else if handler == "onXMPData" {
                            self.info.xmp_data = Some(data_val.clone());
                        }
                        callbacks.push(json!({
                            "handler": handler,
                            "timestamp": timestamp,
                            "info": data_val,
                        }));
                    }
                }
                _ => {}
            }
            let consume = total_tag_bytes.min(self.buffer.len());
            self.buffer.drain(0..consume);
        }

        json!({
            "callbacks": callbacks,
            "info": self.info,
            "timeSeconds": self.time_seconds,
            "videoWidth": self.video_width,
            "videoHeight": self.video_height,
            "decodedFrames": self.decoded_frames,
            "backBufferLength": self.back_buffer_length,
            "backBufferByteLength": self.back_buffer_byte_length,
        })
    }

    pub fn multicast_op(&mut self, kind: &str, bytes: f64, fragments: f64) -> Value {
        let b = bytes.max(0.0);
        let f = fragments.max(1.0);
        let ctrl = (b * 0.05).clamp(8.0, 256.0);
        match kind {
            "push_to_peers" => {
                self.multicast.bytes_pushed_to_peers += b;
                self.multicast.fragments_pushed_to_peers += f;
                self.multicast.send_data_bytes_per_second += b;
                self.multicast.send_control_bytes_per_second += ctrl;
            }
            "push_from_peers" => {
                self.multicast.bytes_pushed_from_peers += b;
                self.multicast.fragments_pushed_from_peers += f;
                self.multicast.receive_data_bytes_per_second += b;
                self.multicast.receive_control_bytes_per_second += ctrl;
                self.info.byte_count += b;
                self.info.current_bytes_per_second = b;
                if b > self.info.max_bytes_per_second {
                    self.info.max_bytes_per_second = b;
                }
            }
            "request_by_peers" => {
                self.multicast.bytes_requested_by_peers += b;
                self.multicast.fragments_requested_by_peers += f;
                self.multicast.receive_control_bytes_per_second += ctrl;
            }
            "request_from_peers" => {
                self.multicast.bytes_requested_from_peers += b;
                self.multicast.fragments_requested_from_peers += f;
                self.multicast.send_control_bytes_per_second += ctrl;
            }
            "receive_from_server" => {
                self.multicast.bytes_received_from_server += b;
                self.multicast.fragments_received_from_server += f;
                self.multicast.receive_data_bytes_per_second += b;
                self.multicast.receive_data_bytes_per_second_from_server += b;
                self.multicast.send_control_bytes_per_second_to_server += ctrl;
                self.info.byte_count += b;
            }
            "receive_from_ip_multicast" => {
                self.multicast.bytes_received_from_ip_multicast += b;
                self.multicast.fragments_received_from_ip_multicast += f;
                self.multicast.receive_data_bytes_per_second += b;
                self.multicast
                    .receive_data_bytes_per_second_from_ip_multicast += b;
                self.info.byte_count += b;
            }
            _ => {}
        }
        json!({
            "multicastInfo": self.multicast,
            "info": self.info,
        })
    }
}

// ---------------------------------------------------------------------------
// LocalConnection & XMLSocket Framing
// ---------------------------------------------------------------------------

pub(super) const FORBIDDEN_LC_METHODS: &[&str] = &[
    "send",
    "connect",
    "close",
    "allowDomain",
    "allowInsecureDomain",
    "client",
    "domain",
    "isPerUser",
];

#[derive(Clone, Debug)]
pub(super) struct LocalConnectionEndpoint {
    pub(super) raw_name: String,
    pub(super) qualified_name: String,
    pub(super) domain: String,
    pub(super) is_per_user: bool,
    pub(super) allow_all: bool,
    pub(super) allowed_domains: HashSet<String>,
}

pub fn normalize_lc_name(name: &str, domain: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 64 || name.contains("::") {
        return Err(
            "ArgumentError: Error #2004: LocalConnection name must be 1..64 valid characters."
                .into(),
        );
    }
    if name.starts_with('_') {
        Ok(name.to_owned())
    } else if let Some((d, local)) = name.split_once(':') {
        if d.is_empty() || local.is_empty() {
            return Err("ArgumentError: Error #2004: Malformed domain:connectionName.".into());
        }
        Ok(format!("{d}:{local}"))
    } else {
        Ok(format!("{domain}:{name}"))
    }
}

#[derive(Clone, Debug, Default)]
pub struct XmlSocketFramer {
    pending: Vec<u8>,
}

impl XmlSocketFramer {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut frames = Vec::new();
        for &b in bytes {
            if b == 0 {
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                frames.push(text);
            } else {
                self.pending.push(b);
            }
        }
        frames
    }
}

// ---------------------------------------------------------------------------
// X.509 DER/PEM ASN.1 Certificate Parser & Loopback Certificate Generator
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X500DistinguishedNameData {
    pub common_name: Option<String>,
    pub organization_name: Option<String>,
    pub organizational_unit_name: Option<String>,
    pub locality_name: Option<String>,
    pub state_or_province_name: Option<String>,
    pub country_name: Option<String>,
}

impl X500DistinguishedNameData {
    pub fn formatted(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref cn) = self.common_name {
            parts.push(format!("CN={cn}"));
        }
        if let Some(ref ou) = self.organizational_unit_name {
            parts.push(format!("OU={ou}"));
        }
        if let Some(ref o) = self.organization_name {
            parts.push(format!("O={o}"));
        }
        if let Some(ref l) = self.locality_name {
            parts.push(format!("L={l}"));
        }
        if let Some(ref st) = self.state_or_province_name {
            parts.push(format!("ST={st}"));
        }
        if let Some(ref c) = self.country_name {
            parts.push(format!("C={c}"));
        }
        parts.join(", ")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct X509CertificateData {
    pub version: u32,
    pub serial_number: String,
    #[serde(rename = "signatureAlgorithmOID")]
    pub signature_algorithm_oid: String,
    pub signature_algorithm_params_base64: Option<String>,
    pub issuer: X500DistinguishedNameData,
    pub valid_not_before_ms: f64,
    pub valid_not_after_ms: f64,
    pub subject: X500DistinguishedNameData,
    #[serde(rename = "subjectPublicKeyAlgorithmOID")]
    pub subject_public_key_algorithm_oid: String,
    pub subject_public_key: String,
    #[serde(rename = "issuerUniqueID")]
    pub issuer_unique_id: Option<String>,
    #[serde(rename = "subjectUniqueID")]
    pub subject_unique_id: Option<String>,
    pub encoded_base64: String,
}

pub(super) fn read_asn1_tlv<'a>(data: &'a [u8], pos: &mut usize) -> Result<(u8, &'a [u8]), String> {
    if *pos + 2 > data.len() {
        return Err("Truncated ASN.1 TLV header".into());
    }
    let tag = data[*pos];
    let len_byte = data[*pos + 1];
    *pos += 2;
    let len = if (len_byte & 0x80) == 0 {
        len_byte as usize
    } else {
        let count = (len_byte & 0x7f) as usize;
        if count == 0 || count > 4 || *pos + count > data.len() {
            return Err("Invalid ASN.1 length".into());
        }
        let mut acc = 0usize;
        for _ in 0..count {
            acc = (acc << 8) | (data[*pos] as usize);
            *pos += 1;
        }
        acc
    };
    if *pos + len > data.len() {
        return Err("Truncated ASN.1 TLV body".into());
    }
    let slice = &data[*pos..*pos + len];
    *pos += len;
    Ok((tag, slice))
}

pub(super) fn decode_oid(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let first = bytes[0];
    let mut arcs = vec![(first / 40) as u64, (first % 40) as u64];
    let mut val = 0u64;
    for &b in &bytes[1..] {
        val = (val << 7) | ((b & 0x7f) as u64);
        if (b & 0x80) == 0 {
            arcs.push(val);
            val = 0;
        }
    }
    arcs.iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn parse_asn1_time_ms(tag: u8, bytes: &[u8]) -> Result<f64, String> {
    let s = std::str::from_utf8(bytes).map_err(|_| "Invalid ASN.1 time string")?;
    let (year, rest) = if tag == 0x17 {
        // UTCTime YYMMDDHHMMSSZ
        if s.len() < 10 {
            return Err("Short UTCTime".into());
        }
        let yy: i32 = s[0..2].parse().map_err(|_| "Invalid UTCTime year")?;
        let y = if yy >= 50 { 1900 + yy } else { 2000 + yy };
        (y, &s[2..])
    } else {
        // GeneralizedTime YYYYMMDDHHMMSSZ
        if s.len() < 12 {
            return Err("Short GeneralizedTime".into());
        }
        let y: i32 = s[0..4]
            .parse()
            .map_err(|_| "Invalid GeneralizedTime year")?;
        (y, &s[4..])
    };
    let month: u32 = rest[0..2].parse().map_err(|_| "Invalid month")?;
    let day: u32 = rest[2..4].parse().map_err(|_| "Invalid day")?;
    let hour: u32 = rest[4..6].parse().map_err(|_| "Invalid hour")?;
    let min: u32 = rest[6..8].parse().map_err(|_| "Invalid minute")?;
    let sec: u32 = if rest.len() >= 10 && rest[8..10].chars().all(|c| c.is_ascii_digit()) {
        rest[8..10].parse().unwrap_or(0)
    } else {
        0
    };
    // Howard Hinnant civil-from-days inverse: days_from_civil(year, month, day)
    let y = year as i64 - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = month as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + (day as u64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + (doe as i64) - 719468;
    let total_secs = days * 86_400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64);
    Ok((total_secs * 1000) as f64)
}

pub(super) fn parse_x500_name(seq: &[u8]) -> Result<X500DistinguishedNameData, String> {
    let mut dn = X500DistinguishedNameData::default();
    let mut pos = 0usize;
    while pos < seq.len() {
        let (_set_tag, rdn_set) = read_asn1_tlv(seq, &mut pos)?;
        let mut rpos = 0usize;
        while rpos < rdn_set.len() {
            let (_attr_tag, attr_seq) = read_asn1_tlv(rdn_set, &mut rpos)?;
            let mut apos = 0usize;
            let (oid_tag, oid_bytes) = read_asn1_tlv(attr_seq, &mut apos)?;
            if oid_tag != 0x06 {
                continue;
            }
            let oid = decode_oid(oid_bytes);
            let (_val_tag, val_bytes) = read_asn1_tlv(attr_seq, &mut apos)?;
            let text = String::from_utf8_lossy(val_bytes).into_owned();
            match oid.as_str() {
                "2.5.4.3" => dn.common_name = Some(text),
                "2.5.4.6" => dn.country_name = Some(text),
                "2.5.4.7" => dn.locality_name = Some(text),
                "2.5.4.8" => dn.state_or_province_name = Some(text),
                "2.5.4.10" => dn.organization_name = Some(text),
                "2.5.4.11" => dn.organizational_unit_name = Some(text),
                _ => {}
            }
        }
    }
    Ok(dn)
}

pub fn parse_x509_certificate(input: &[u8]) -> Result<X509CertificateData, String> {
    // Support PEM (`-----BEGIN CERTIFICATE-----`) or raw DER.
    let der: Vec<u8> = if let Ok(text) = std::str::from_utf8(input) {
        if let Some(start) = text.find("-----BEGIN CERTIFICATE-----") {
            let body_start = start + "-----BEGIN CERTIFICATE-----".len();
            let end = text[body_start..]
                .find("-----END CERTIFICATE-----")
                .ok_or_else(|| "Unterminated PEM certificate".to_owned())?;
            let b64: String = text[body_start..body_start + end]
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            STANDARD
                .decode(b64.as_bytes())
                .map_err(|_| "Invalid PEM base64".to_owned())?
        } else {
            input.to_vec()
        }
    } else {
        input.to_vec()
    };

    let mut root_pos = 0usize;
    let (cert_tag, cert_body) = read_asn1_tlv(&der, &mut root_pos)?;
    if cert_tag != 0x30 {
        return Err("X.509 certificate requires an outer ASN.1 SEQUENCE".into());
    }
    let mut cpos = 0usize;
    let (tbs_tag, tbs) = read_asn1_tlv(cert_body, &mut cpos)?;
    if tbs_tag != 0x30 {
        return Err("X.509 TBSCertificate requires an ASN.1 SEQUENCE".into());
    }
    let (_sig_alg_tag, outer_sig_alg) = read_asn1_tlv(cert_body, &mut cpos)?;
    let _sig_val = read_asn1_tlv(cert_body, &mut cpos)?;

    let mut tpos = 0usize;
    let mut version = 1u32;
    if tpos < tbs.len() && tbs[tpos] == 0xa0 {
        let (_, ver_wrapper) = read_asn1_tlv(tbs, &mut tpos)?;
        let mut vpos = 0usize;
        let (vtag, vbytes) = read_asn1_tlv(ver_wrapper, &mut vpos)?;
        if vtag == 0x02 && !vbytes.is_empty() {
            version = (vbytes[vbytes.len() - 1] as u32).saturating_add(1);
        }
    }
    let (ser_tag, ser_bytes) = read_asn1_tlv(tbs, &mut tpos)?;
    if ser_tag != 0x02 {
        return Err("X.509 serialNumber requires INTEGER".into());
    }
    let serial_number = hex_encode_upper(ser_bytes);

    let (_tbs_sig_tag, tbs_sig_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let sig_seq_to_read = if !tbs_sig_seq.is_empty() {
        tbs_sig_seq
    } else {
        outer_sig_alg
    };
    let mut spos = 0usize;
    let (_, sig_oid_bytes) = read_asn1_tlv(sig_seq_to_read, &mut spos)?;
    let signature_algorithm_oid = decode_oid(sig_oid_bytes);
    let signature_algorithm_params_base64 = if spos < sig_seq_to_read.len() {
        let start = spos;
        let (ptag, _) = read_asn1_tlv(sig_seq_to_read, &mut spos)?;
        if ptag == 0x05 {
            None // ASN.1 NULL params
        } else {
            Some(STANDARD.encode(&sig_seq_to_read[start..spos]))
        }
    } else {
        None
    };

    let (_, issuer_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let issuer = parse_x500_name(issuer_seq)?;

    let (_, validity_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let mut val_pos = 0usize;
    let (nb_tag, nb_bytes) = read_asn1_tlv(validity_seq, &mut val_pos)?;
    let (na_tag, na_bytes) = read_asn1_tlv(validity_seq, &mut val_pos)?;
    let valid_not_before_ms = parse_asn1_time_ms(nb_tag, nb_bytes)?;
    let valid_not_after_ms = parse_asn1_time_ms(na_tag, na_bytes)?;

    let (_, subject_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let subject = parse_x500_name(subject_seq)?;

    let (_, spki_seq) = read_asn1_tlv(tbs, &mut tpos)?;
    let mut kpos = 0usize;
    let (_, spki_alg_seq) = read_asn1_tlv(spki_seq, &mut kpos)?;
    let mut kapos = 0usize;
    let (_, spki_oid_bytes) = read_asn1_tlv(spki_alg_seq, &mut kapos)?;
    let subject_public_key_algorithm_oid = decode_oid(spki_oid_bytes);
    let (spk_tag, spk_bytes) = read_asn1_tlv(spki_seq, &mut kpos)?;
    let raw_pk = if spk_tag == 0x03 && !spk_bytes.is_empty() {
        &spk_bytes[1..]
    } else {
        spk_bytes
    };
    let subject_public_key = hex_encode_upper(raw_pk);

    let mut issuer_unique_id = None;
    let mut subject_unique_id = None;
    while tpos < tbs.len() {
        let (opt_tag, opt_bytes) = read_asn1_tlv(tbs, &mut tpos)?;
        if opt_tag == 0x81 {
            issuer_unique_id = Some(hex_encode_upper(opt_bytes));
        } else if opt_tag == 0x82 {
            subject_unique_id = Some(hex_encode_upper(opt_bytes));
        }
    }

    Ok(X509CertificateData {
        version,
        serial_number,
        signature_algorithm_oid,
        signature_algorithm_params_base64,
        issuer,
        valid_not_before_ms,
        valid_not_after_ms,
        subject,
        subject_public_key_algorithm_oid,
        subject_public_key,
        issuer_unique_id,
        subject_unique_id,
        encoded_base64: STANDARD.encode(&der),
    })
}

pub(super) fn asn1_wrap(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(tag);
    let len = body.len();
    if len < 0x80 {
        out.push(len as u8);
    } else if len <= 0xff {
        out.push(0x81);
        out.push(len as u8);
    } else {
        out.push(0x82);
        out.push(((len >> 8) & 0xff) as u8);
        out.push((len & 0xff) as u8);
    }
    out.extend_from_slice(body);
    out
}

pub(super) fn asn1_rdn(oid_bytes: &[u8], value: &str) -> Vec<u8> {
    let mut seq = asn1_wrap(0x06, oid_bytes);
    seq.extend_from_slice(&asn1_wrap(0x0c, value.as_bytes()));
    let attr = asn1_wrap(0x30, &seq);
    asn1_wrap(0x31, &attr)
}

/// Builds a structurally valid ASN.1 DER X.509 v3 certificate for `common_name`.
pub fn build_loopback_x509_der(common_name: &str) -> Vec<u8> {
    build_custom_x509_der(
        common_name,
        common_name,
        "20250101000000Z",
        "20350101000000Z",
    )
}

/// Builds an ASN.1 DER X.509 v3 certificate with custom subject CN, issuer CN, and GeneralizedTime validity window.
pub fn build_custom_x509_der(
    subject_cn: &str,
    issuer_cn: &str,
    not_before_gen: &str,
    not_after_gen: &str,
) -> Vec<u8> {
    // sha256WithRSAEncryption: 1.2.840.113549.1.1.11
    let sig_oid = [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
    // rsaEncryption: 1.2.840.113549.1.1.1
    let rsa_oid = [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];

    let mut sig_alg = asn1_wrap(0x06, &sig_oid);
    sig_alg.extend_from_slice(&[0x05, 0x00]);
    let sig_alg_seq = asn1_wrap(0x30, &sig_alg);

    let build_dn = |cn: &str| -> Vec<u8> {
        let mut name_body = Vec::new();
        name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x06], "US"));
        name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x08], "California"));
        name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x07], "San Francisco"));
        name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x0a], "NeoFlash Runtime"));
        name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x0b], "Security"));
        name_body.extend_from_slice(&asn1_rdn(&[0x55, 0x04, 0x03], cn));
        asn1_wrap(0x30, &name_body)
    };
    let issuer_seq = build_dn(issuer_cn);
    let subject_seq = build_dn(subject_cn);

    let mut validity_body = asn1_wrap(0x18, not_before_gen.as_bytes());
    validity_body.extend_from_slice(&asn1_wrap(0x18, not_after_gen.as_bytes()));
    let validity_seq = asn1_wrap(0x30, &validity_body);

    let mut rsa_alg = asn1_wrap(0x06, &rsa_oid);
    rsa_alg.extend_from_slice(&[0x05, 0x00]);
    let rsa_alg_seq = asn1_wrap(0x30, &rsa_alg);
    let pk_digest = sha256(format!("pk:{subject_cn}").as_bytes());
    let mut bit_str = vec![0x00];
    bit_str.extend_from_slice(&pk_digest);
    let mut spki_body = rsa_alg_seq;
    spki_body.extend_from_slice(&asn1_wrap(0x03, &bit_str));
    let spki_seq = asn1_wrap(0x30, &spki_body);

    let serial_digest = sha256(format!("serial:{subject_cn}:{issuer_cn}").as_bytes());
    let mut serial_bytes = vec![0x01];
    serial_bytes.extend_from_slice(&serial_digest[..7]);

    let mut tbs_body = Vec::new();
    tbs_body.extend_from_slice(&asn1_wrap(0xa0, &asn1_wrap(0x02, &[0x02]))); // v3
    tbs_body.extend_from_slice(&asn1_wrap(0x02, &serial_bytes));
    tbs_body.extend_from_slice(&sig_alg_seq);
    tbs_body.extend_from_slice(&issuer_seq);
    tbs_body.extend_from_slice(&validity_seq);
    tbs_body.extend_from_slice(&subject_seq);
    tbs_body.extend_from_slice(&spki_seq);
    let tbs_seq = asn1_wrap(0x30, &tbs_body);

    let sig_digest = sha256(&tbs_seq);
    let mut sig_bits = vec![0x00];
    sig_bits.extend_from_slice(&sig_digest);
    let mut cert_body = tbs_seq;
    cert_body.extend_from_slice(&sig_alg_seq);
    cert_body.extend_from_slice(&asn1_wrap(0x03, &sig_bits));
    asn1_wrap(0x30, &cert_body)
}

fn hostname_matches_pattern(host: &str, pattern: &str, exact_settings: bool) -> bool {
    let h = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    let p = pattern
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if p.is_empty() || h.is_empty() {
        return false;
    }
    if p == "*" || p == h {
        return true;
    }
    if let Some(suffix) = p.strip_prefix("*.") {
        return h == suffix || (h.ends_with(&format!(".{suffix}")) && h.len() > suffix.len() + 1);
    }
    if !exact_settings {
        return h.ends_with(&format!(".{p}")) || p.ends_with(&format!(".{h}"));
    }
    false
}

/// Extracts the normalized target hostname from a policy file URL (`http(s)://host:port/...` or `xmlsocket://host:port`).
pub fn extract_policy_target_host(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.to_ascii_lowercase().starts_with("data:") {
        return String::new();
    }
    let after_scheme = match trimmed.find("://") {
        Some(idx) => &trimmed[idx + 3..],
        None => return String::new(),
    };
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let host_port = match authority.rfind('@') {
        Some(at) => &authority[at + 1..],
        None => authority,
    };
    if let Some(rest) = host_port.strip_prefix('[')
        && let Some(close) = rest.find(']')
    {
        return rest[..close].trim().to_ascii_lowercase();
    }
    let host = match host_port.split_once(':') {
        Some((h, _)) => h,
        None => host_port,
    };
    host.trim().to_ascii_lowercase()
}

fn x509_hostname_matches_cn(host: &str, cn: &str) -> bool {
    let h = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    let p = cn.trim().to_ascii_lowercase();
    if p.is_empty() || h.is_empty() {
        return false;
    }
    if p == "*" || p == h {
        return true;
    }
    if let Some(suffix) = p.strip_prefix("*.") {
        return h.ends_with(&format!(".{suffix}")) && h.len() > suffix.len() + 1;
    }
    false
}

/// Validates an X.509 certificate against `host`, `now_ms`, and `addBinaryChainBuildingCertificate` anchors,
/// returning `(CertificateStatus, Option<X509CertificateData>)`.
pub fn verify_x509_certificate_chain(
    server_der: &[u8],
    host: &str,
    now_ms: f64,
    chain_certs: &[(Vec<u8>, bool)],
    os_verified_tls: bool,
) -> (String, Option<X509CertificateData>) {
    let Ok(cert) = parse_x509_certificate(server_der) else {
        return ("invalid".to_owned(), None);
    };
    if now_ms.is_finite() && now_ms > 0.0 {
        if now_ms < cert.valid_not_before_ms {
            return ("notYetValid".to_owned(), Some(cert));
        }
        if now_ms > cert.valid_not_after_ms {
            return ("expired".to_owned(), Some(cert));
        }
    }
    if let Some(ref cn) = cert.subject.common_name
        && !host.trim().is_empty()
        && !x509_hostname_matches_cn(host, cn)
    {
        return ("principalMismatch".to_owned(), Some(cert));
    }

    let issuer_fmt = cert.issuer.formatted();
    let mut matched_trusted = false;
    let mut matched_untrusted = false;
    for (ca_der, trusted) in chain_certs {
        if let Ok(ca_cert) = parse_x509_certificate(ca_der)
            && (ca_cert.encoded_base64 == cert.encoded_base64
                || ca_cert.subject.formatted() == issuer_fmt)
        {
            if *trusted {
                matched_trusted = true;
            } else {
                matched_untrusted = true;
            }
        }
    }
    if matched_trusted {
        return ("trusted".to_owned(), Some(cert));
    }
    if matched_untrusted {
        return ("untrustedSigners".to_owned(), Some(cert));
    }
    if os_verified_tls {
        return ("trusted".to_owned(), Some(cert));
    }
    ("untrustedSigners".to_owned(), Some(cert))
}

// ---------------------------------------------------------------------------
// Cross-Domain Policy XML Parser (`Security.loadPolicyFile`)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyAllowAccessRule {
    pub domain: String,
    pub to_ports: String,
    pub secure: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyAllowHeadersRule {
    pub domain: String,
    pub headers: String,
    pub secure: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrossDomainPolicyFile {
    pub site_control: String,
    pub allow_access: Vec<PolicyAllowAccessRule>,
    pub allow_headers: Vec<PolicyAllowHeadersRule>,
}

fn extract_xml_attr(tag_text: &str, attr_name: &str) -> Option<String> {
    let lower = tag_text.to_ascii_lowercase();
    let needle = format!("{}=", attr_name.to_ascii_lowercase());
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(&needle) {
        let idx = search_from + rel;
        let prev_ok = idx == 0 || lower.as_bytes()[idx - 1].is_ascii_whitespace();
        let val_start = idx + needle.len();
        if !prev_ok || val_start >= tag_text.len() {
            search_from = val_start;
            continue;
        }
        let rest = tag_text[val_start..].trim_start();
        let quote = rest.chars().next()?;
        if quote == '"' || quote == '\'' {
            let inner = &rest[1..];
            let end = inner.find(quote)?;
            return Some(inner[..end].trim().to_owned());
        }
        search_from = val_start;
    }
    None
}

pub fn parse_cross_domain_policy_xml(xml: &str) -> Result<CrossDomainPolicyFile, String> {
    let trimmed = xml.trim_start_matches('\u{feff}').trim();
    if !trimmed
        .to_ascii_lowercase()
        .contains("<cross-domain-policy")
    {
        return Err("SecurityError: Policy file lacks <cross-domain-policy> root element.".into());
    }
    let mut policy = CrossDomainPolicyFile {
        site_control: "master-only".to_owned(),
        allow_access: Vec::new(),
        allow_headers: Vec::new(),
    };
    let mut cursor = 0usize;
    while let Some(rel_lt) = trimmed[cursor..].find('<') {
        let lt = cursor + rel_lt;
        let Some(rel_gt) = trimmed[lt..].find('>') else {
            break;
        };
        let gt = lt + rel_gt;
        let tag = trimmed[lt + 1..gt].trim();
        cursor = gt + 1;
        if tag.starts_with('?') || tag.starts_with('!') || tag.starts_with('/') {
            continue;
        }
        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.starts_with("site-control") {
            if let Some(sc) = extract_xml_attr(tag, "permitted-cross-domain-policies") {
                policy.site_control = sc.to_ascii_lowercase();
            }
        } else if tag_lower.starts_with("allow-access-from") {
            if let Some(domain) = extract_xml_attr(tag, "domain")
                && !domain.is_empty()
            {
                let to_ports = extract_xml_attr(tag, "to-ports").unwrap_or_else(|| "*".to_owned());
                let secure = extract_xml_attr(tag, "secure")
                    .map(|s| !s.eq_ignore_ascii_case("false"))
                    .unwrap_or(true);
                policy.allow_access.push(PolicyAllowAccessRule {
                    domain,
                    to_ports,
                    secure,
                });
            }
        } else if tag_lower.starts_with("allow-http-request-headers-from")
            && let Some(domain) = extract_xml_attr(tag, "domain")
            && let Some(headers) = extract_xml_attr(tag, "headers")
            && !domain.is_empty()
            && !headers.is_empty()
        {
            let secure = extract_xml_attr(tag, "secure")
                .map(|s| !s.eq_ignore_ascii_case("false"))
                .unwrap_or(true);
            policy.allow_headers.push(PolicyAllowHeadersRule {
                domain,
                headers,
                secure,
            });
        }
    }
    if policy.site_control == "none" {
        policy.allow_access.clear();
        policy.allow_headers.clear();
    }
    Ok(policy)
}

pub fn port_matches_policy_spec(port: u16, spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed == "*" || trimmed.is_empty() {
        return true;
    }
    for part in trimmed.split(',') {
        let p = part.trim();
        if p == "*" {
            return true;
        }
        if let Some((lo_s, hi_s)) = p.split_once('-') {
            if let (Ok(lo), Ok(hi)) = (lo_s.trim().parse::<u16>(), hi_s.trim().parse::<u16>())
                && port >= lo
                && port <= hi
            {
                return true;
            }
        } else if let Ok(exact) = p.parse::<u16>()
            && port == exact
        {
            return true;
        }
    }
    false
}

pub fn policy_allows_access(
    policy: &CrossDomainPolicyFile,
    requester_domain: &str,
    port: Option<u16>,
    requester_is_secure: bool,
) -> bool {
    policy_allows_access_with_settings(policy, requester_domain, port, requester_is_secure, true)
}

pub fn policy_allows_access_with_settings(
    policy: &CrossDomainPolicyFile,
    requester_domain: &str,
    port: Option<u16>,
    requester_is_secure: bool,
    exact_settings: bool,
) -> bool {
    if policy.site_control == "none" {
        return false;
    }
    for rule in &policy.allow_access {
        if rule.secure && !requester_is_secure {
            continue;
        }
        if !hostname_matches_pattern(requester_domain, &rule.domain, exact_settings) {
            continue;
        }
        if let Some(p) = port
            && !port_matches_policy_spec(p, &rule.to_ports)
        {
            continue;
        }
        return true;
    }
    false
}
