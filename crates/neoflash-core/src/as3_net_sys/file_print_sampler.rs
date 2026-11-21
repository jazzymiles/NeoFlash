use super::sockets_conn::{FORBIDDEN_LC_METHODS, LocalConnectionEndpoint, NetGroupMember};
use super::*;

// ---------------------------------------------------------------------------
// DRM, Workers, Concurrency, Sampler & Telemetry State in As3NetSysKernel
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DrmVoucherRecord {
    server_url: String,
    license_id: String,
    policy_id: String,
    domain: String,
    voucher_start_ms: f64,
    voucher_end_ms: f64,
    offline_start_ms: f64,
    offline_end_ms: f64,
    window_period: u32,
    window_start_ms: f64,
    window_end_ms: f64,
    policies: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct WorkerRecord {
    id: u32,
    is_primordial: bool,
    state: String, // "new" | "running" | "terminated"
}

#[derive(Clone, Debug)]
struct ChannelRecord {
    id: u32,
    sender_id: u32,
    receiver_id: u32,
    state: String, // "open" | "closing" | "closed"
    queued_count: usize,
}

#[derive(Clone, Debug, Default)]
struct MutexRecord {
    owner_worker: Option<u32>,
    depth: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SamplerEntry {
    kind: String, // "sample" | "new" | "delete"
    time_us: f64,
    id: u64,
    size: u64,
    class_name: Option<String>,
    stack: Vec<(String, String, u32, u64)>,
}

#[derive(Clone, Debug)]
struct ReplicationTicket {
    requester_id: u32,
    provider_id: u32,
    index: u64,
}

fn parse_sampler_stack(
    req: &Value,
    default_fn: &str,
    default_file: &str,
) -> Vec<(String, String, u32, u64)> {
    if let Some(arr) = req.get("stack").and_then(Value::as_array) {
        let parsed: Vec<(String, String, u32, u64)> = arr
            .iter()
            .take(64)
            .filter_map(|item| {
                let a = item.as_array()?;
                let name = a
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or(default_fn)
                    .to_owned();
                let file = a
                    .get(1)
                    .and_then(Value::as_str)
                    .unwrap_or(default_file)
                    .to_owned();
                let line = a.get(2).and_then(Value::as_u64).unwrap_or(1) as u32;
                let sid = a.get(3).and_then(Value::as_u64).unwrap_or(1);
                Some((name, file, line, sid))
            })
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    vec![(default_fn.to_owned(), default_file.to_owned(), 1, 1)]
}

pub struct As3NetSysKernel {
    next_id: u32,
    next_replication_req_id: i64,
    rng_counter: u64,
    default_locale: String,
    currency_formatters: HashMap<u32, CurrencyFormatterState>,
    collators: HashMap<u32, CollatorState>,
    string_tools: HashMap<u32, StringToolsState>,
    group_specs: HashMap<u32, GroupSpecifierState>,
    net_groups: HashMap<u32, NetGroupMember>,
    replication_tickets: HashMap<i64, ReplicationTicket>,
    flv_streams: HashMap<u32, FlvStreamState>,
    local_connections: HashMap<u32, LocalConnectionEndpoint>,
    lc_by_name: HashMap<String, u32>,
    xml_framers: HashMap<u32, XmlSocketFramer>,
    drm_tokens: HashMap<(String, String), Vec<u8>>,
    drm_vouchers: HashMap<String, DrmVoucherRecord>,
    drm_device_groups: HashSet<String>,
    workers: HashMap<u32, WorkerRecord>,
    channels: HashMap<u32, ChannelRecord>,
    mutexes: HashMap<u32, MutexRecord>,
    conditions: HashMap<u32, u32>, // condition_id -> mutex_id
    sampling_active: bool,
    sampling_paused: bool,
    sample_internal_allocs: bool,
    samples: VecDeque<SamplerEntry>,
    invocation_counts: HashMap<String, u64>,
    telemetry_commands: HashSet<String>,
    telemetry_metrics: Vec<(String, Value)>,
    profiler_active: bool,
    redraw_regions_shown: bool,
    redraw_regions_color: u32,
    policy_files: Vec<(String, CrossDomainPolicyFile)>,
    disable_avm1_loading: bool,
    last_security_panel: Option<String>,
    system_paused: bool,
    last_exit_code: Option<u32>,
}

impl Default for As3NetSysKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl As3NetSysKernel {
    pub fn new() -> Self {
        let mut kernel = Self {
            next_id: 2,
            next_replication_req_id: 1,
            rng_counter: 0x9e37_79b9_7f4a_7c15,
            default_locale: "en-US".into(),
            currency_formatters: HashMap::new(),
            collators: HashMap::new(),
            string_tools: HashMap::new(),
            group_specs: HashMap::new(),
            net_groups: HashMap::new(),
            replication_tickets: HashMap::new(),
            flv_streams: HashMap::new(),
            local_connections: HashMap::new(),
            lc_by_name: HashMap::new(),
            xml_framers: HashMap::new(),
            drm_tokens: HashMap::new(),
            drm_vouchers: HashMap::new(),
            drm_device_groups: HashSet::new(),
            workers: HashMap::new(),
            channels: HashMap::new(),
            mutexes: HashMap::new(),
            conditions: HashMap::new(),
            sampling_active: false,
            sampling_paused: false,
            sample_internal_allocs: false,
            samples: VecDeque::new(),
            invocation_counts: HashMap::new(),
            telemetry_commands: HashSet::new(),
            telemetry_metrics: Vec::new(),
            profiler_active: false,
            redraw_regions_shown: false,
            redraw_regions_color: 0xff0000,
            policy_files: Vec::new(),
            disable_avm1_loading: false,
            last_security_panel: None,
            system_paused: false,
            last_exit_code: None,
        };
        kernel.workers.insert(
            1,
            WorkerRecord {
                id: 1,
                is_primordial: true,
                state: "running".into(),
            },
        );
        kernel
    }

    fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(2);
        id
    }

    pub fn reset(&mut self) {
        let locale = self.default_locale.clone();
        *self = Self::new();
        self.default_locale = locale;
    }

    pub fn random_bytes(&mut self, count: u32, entropy: &[u8]) -> Result<Vec<u8>, String> {
        generate_random_bytes(count, entropy, &mut self.rng_counter)
    }

    pub fn flv_append(&mut self, stream_id: u32, bytes: &[u8]) -> String {
        let state = self.flv_streams.entry(stream_id).or_default();
        state.append_bytes(bytes).to_string()
    }

    pub fn xml_socket_feed(&mut self, socket_id: u32, bytes: &[u8]) -> String {
        let framer = self.xml_framers.entry(socket_id).or_default();
        serde_json::to_string(&framer.feed(bytes)).unwrap_or_else(|_| "[]".into())
    }

    pub fn parse_x509(&self, bytes: &[u8]) -> Result<String, String> {
        let parsed = parse_x509_certificate(bytes)?;
        serde_json::to_string(&parsed).map_err(|e| e.to_string())
    }

    pub fn loopback_x509_der(&self, host: &str) -> Vec<u8> {
        build_loopback_x509_der(if host.is_empty() { "localhost" } else { host })
    }

    pub fn execute_json(&mut self, request_json: &str) -> Result<String, String> {
        let req: Value = serde_json::from_str(request_json)
            .map_err(|e| format!("Invalid As3NetSysKernel JSON request: {e}"))?;
        let op = req
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing 'op' in As3NetSysKernel request".to_owned())?;
        let resp = self.dispatch(op, &req)?;
        serde_json::to_string(&resp).map_err(|e| e.to_string())
    }

    fn recompute_net_group_ring(&mut self, groupspec_base: &str) {
        let mut active_addrs: Vec<(String, u32)> = self
            .net_groups
            .values()
            .filter(|m| !m.closed && m.groupspec_base == groupspec_base)
            .map(|m| (m.address.clone(), m.id))
            .collect();
        active_addrs.sort_by(|a, b| a.0.cmp(&b.0));
        let peers: Vec<String> = self
            .net_groups
            .values()
            .filter(|m| !m.closed && m.groupspec_base == groupspec_base)
            .map(|m| m.peer_id.clone())
            .collect();
        for (_, id) in &active_addrs {
            if let Some(member) = self.net_groups.get_mut(id) {
                for p in &peers {
                    if *p != member.peer_id {
                        member.neighbors.insert(p.clone());
                    }
                }
            }
        }
    }

    fn schedule_replication_requests(&mut self, groupspec_base: &str) -> Vec<Value> {
        let mut availability: BTreeMap<u64, Vec<u32>> = BTreeMap::new();
        let mut requester_ids = Vec::new();
        for m in self.net_groups.values() {
            if m.closed || m.groupspec_base != groupspec_base {
                continue;
            }
            for &idx in &m.have_objects {
                availability.entry(idx).or_default().push(m.id);
            }
            if !m.want_objects.is_empty() {
                requester_ids.push((m.id, m.replication_strategy.clone()));
            }
        }
        requester_ids.sort_by_key(|(id, _)| *id);

        let mut scheduled = Vec::new();
        for (req_member_id, strategy) in requester_ids {
            let pending_wants: Vec<u64> = match self.net_groups.get(&req_member_id) {
                Some(m) => m
                    .want_objects
                    .iter()
                    .copied()
                    .filter(|idx| !m.in_flight_wants.contains(idx) && !m.have_objects.contains(idx))
                    .collect(),
                None => continue,
            };
            let mut candidates: Vec<(u64, Vec<u32>)> = pending_wants
                .into_iter()
                .filter_map(|idx| {
                    let providers: Vec<u32> = availability
                        .get(&idx)?
                        .iter()
                        .copied()
                        .filter(|&pid| pid != req_member_id)
                        .collect();
                    (!providers.is_empty()).then_some((idx, providers))
                })
                .collect();
            if strategy == "rarestFirst" {
                candidates.sort_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| a.0.cmp(&b.0)));
            } else {
                candidates.sort_by_key(|(idx, _)| *idx);
            }
            for (idx, providers) in candidates {
                let provider_id = providers[0];
                let ticket_id = self.next_replication_req_id;
                self.next_replication_req_id = self.next_replication_req_id.wrapping_add(1).max(1);
                self.replication_tickets.insert(
                    ticket_id,
                    ReplicationTicket {
                        requester_id: req_member_id,
                        provider_id,
                        index: idx,
                    },
                );
                if let Some(m) = self.net_groups.get_mut(&req_member_id) {
                    m.in_flight_wants.insert(idx);
                }
                scheduled.push(json!({
                    "requestID": ticket_id,
                    "requesterId": req_member_id,
                    "providerId": provider_id,
                    "index": idx,
                }));
            }
        }
        scheduled
    }

    fn dispatch(&mut self, op: &str, req: &Value) -> Result<Value, String> {
        match op {
            "set_default_locale" => {
                if let Some(loc) = req.get("locale").and_then(Value::as_str)
                    && !loc.trim().is_empty()
                {
                    self.default_locale = loc.trim().to_owned();
                }
                Ok(json!({ "defaultLocale": self.default_locale }))
            }
            "available_locales" => Ok(json!(SUPPORTED_LOCALES)),
            "locale_parse" => {
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let parsed = parse_locale_id(name, &self.default_locale);
                Ok(serde_json::to_value(parsed).map_err(|e| e.to_string())?)
            }
            "locale_determine_preferred" => {
                let want: Vec<String> = req
                    .get("want")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let have: Vec<String> = req
                    .get("have")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let keyword = req
                    .get("keyword")
                    .and_then(Value::as_str)
                    .unwrap_or("userinterface");
                Ok(json!(determine_preferred_locales(&want, &have, keyword)))
            }
            // CurrencyFormatter
            "currency_new" => {
                let loc = req.get("locale").and_then(Value::as_str).ok_or_else(|| {
                    "TypeError: CurrencyFormatter locale must be non-null.".to_owned()
                })?;
                let id = self.alloc_id();
                let state = CurrencyFormatterState::new(loc, &self.default_locale);
                let snap = serde_json::to_value(&state).map_err(|e| e.to_string())?;
                self.currency_formatters.insert(id, state);
                Ok(json!({ "id": id, "state": snap }))
            }
            "currency_snapshot" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let st = self
                    .currency_formatters
                    .get(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                serde_json::to_value(st).map_err(|e| e.to_string())
            }
            "currency_set_prop" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let val = req.get("value").cloned().unwrap_or(Value::Null);
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                st.set_property(name, val)?;
                serde_json::to_value(&*st).map_err(|e| e.to_string())
            }
            "currency_set_currency" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let iso = req.get("iso").and_then(Value::as_str).unwrap_or("");
                let sym = req.get("symbol").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                st.set_currency(iso, sym)?;
                serde_json::to_value(&*st).map_err(|e| e.to_string())
            }
            "currency_is_safe" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let iso = req.get("iso").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                let safe = st.is_symbol_safe(iso)?;
                Ok(json!({ "safe": safe, "status": st.last_operation_status }))
            }
            "currency_format" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let val = match req.get("value") {
                    Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
                    Some(Value::String(s)) => match s.as_str() {
                        "Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        _ => f64::NAN,
                    },
                    _ => f64::NAN,
                };
                let with_sym = req
                    .get("withSymbol")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                let formatted = st.format(val, with_sym)?;
                Ok(json!({ "formatted": formatted, "status": st.last_operation_status }))
            }
            "currency_parse" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let input = req.get("input").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .currency_formatters
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown CurrencyFormatter handle".to_owned())?;
                let parsed = st.parse(input)?;
                Ok(json!({
                    "value": parsed["value"],
                    "currencyString": parsed["currencyString"],
                    "status": st.last_operation_status,
                }))
            }
            // Collator
            "collator_new" => {
                let loc = req.get("locale").and_then(Value::as_str).unwrap_or("");
                let mode = req.get("mode").and_then(Value::as_str).unwrap_or("sorting");
                let st = CollatorState::new(loc, mode, &self.default_locale)?;
                let id = self.alloc_id();
                let snap = serde_json::to_value(&st).map_err(|e| e.to_string())?;
                self.collators.insert(id, st);
                Ok(json!({ "id": id, "state": snap }))
            }
            "collator_snapshot" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let st = self
                    .collators
                    .get(&id)
                    .ok_or_else(|| "Unknown Collator handle".to_owned())?;
                serde_json::to_value(st).map_err(|e| e.to_string())
            }
            "collator_set_bool" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let val = req.get("value").and_then(Value::as_bool).ok_or_else(|| {
                    "TypeError: Collator boolean property requires Boolean.".to_owned()
                })?;
                let st = self
                    .collators
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown Collator handle".to_owned())?;
                st.set_bool(name, val)?;
                serde_json::to_value(&*st).map_err(|e| e.to_string())
            }
            "collator_compare" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let a = req.get("a").and_then(Value::as_str).unwrap_or("");
                let b = req.get("b").and_then(Value::as_str).unwrap_or("");
                let st = self
                    .collators
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown Collator handle".to_owned())?;
                let cmp = st.compare(a, b);
                Ok(json!({ "result": cmp, "equals": cmp == 0, "status": st.last_operation_status }))
            }
            // StringTools
            "string_tools_new" => {
                let loc = req.get("locale").and_then(Value::as_str).unwrap_or("");
                let st = StringToolsState::new(loc, &self.default_locale);
                let id = self.alloc_id();
                let snap = serde_json::to_value(&st).map_err(|e| e.to_string())?;
                self.string_tools.insert(id, st);
                Ok(json!({ "id": id, "state": snap }))
            }
            "string_tools_case" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let val = req.get("value").and_then(Value::as_str).unwrap_or("");
                let upper = req.get("upper").and_then(Value::as_bool).unwrap_or(false);
                let st = self
                    .string_tools
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown StringTools handle".to_owned())?;
                let out = if upper {
                    st.to_upper(val)
                } else {
                    st.to_lower(val)
                };
                Ok(json!({ "result": out, "status": st.last_operation_status }))
            }
            // GroupSpecifier
            "groupspec_new" => {
                let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                let st = GroupSpecifierState::new(name, self.rng_counter)?;
                let id = self.alloc_id();
                let snap = serde_json::to_value(&st).map_err(|e| e.to_string())?;
                self.group_specs.insert(id, st);
                Ok(json!({ "id": id, "state": snap }))
            }
            "groupspec_update" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                let mut rng = self.rng_counter;
                let st = self
                    .group_specs
                    .get_mut(&id)
                    .ok_or_else(|| "Unknown GroupSpecifier handle".to_owned())?;
                match action {
                    "set_bool" => {
                        let prop = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let v = req.get("value").and_then(Value::as_bool).unwrap_or(false);
                        match prop {
                            "ipMulticastMemberUpdatesEnabled" => {
                                st.ip_multicast_member_updates_enabled = v
                            }
                            "multicastEnabled" => st.multicast_enabled = v,
                            "objectReplicationEnabled" => st.object_replication_enabled = v,
                            "peerToPeerDisabled" => st.peer_to_peer_disabled = v,
                            "postingEnabled" => st.posting_enabled = v,
                            "routingEnabled" => st.routing_enabled = v,
                            "serverChannelEnabled" => st.server_channel_enabled = v,
                            _ => return Err(format!("Unknown GroupSpecifier property {prop}")),
                        }
                    }
                    "set_min_version" => {
                        let v = req.get("value").and_then(Value::as_u64).unwrap_or(0) as u32;
                        st.set_min_version(v)?;
                    }
                    "make_unique" => {
                        st.make_unique(&mut rng);
                    }
                    "set_posting_password" => {
                        st.set_posting_password(
                            req.get("password").and_then(Value::as_str),
                            req.get("salt").and_then(Value::as_str),
                        );
                    }
                    "set_publish_password" => {
                        st.set_publish_password(
                            req.get("password").and_then(Value::as_str),
                            req.get("salt").and_then(Value::as_str),
                        );
                    }
                    "add_bootstrap_peer" => {
                        st.add_bootstrap_peer(
                            req.get("peerID").and_then(Value::as_str).unwrap_or(""),
                        )?;
                    }
                    "add_ip_multicast_address" => {
                        let port = req.get("port").and_then(Value::as_u64).map(|p| p as u32);
                        st.add_ip_multicast_address(
                            req.get("address").and_then(Value::as_str).unwrap_or(""),
                            port,
                            req.get("source").and_then(Value::as_str),
                        )?;
                    }
                    _ => {}
                }
                self.rng_counter = rng;
                Ok(json!({
                    "state": st,
                    "authorizations": st.authorizations(),
                    "withoutAuth": st.groupspec_without_authorizations(),
                    "withAuth": st.groupspec_with_authorizations(),
                }))
            }
            "groupspec_encode_static" => {
                let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                let encoded = match kind {
                    "posting" => encode_posting_authorization(
                        req.get("password").and_then(Value::as_str).unwrap_or(""),
                    ),
                    "publish" => encode_publish_authorization(
                        req.get("password").and_then(Value::as_str).unwrap_or(""),
                    ),
                    "bootstrap" => encode_bootstrap_peer_id_spec(
                        req.get("peerID").and_then(Value::as_str).unwrap_or(""),
                    )?,
                    "multicast" => encode_ip_multicast_address_spec(
                        req.get("address").and_then(Value::as_str).unwrap_or(""),
                        req.get("port").and_then(Value::as_u64).map(|p| p as u32),
                        req.get("source").and_then(Value::as_str),
                    )?,
                    _ => return Err(format!("Unknown static groupspec encoder {kind}")),
                };
                Ok(json!({ "encoded": encoded }))
            }
            // Peer ID & NetGroup
            "net_alloc_peer" => {
                self.rng_counter = self.rng_counter.wrapping_add(1);
                let uri = req.get("uri").and_then(Value::as_str).unwrap_or("rtmfp:");
                let near_id = hex_encode(&sha256(
                    format!("near:{uri}:{}", self.rng_counter).as_bytes(),
                ));
                let far_id = hex_encode(&sha256(
                    format!("far:{uri}:{}", self.rng_counter).as_bytes(),
                ));
                let near_nonce =
                    hex_encode(&sha256(format!("nnonce:{}", self.rng_counter).as_bytes()));
                let far_nonce =
                    hex_encode(&sha256(format!("fnonce:{}", self.rng_counter).as_bytes()));
                Ok(json!({
                    "nearID": near_id,
                    "farID": far_id,
                    "nearNonce": near_nonce,
                    "farNonce": far_nonce,
                }))
            }
            "netgroup_new" => {
                let groupspec = req.get("groupspec").and_then(Value::as_str).unwrap_or("");
                if groupspec.trim().is_empty() {
                    return Err(
                        "ArgumentError: Error #2004: NetGroup groupspec must not be empty.".into(),
                    );
                }
                let peer_id = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                let peer_id = if peer_id.is_empty() {
                    self.rng_counter = self.rng_counter.wrapping_add(1);
                    hex_encode(&sha256(format!("ng-peer:{}", self.rng_counter).as_bytes()))
                } else {
                    peer_id.to_owned()
                };
                let parsed = parse_groupspec_hex(groupspec);
                let base = parsed.canonical_base;
                let id = self.alloc_id();
                let address = peer_id_to_group_address(&peer_id);
                let member = NetGroupMember {
                    id,
                    groupspec_base: base.clone(),
                    peer_id: peer_id.clone(),
                    address: address.clone(),
                    posting_authorized: parsed.posting_authorized,
                    publish_authorized: parsed.publish_authorized,
                    receive_mode: "exact".into(),
                    replication_strategy: "lowestFirst".into(),
                    neighbors: BTreeSet::new(),
                    member_hints: BTreeSet::new(),
                    have_objects: BTreeSet::new(),
                    want_objects: BTreeSet::new(),
                    in_flight_wants: BTreeSet::new(),
                    seen_posts: HashSet::new(),
                    info: NetGroupInfoSnapshot::default(),
                    closed: false,
                };
                self.net_groups.insert(id, member);
                self.recompute_net_group_ring(&base);
                let peer_ids: Vec<u32> = self
                    .net_groups
                    .values()
                    .filter(|m| !m.closed && m.id != id && m.groupspec_base == base)
                    .map(|m| m.id)
                    .collect();
                Ok(json!({
                    "id": id,
                    "peerID": peer_id,
                    "address": address,
                    "canonicalBase": base,
                    "postingAuthorized": parsed.posting_authorized,
                    "publishAuthorized": parsed.publish_authorized,
                    "existingGroupIds": peer_ids,
                }))
            }
            "netgroup_op" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "convert" => {
                        let pid = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        Ok(json!({ "address": peer_id_to_group_address(pid) }))
                    }
                    "snapshot" => {
                        let m = self
                            .net_groups
                            .get(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        let active_count = self
                            .net_groups
                            .values()
                            .filter(|x| !x.closed && x.groupspec_base == m.groupspec_base)
                            .count();
                        let neighbor_count = m.neighbors.len();
                        Ok(json!({
                            "estimatedMemberCount": (active_count.max(1 + neighbor_count)) as f64,
                            "neighborCount": neighbor_count as f64,
                            "localCoverageFrom": "0000000000000000000000000000000000000000000000000000000000000000",
                            "localCoverageTo": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                            "receiveMode": m.receive_mode,
                            "replicationStrategy": m.replication_strategy,
                            "info": m.info,
                            "closed": m.closed,
                            "postingAuthorized": m.posting_authorized,
                            "publishAuthorized": m.publish_authorized,
                        }))
                    }
                    "set_mode" => {
                        let prop = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let val = req.get("value").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        match (prop, val) {
                            ("receiveMode", "exact" | "nearest") => m.receive_mode = val.into(),
                            ("replicationStrategy", "lowestFirst" | "rarestFirst") => {
                                m.replication_strategy = val.into()
                            }
                            _ => {
                                return Err(format!(
                                    "ArgumentError: Error #2004: Invalid NetGroup.{prop} '{val}'."
                                ));
                            }
                        }
                        Ok(json!({ "ok": true }))
                    }
                    "add_neighbor" => {
                        let pid = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        if m.closed || pid.is_empty() {
                            return Ok(json!({ "added": false }));
                        }
                        Ok(json!({ "added": m.neighbors.insert(pid.to_owned()) }))
                    }
                    "add_member_hint" => {
                        let pid = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        if m.closed || pid.is_empty() {
                            return Ok(json!({ "added": false }));
                        }
                        Ok(json!({ "added": m.member_hints.insert(pid.to_owned()) }))
                    }
                    "have_want" => {
                        let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                        let start = req.get("start").and_then(Value::as_u64).unwrap_or(0);
                        let end = req.get("end").and_then(Value::as_u64).unwrap_or(0);
                        if end < start || end.saturating_sub(start) > 65536 {
                            return Err(
                                "RangeError: NetGroup object index range is invalid.".into()
                            );
                        }
                        let (base, have_count, want_count) = {
                            let m = self
                                .net_groups
                                .get_mut(&id)
                                .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                            for idx in start..=end {
                                match kind {
                                    "addHave" => {
                                        m.have_objects.insert(idx);
                                    }
                                    "removeHave" => {
                                        m.have_objects.remove(&idx);
                                    }
                                    "addWant" => {
                                        m.want_objects.insert(idx);
                                    }
                                    "removeWant" => {
                                        m.want_objects.remove(&idx);
                                        m.in_flight_wants.remove(&idx);
                                    }
                                    _ => {}
                                }
                            }
                            (
                                m.groupspec_base.clone(),
                                m.have_objects.len(),
                                m.want_objects.len(),
                            )
                        };
                        let fetch_requests = self.schedule_replication_requests(&base);
                        Ok(json!({
                            "haveCount": have_count,
                            "wantCount": want_count,
                            "fetchRequests": fetch_requests,
                        }))
                    }
                    "replication_reply" => {
                        let req_id = req.get("requestID").and_then(Value::as_i64).unwrap_or(0);
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as f64;
                        let deny = req.get("deny").and_then(Value::as_bool).unwrap_or(false);
                        if let Some(m) = self.net_groups.get_mut(&id)
                            && !deny
                        {
                            m.info.object_replication_send_bytes_per_second += bytes;
                        }
                        let mut requester_id = None;
                        let mut index = None;
                        if let Some(ticket) = self.replication_tickets.remove(&req_id)
                            && ticket.provider_id == id
                        {
                            requester_id = Some(ticket.requester_id);
                            index = Some(ticket.index);
                            if let Some(requester) = self.net_groups.get_mut(&ticket.requester_id) {
                                requester.in_flight_wants.remove(&ticket.index);
                                if !deny {
                                    requester.want_objects.remove(&ticket.index);
                                    requester.have_objects.insert(ticket.index);
                                    requester.info.object_replication_receive_bytes_per_second +=
                                        bytes;
                                }
                            }
                        }
                        Ok(json!({
                            "requestID": req_id,
                            "denied": deny,
                            "requesterId": requester_id,
                            "index": index,
                        }))
                    }
                    "ingest_remote" => {
                        let remote_kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(16) as f64;
                        let msg_id = req.get("messageID").and_then(Value::as_str).unwrap_or("");
                        let peer_id = req.get("peerID").and_then(Value::as_str).unwrap_or("");
                        let m = self
                            .net_groups
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                        if m.closed {
                            return Ok(json!({ "accepted": false }));
                        }
                        if !peer_id.is_empty() && peer_id != m.peer_id {
                            m.neighbors.insert(peer_id.to_owned());
                        }
                        let accepted = match remote_kind {
                            "post" => {
                                if !msg_id.is_empty() && m.seen_posts.insert(msg_id.to_owned()) {
                                    m.info.posting_receive_data_bytes_per_second += bytes;
                                    m.info.posting_receive_control_bytes_per_second += 8.0;
                                    true
                                } else {
                                    false
                                }
                            }
                            "route" => {
                                m.info.routing_receive_bytes_per_second += bytes;
                                true
                            }
                            "replication_result" => {
                                if let Some(idx) = req.get("index").and_then(Value::as_u64) {
                                    m.in_flight_wants.remove(&idx);
                                    if m.want_objects.remove(&idx) {
                                        m.have_objects.insert(idx);
                                        m.info.object_replication_receive_bytes_per_second += bytes;
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            }
                            "replication_send" => {
                                m.info.object_replication_send_bytes_per_second += bytes;
                                true
                            }
                            "neighbor_join" => true,
                            _ => false,
                        };
                        Ok(json!({ "accepted": accepted }))
                    }
                    "post" => {
                        let payload_hash = req
                            .get("payloadSummary")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(16) as f64;
                        self.rng_counter = self.rng_counter.wrapping_add(1);
                        let (base, closed, authorized) = {
                            let m = self
                                .net_groups
                                .get(&id)
                                .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                            (m.groupspec_base.clone(), m.closed, m.posting_authorized)
                        };
                        if closed || !authorized {
                            return Ok(json!({ "messageID": Value::Null, "recipients": [] }));
                        }
                        let msg_id = hex_encode(&sha256(
                            format!("post:{id}:{}:{payload_hash}", self.rng_counter).as_bytes(),
                        ));
                        if let Some(sender) = self.net_groups.get_mut(&id) {
                            sender.seen_posts.insert(msg_id.clone());
                            sender.info.posting_send_data_bytes_per_second += bytes;
                            sender.info.posting_send_control_bytes_per_second += 8.0;
                        }
                        let mut recipients = Vec::new();
                        for other in self.net_groups.values_mut() {
                            if !other.closed
                                && other.id != id
                                && other.groupspec_base == base
                                && other.seen_posts.insert(msg_id.clone())
                            {
                                other.info.posting_receive_data_bytes_per_second += bytes;
                                other.info.posting_receive_control_bytes_per_second += 8.0;
                                recipients.push(other.id);
                            }
                        }
                        Ok(json!({ "messageID": msg_id, "recipients": recipients }))
                    }
                    "route" => {
                        let mode = req.get("sendMode").and_then(Value::as_str).unwrap_or("all");
                        let target_addr = req
                            .get("groupAddress")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let bytes =
                            req.get("byteLength").and_then(Value::as_u64).unwrap_or(16) as f64;
                        if mode != "all"
                            && mode != "nearest"
                            && !matches!(mode, "nextIncreasing" | "nextDecreasing")
                        {
                            return Err("ArgumentError: Invalid NetGroupSendMode.".into());
                        }
                        let (base, closed, has_neighbors) = {
                            let m = self
                                .net_groups
                                .get(&id)
                                .ok_or_else(|| "Unknown NetGroup".to_owned())?;
                            (m.groupspec_base.clone(), m.closed, !m.neighbors.is_empty())
                        };
                        if closed {
                            return Ok(json!({ "result": "error", "recipients": [] }));
                        }
                        let mut peers: Vec<(u32, String)> = self
                            .net_groups
                            .values()
                            .filter(|x| !x.closed && x.id != id && x.groupspec_base == base)
                            .map(|x| (x.id, x.address.clone()))
                            .collect();
                        if peers.is_empty() {
                            if has_neighbors && let Some(sender) = self.net_groups.get_mut(&id) {
                                sender.info.routing_send_bytes_per_second += bytes;
                            }
                            return Ok(json!({
                                "result": if has_neighbors { "sent" } else { "noRoute" },
                                "recipients": []
                            }));
                        }
                        peers.sort_by(|a, b| a.1.cmp(&b.1));
                        let recipients: Vec<u32> = match mode {
                            "all" => peers.iter().map(|(pid, _)| *pid).collect(),
                            "nearest" => {
                                let chosen = peers
                                    .iter()
                                    .find(|(_, addr)| addr.as_str() >= target_addr)
                                    .unwrap_or(&peers[0]);
                                vec![chosen.0]
                            }
                            "nextDecreasing" => vec![peers.last().unwrap().0],
                            _ => vec![peers[0].0],
                        };
                        if let Some(sender) = self.net_groups.get_mut(&id) {
                            sender.info.routing_send_bytes_per_second += bytes;
                        }
                        for rid in &recipients {
                            if let Some(r) = self.net_groups.get_mut(rid) {
                                r.info.routing_receive_bytes_per_second += bytes;
                            }
                        }
                        Ok(json!({ "result": "sent", "recipients": recipients }))
                    }
                    "close" => {
                        let base = if let Some(m) = self.net_groups.get_mut(&id) {
                            m.closed = true;
                            m.neighbors.clear();
                            Some(m.groupspec_base.clone())
                        } else {
                            None
                        };
                        if let Some(b) = base {
                            self.recompute_net_group_ring(&b);
                        }
                        Ok(json!({ "closed": true }))
                    }
                    _ => Err(format!("Unknown netgroup_op {action}")),
                }
            }
            // FLV stream actions & info
            "flv_action" => {
                let id = req.get("streamId").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                let st = self.flv_streams.entry(id).or_default();
                st.apply_action(action)
            }
            "flv_info" => {
                let id = req.get("streamId").and_then(Value::as_u64).unwrap_or(0) as u32;
                let st = self.flv_streams.entry(id).or_default();
                Ok(json!({
                    "info": st.info,
                    "multicastInfo": st.multicast,
                    "backBufferLength": st.back_buffer_length,
                    "backBufferByteLength": st.back_buffer_byte_length,
                }))
            }
            "flv_multicast_op" => {
                let id = req.get("streamId").and_then(Value::as_u64).unwrap_or(0) as u32;
                let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                let bytes = req.get("bytes").and_then(Value::as_f64).unwrap_or(0.0);
                let fragments = req.get("fragments").and_then(Value::as_f64).unwrap_or(1.0);
                let st = self.flv_streams.entry(id).or_default();
                Ok(st.multicast_op(kind, bytes, fragments))
            }
            // LocalConnection
            "lc_new" => {
                let domain = req
                    .get("domain")
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())
                    .unwrap_or("localhost")
                    .to_owned();
                let id = self.alloc_id();
                self.local_connections.insert(
                    id,
                    LocalConnectionEndpoint {
                        raw_name: String::new(),
                        qualified_name: String::new(),
                        domain: domain.clone(),
                        is_per_user: true,
                        allow_all: false,
                        allowed_domains: HashSet::from([domain.clone()]),
                    },
                );
                Ok(json!({ "id": id, "domain": domain }))
            }
            "lc_op" => {
                let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "set_per_user" => {
                        let v = req.get("value").and_then(Value::as_bool).unwrap_or(true);
                        let ep = self
                            .local_connections
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                        ep.is_per_user = v;
                        Ok(json!({ "isPerUser": ep.is_per_user }))
                    }
                    "allow_domain" => {
                        let domains: Vec<String> = req
                            .get("domains")
                            .and_then(Value::as_array)
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(Value::as_str)
                                    .map(ToOwned::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let ep = self
                            .local_connections
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                        for d in domains {
                            if d == "*" {
                                ep.allow_all = true;
                            } else if !d.is_empty() {
                                ep.allowed_domains.insert(d);
                            }
                        }
                        Ok(json!({ "ok": true }))
                    }
                    "connect" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        if name.contains(':') {
                            return Err("ArgumentError: Error #2004: LocalConnection.connect name cannot contain a colon.".into());
                        }
                        let (domain, already) = {
                            let ep = self
                                .local_connections
                                .get(&id)
                                .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                            (ep.domain.clone(), !ep.qualified_name.is_empty())
                        };
                        if already {
                            return Err("ArgumentError: Error #2082: Connect failed because the object is already connected.".into());
                        }
                        let qname = normalize_lc_name(name, &domain)?;
                        if self.lc_by_name.contains_key(&qname) {
                            return Err("ArgumentError: Error #2082: Connect failed because the connection name is already in use.".into());
                        }
                        self.lc_by_name.insert(qname.clone(), id);
                        if let Some(ep) = self.local_connections.get_mut(&id) {
                            ep.raw_name = name.to_owned();
                            ep.qualified_name = qname.clone();
                        }
                        Ok(json!({ "qualifiedName": qname }))
                    }
                    "close" => {
                        let ep = self
                            .local_connections
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown LocalConnection".to_owned())?;
                        if ep.qualified_name.is_empty() {
                            return Err("ArgumentError: Error #2083: Close failed because the object is not connected.".into());
                        }
                        let old = std::mem::take(&mut ep.qualified_name);
                        ep.raw_name.clear();
                        self.lc_by_name.remove(&old);
                        Ok(json!({ "closed": true }))
                    }
                    "resolve_send" => {
                        let target_name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
                        if target_name.is_empty()
                            || method.is_empty()
                            || FORBIDDEN_LC_METHODS.contains(&method)
                        {
                            return Err("ArgumentError: Error #2004: Invalid LocalConnection target or method name.".into());
                        }
                        let sender_domain = self
                            .local_connections
                            .get(&id)
                            .map(|ep| ep.domain.clone())
                            .unwrap_or_else(|| "localhost".into());
                        let qname = normalize_lc_name(target_name, &sender_domain)?;
                        if let Some(&target_id) = self.lc_by_name.get(&qname)
                            && let Some(target_ep) = self.local_connections.get(&target_id)
                        {
                            let allowed = target_ep.allow_all
                                || target_ep.domain == sender_domain
                                || target_ep.allowed_domains.contains(&sender_domain);
                            if !allowed {
                                return Ok(json!({
                                    "found": false,
                                    "securityDenied": true,
                                }));
                            }
                            return Ok(json!({
                                "found": true,
                                "targetId": target_id,
                                "qualifiedName": qname,
                            }));
                        }
                        Ok(json!({
                            "found": false,
                            "securityDenied": false,
                            "qualifiedName": qname,
                        }))
                    }
                    _ => Err(format!("Unknown lc_op {action}")),
                }
            }
            // DRM
            "drm_parse_content" => {
                let raw_b64 = req.get("base64").and_then(Value::as_str).unwrap_or("");
                let raw = if raw_b64.is_empty() {
                    Vec::new()
                } else {
                    STANDARD.decode(raw_b64.as_bytes()).unwrap_or_default()
                };
                if !raw.is_empty() {
                    if let Ok(val) = serde_json::from_slice::<Value>(&raw)
                        && let Some(obj) = val.as_object()
                    {
                        let server = obj
                            .get("serverURL")
                            .and_then(Value::as_str)
                            .unwrap_or("https://drm.neoflash.local/license");
                        let license = obj
                            .get("licenseID")
                            .and_then(Value::as_str)
                            .unwrap_or("nf-license-default");
                        let domain = obj
                            .get("domain")
                            .and_then(Value::as_str)
                            .unwrap_or("default");
                        let auth = obj
                            .get("authenticationMethod")
                            .and_then(Value::as_str)
                            .unwrap_or("anonymous");
                        let policy = obj
                            .get("policyID")
                            .and_then(Value::as_str)
                            .unwrap_or("policy-standard");
                        let display_name = obj
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or("Standard Access");
                        return Ok(json!({
                            "serverURL": server,
                            "licenseID": license,
                            "domain": domain,
                            "authenticationMethod": auth,
                            "accessInfo": [{
                                "authenticationMethod": auth,
                                "displayName": display_name,
                                "domain": domain,
                                "policyID": policy,
                            }]
                        }));
                    }
                    // Arbitrary non-JSON raw header bytes: derive deterministic licenseID.
                    if raw.len() < 4 {
                        return Err(
                            "ArgumentError: Error #3300: Invalid DRMContentData raw metadata."
                                .into(),
                        );
                    }
                    let lic = format!("lic-{}", hex_encode(&sha256(&raw)[..8]));
                    return Ok(json!({
                        "serverURL": "https://drm.neoflash.local/license",
                        "licenseID": lic,
                        "domain": "default",
                        "authenticationMethod": "anonymous",
                        "accessInfo": [{
                            "authenticationMethod": "anonymous",
                            "displayName": "Default Access",
                            "domain": "default",
                            "policyID": "policy-default",
                        }]
                    }));
                }
                Ok(json!({
                    "serverURL": "https://drm.neoflash.local/license",
                    "licenseID": "lic-default",
                    "domain": "default",
                    "authenticationMethod": "anonymous",
                    "accessInfo": [{
                        "authenticationMethod": "anonymous",
                        "displayName": "Default Access",
                        "domain": "default",
                        "policyID": "policy-default",
                    }]
                }))
            }
            "drm_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "authenticate" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let domain = req.get("domain").and_then(Value::as_str).unwrap_or("");
                        let user = req.get("username").and_then(Value::as_str).unwrap_or("");
                        let pass = req.get("password").and_then(Value::as_str).unwrap_or("");
                        if server.is_empty() || user.is_empty() {
                            return Ok(json!({ "ok": false, "errorID": 3315, "subErrorID": 401 }));
                        }
                        let token =
                            sha256(format!("drm-tok:{server}:{domain}:{user}:{pass}").as_bytes())
                                .to_vec();
                        self.drm_tokens
                            .insert((server.to_owned(), domain.to_owned()), token.clone());
                        Ok(json!({
                            "ok": true,
                            "tokenBase64": STANDARD.encode(&token),
                        }))
                    }
                    "set_token" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let domain = req.get("domain").and_then(Value::as_str).unwrap_or("");
                        let b64 = req.get("tokenBase64").and_then(Value::as_str).unwrap_or("");
                        let bytes = STANDARD.decode(b64.as_bytes()).unwrap_or_default();
                        self.drm_tokens
                            .insert((server.to_owned(), domain.to_owned()), bytes);
                        Ok(json!({ "ok": true }))
                    }
                    "load_voucher" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let license = req.get("licenseID").and_then(Value::as_str).unwrap_or("");
                        let policy = req
                            .get("policyID")
                            .and_then(Value::as_str)
                            .unwrap_or("policy-default");
                        let domain = req
                            .get("domain")
                            .and_then(Value::as_str)
                            .unwrap_or("default");
                        let auth_method = req
                            .get("authenticationMethod")
                            .and_then(Value::as_str)
                            .unwrap_or("anonymous");
                        let setting = req
                            .get("setting")
                            .and_then(Value::as_str)
                            .unwrap_or("allowServer");
                        let preview = req.get("preview").and_then(Value::as_bool).unwrap_or(false);
                        let now_ms = req
                            .get("nowMs")
                            .and_then(Value::as_f64)
                            .unwrap_or(1_762_000_000_000.0);
                        if !preview
                            && !matches!(setting, "allowServer" | "forceRefresh" | "localOnly")
                        {
                            return Err(
                                "ArgumentError: Error #2004: Invalid LoadVoucherSetting.".into()
                            );
                        }
                        let key = format!("{server}|{license}");
                        if !preview
                            && setting != "forceRefresh"
                            && let Some(existing) = self.drm_vouchers.get(&key)
                        {
                            return Ok(json!({ "ok": true, "isLocal": true, "voucher": existing }));
                        }
                        if !preview && setting == "localOnly" {
                            return Ok(json!({ "ok": false, "voucher": Value::Null }));
                        }
                        if auth_method == "usernameAndPassword"
                            && !self
                                .drm_tokens
                                .contains_key(&(server.to_owned(), domain.to_owned()))
                        {
                            return Ok(json!({
                                "ok": false,
                                "error": true,
                                "errorID": 3305,
                                "subErrorID": 401,
                                "text": "Authentication required before loading voucher."
                            }));
                        }
                        let mut policies = BTreeMap::new();
                        policies.insert("preview".into(), preview.to_string());
                        policies.insert("domain".into(), domain.to_owned());
                        let voucher = DrmVoucherRecord {
                            server_url: server.to_owned(),
                            license_id: license.to_owned(),
                            policy_id: policy.to_owned(),
                            domain: domain.to_owned(),
                            voucher_start_ms: now_ms,
                            voucher_end_ms: now_ms + 86_400_000.0 * 30.0,
                            offline_start_ms: now_ms,
                            offline_end_ms: now_ms + 86_400_000.0 * 7.0,
                            window_period: 86_400,
                            window_start_ms: now_ms,
                            window_end_ms: now_ms + 86_400_000.0,
                            policies,
                        };
                        if !preview {
                            self.drm_vouchers.insert(key, voucher.clone());
                        }
                        Ok(json!({ "ok": true, "isLocal": false, "voucher": voucher }))
                    }
                    "store_voucher" => {
                        let b64 = req.get("base64").and_then(Value::as_str).unwrap_or("");
                        let raw = STANDARD.decode(b64.as_bytes()).unwrap_or_default();
                        let now_ms = req
                            .get("nowMs")
                            .and_then(Value::as_f64)
                            .unwrap_or(1_762_000_000_000.0);
                        let record: DrmVoucherRecord =
                            serde_json::from_slice(&raw).unwrap_or_else(|_| DrmVoucherRecord {
                                server_url: "https://drm.neoflash.local/license".into(),
                                license_id: format!("stored-{}", hex_encode(&sha256(&raw)[..6])),
                                policy_id: "policy-stored".into(),
                                domain: "default".into(),
                                voucher_start_ms: now_ms,
                                voucher_end_ms: now_ms + 86_400_000.0 * 30.0,
                                offline_start_ms: now_ms,
                                offline_end_ms: now_ms + 86_400_000.0 * 7.0,
                                window_period: 86_400,
                                window_start_ms: now_ms,
                                window_end_ms: now_ms + 86_400_000.0,
                                policies: BTreeMap::new(),
                            });
                        let key = format!("{}|{}", record.server_url, record.license_id);
                        self.drm_vouchers.insert(key, record.clone());
                        Ok(json!({ "ok": true, "voucher": record }))
                    }
                    "return_voucher" => {
                        let server = req.get("serverURL").and_then(Value::as_str).unwrap_or("");
                        let license = req.get("licenseID").and_then(Value::as_str).unwrap_or("");
                        let mut removed = 0usize;
                        if !license.is_empty() {
                            if self
                                .drm_vouchers
                                .remove(&format!("{server}|{license}"))
                                .is_some()
                            {
                                removed += 1;
                            }
                        } else {
                            let before = self.drm_vouchers.len();
                            self.drm_vouchers.retain(|_, v| v.server_url != server);
                            removed = before - self.drm_vouchers.len();
                        }
                        Ok(json!({ "ok": true, "removed": removed }))
                    }
                    "reset_vouchers" => {
                        self.drm_vouchers.clear();
                        self.drm_tokens.clear();
                        Ok(json!({ "ok": true }))
                    }
                    "device_group" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        let remove = req.get("remove").and_then(Value::as_bool).unwrap_or(false);
                        if remove {
                            self.drm_device_groups.remove(name);
                        } else {
                            self.drm_device_groups.insert(name.to_owned());
                        }
                        Ok(json!({ "ok": true, "joined": !remove }))
                    }
                    _ => Err(format!("Unknown drm_op {action}")),
                }
            }
            // Capabilities
            "capabilities_snapshot" => {
                let width = req.get("width").and_then(Value::as_u64).unwrap_or(1920) as u32;
                let height = req.get("height").and_then(Value::as_u64).unwrap_or(1080) as u32;
                let lang = req.get("language").and_then(Value::as_str).unwrap_or("en");
                let os = req
                    .get("os")
                    .and_then(Value::as_str)
                    .unwrap_or("Mac OS 10.15.7");
                let dpi = req.get("dpi").and_then(Value::as_u64).unwrap_or(72) as u32;
                let debugger = req
                    .get("isDebugger")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(build_capabilities_snapshot(
                    width, height, lang, os, dpi, debugger,
                ))
            }
            // Workers & MessageChannel
            "worker_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "create_worker" => {
                        let id = self.alloc_id();
                        self.workers.insert(
                            id,
                            WorkerRecord {
                                id,
                                is_primordial: false,
                                state: "new".into(),
                            },
                        );
                        Ok(json!({ "id": id, "state": "new", "isPrimordial": false }))
                    }
                    "start" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let w = self
                            .workers
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Worker".to_owned())?;
                        if w.is_primordial || w.state != "new" {
                            return Err("IllegalOperationError: Error #3716: Worker.start requires a Worker in WorkerState.NEW.".into());
                        }
                        w.state = "running".into();
                        Ok(json!({ "state": w.state }))
                    }
                    "terminate" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let w = self
                            .workers
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Worker".to_owned())?;
                        if w.is_primordial || w.state == "terminated" {
                            return Ok(json!({ "terminated": false, "state": w.state }));
                        }
                        w.state = "terminated".into();
                        Ok(json!({ "terminated": true, "state": w.state }))
                    }
                    "list_running" => {
                        let mut ids: Vec<u32> = self
                            .workers
                            .values()
                            .filter(|w| w.state == "running")
                            .map(|w| w.id)
                            .collect();
                        ids.sort_unstable();
                        Ok(json!({ "workerIds": ids }))
                    }
                    "create_channel" => {
                        let sender =
                            req.get("senderId").and_then(Value::as_u64).unwrap_or(1) as u32;
                        let receiver =
                            req.get("receiverId").and_then(Value::as_u64).unwrap_or(1) as u32;
                        if !self.workers.contains_key(&sender)
                            || !self.workers.contains_key(&receiver)
                        {
                            return Err(
                                "ArgumentError: Invalid Worker for createMessageChannel.".into()
                            );
                        }
                        let id = self.alloc_id();
                        self.channels.insert(
                            id,
                            ChannelRecord {
                                id,
                                sender_id: sender,
                                receiver_id: receiver,
                                state: "open".into(),
                                queued_count: 0,
                            },
                        );
                        Ok(json!({ "id": id, "state": "open" }))
                    }
                    "channel_send" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let limit = req.get("queueLimit").and_then(Value::as_i64).unwrap_or(-1);
                        let ch = self
                            .channels
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown MessageChannel".to_owned())?;
                        let _ = (ch.id, ch.sender_id, ch.receiver_id);
                        if ch.state != "open" {
                            return Err("IOError: MessageChannel is closed.".into());
                        }
                        if limit >= 0 && (ch.queued_count as i64) >= limit {
                            return Err("MemoryError: MessageChannel queueLimit exceeded.".into());
                        }
                        ch.queued_count += 1;
                        Ok(json!({ "queued": ch.queued_count, "state": ch.state }))
                    }
                    "channel_receive" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let ch = self
                            .channels
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown MessageChannel".to_owned())?;
                        if ch.queued_count > 0 {
                            ch.queued_count -= 1;
                        }
                        if ch.state == "closing" && ch.queued_count == 0 {
                            ch.state = "closed".into();
                        }
                        Ok(json!({ "queued": ch.queued_count, "state": ch.state }))
                    }
                    "channel_close" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let ch = self
                            .channels
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown MessageChannel".to_owned())?;
                        if ch.state == "open" {
                            ch.state = if ch.queued_count == 0 {
                                "closed".into()
                            } else {
                                "closing".into()
                            };
                        }
                        Ok(json!({ "state": ch.state }))
                    }
                    _ => Err(format!("Unknown worker_op {action}")),
                }
            }
            // Mutex & Condition
            "mutex_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                let worker_id = req.get("workerId").and_then(Value::as_u64).unwrap_or(1) as u32;
                match action {
                    "new_mutex" => {
                        let id = self.alloc_id();
                        self.mutexes.insert(id, MutexRecord::default());
                        Ok(json!({ "id": id }))
                    }
                    "lock" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let m = self
                            .mutexes
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        match m.owner_worker {
                            None => {
                                m.owner_worker = Some(worker_id);
                                m.depth = 1;
                            }
                            Some(owner) if owner == worker_id => {
                                m.depth = m.depth.saturating_add(1);
                            }
                            Some(_) => {
                                return Err("IllegalOperationError: Mutex is currently locked by another worker.".into());
                            }
                        }
                        Ok(json!({ "locked": true, "depth": m.depth }))
                    }
                    "try_lock" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let m = self
                            .mutexes
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        let acquired = match m.owner_worker {
                            None => {
                                m.owner_worker = Some(worker_id);
                                m.depth = 1;
                                true
                            }
                            Some(owner) if owner == worker_id => {
                                m.depth = m.depth.saturating_add(1);
                                true
                            }
                            Some(_) => false,
                        };
                        Ok(json!({ "acquired": acquired, "depth": m.depth }))
                    }
                    "unlock" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let m = self
                            .mutexes
                            .get_mut(&id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        if m.owner_worker != Some(worker_id) || m.depth == 0 {
                            return Err("IllegalOperationError: Error #1520: Mutex cannot be unlocked because it is not owned by the current worker.".into());
                        }
                        m.depth -= 1;
                        if m.depth == 0 {
                            m.owner_worker = None;
                        }
                        Ok(json!({ "depth": m.depth }))
                    }
                    "new_condition" => {
                        let mutex_id =
                            req.get("mutexId").and_then(Value::as_u64).unwrap_or(0) as u32;
                        if !self.mutexes.contains_key(&mutex_id) {
                            return Err("ArgumentError: Condition requires a valid Mutex.".into());
                        }
                        let id = self.alloc_id();
                        self.conditions.insert(id, mutex_id);
                        Ok(json!({ "id": id }))
                    }
                    "condition_wait" | "condition_notify" | "condition_notify_all" => {
                        let id = req.get("id").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let timeout = req.get("timeout").and_then(Value::as_f64).unwrap_or(-1.0);
                        if action == "condition_wait" && timeout < 0.0 && timeout != -1.0 {
                            return Err(
                                "ArgumentError: Condition.wait timeout must be -1 or non-negative."
                                    .into(),
                            );
                        }
                        let mutex_id = *self
                            .conditions
                            .get(&id)
                            .ok_or_else(|| "Unknown Condition".to_owned())?;
                        let m = self
                            .mutexes
                            .get(&mutex_id)
                            .ok_or_else(|| "Unknown Mutex".to_owned())?;
                        if m.owner_worker != Some(worker_id) || m.depth == 0 {
                            return Err("IllegalOperationError: Error #1521: Condition requires its Mutex to be locked by the calling worker.".into());
                        }
                        // Single-threaded JS execution cannot block synchronously;
                        // finite timeout reports timedOut=true, default wait reports false.
                        let timed_out = action == "condition_wait" && timeout >= 0.0;
                        Ok(json!({ "timedOut": timed_out }))
                    }
                    _ => Err(format!("Unknown mutex_op {action}")),
                }
            }
            // Sampler & Telemetry
            "sampler_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "start" => {
                        self.sampling_active = true;
                        self.sampling_paused = false;
                        let now_us = req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                        if self.samples.is_empty() {
                            let stack = parse_sampler_stack(req, "startSampling", "flash.sampler");
                            self.samples.push_back(SamplerEntry {
                                kind: "sample".into(),
                                time_us: now_us,
                                id: 0,
                                size: 0,
                                class_name: None,
                                stack,
                            });
                        }
                        Ok(json!({ "active": true, "internalAllocs": self.sample_internal_allocs }))
                    }
                    "pause" => {
                        if self.sampling_active {
                            self.sampling_paused = true;
                        }
                        Ok(json!({ "paused": self.sampling_paused }))
                    }
                    "stop" => {
                        self.sampling_active = false;
                        self.sampling_paused = false;
                        Ok(json!({ "active": false }))
                    }
                    "clear" => {
                        self.samples.clear();
                        Ok(json!({ "count": 0 }))
                    }
                    "set_internal_allocs" => {
                        self.sample_internal_allocs =
                            req.get("enabled").and_then(Value::as_bool).unwrap_or(false);
                        Ok(json!({ "internalAllocs": self.sample_internal_allocs }))
                    }
                    "record_sample" => {
                        if self.sampling_active && !self.sampling_paused {
                            let time_us =
                                req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                            let stack = parse_sampler_stack(req, "sample", "runtime.as");
                            if self.samples.len() >= 4096 {
                                self.samples.pop_front();
                            }
                            self.samples.push_back(SamplerEntry {
                                kind: "sample".into(),
                                time_us,
                                id: 0,
                                size: 0,
                                class_name: None,
                                stack,
                            });
                        }
                        Ok(json!({ "count": self.samples.len() }))
                    }
                    "record_alloc" => {
                        let is_internal = req
                            .get("internal")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        if self.sampling_active
                            && !self.sampling_paused
                            && (!is_internal || self.sample_internal_allocs)
                        {
                            let id = req.get("allocId").and_then(Value::as_u64).unwrap_or(1);
                            let size = req.get("size").and_then(Value::as_u64).unwrap_or(32);
                            let cname = req
                                .get("className")
                                .and_then(Value::as_str)
                                .unwrap_or("Object")
                                .to_owned();
                            let time_us =
                                req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                            let stack = parse_sampler_stack(req, &cname, "runtime.as");
                            if self.samples.len() >= 4096 {
                                self.samples.pop_front();
                            }
                            self.samples.push_back(SamplerEntry {
                                kind: "new".into(),
                                time_us,
                                id,
                                size,
                                class_name: Some(cname),
                                stack,
                            });
                        }
                        Ok(json!({ "count": self.samples.len() }))
                    }
                    "record_delete" => {
                        if self.sampling_active && !self.sampling_paused {
                            let id = req.get("allocId").and_then(Value::as_u64).unwrap_or(1);
                            let size = req.get("size").and_then(Value::as_u64).unwrap_or(32);
                            let time_us =
                                req.get("timeUs").and_then(Value::as_f64).unwrap_or(1000.0);
                            let stack = if req.get("stack").is_some() {
                                parse_sampler_stack(req, "gc", "runtime.as")
                            } else {
                                Vec::new()
                            };
                            if self.samples.len() >= 4096 {
                                self.samples.pop_front();
                            }
                            self.samples.push_back(SamplerEntry {
                                kind: "delete".into(),
                                time_us,
                                id,
                                size,
                                class_name: None,
                                stack,
                            });
                        }
                        Ok(json!({ "count": self.samples.len() }))
                    }
                    "count" => Ok(json!({ "count": self.samples.len() as f64 })),
                    "get_samples" => {
                        let list: Vec<Value> = self.samples.iter().map(|s| json!(s)).collect();
                        Ok(json!({ "samples": list }))
                    }
                    "record_invocation" => {
                        let key = req.get("key").and_then(Value::as_str).unwrap_or("");
                        let entry = self.invocation_counts.entry(key.to_owned()).or_insert(0);
                        *entry += 1;
                        Ok(json!({ "count": *entry as f64 }))
                    }
                    "get_invocation" => {
                        let key = req.get("key").and_then(Value::as_str).unwrap_or("");
                        let count = self.invocation_counts.get(key).copied().unwrap_or(0);
                        Ok(json!({ "count": count as f64 }))
                    }
                    "state" => Ok(json!({
                        "active": self.sampling_active,
                        "paused": self.sampling_paused,
                        "internalAllocs": self.sample_internal_allocs,
                        "count": self.samples.len(),
                    })),
                    _ => Err(format!("Unknown sampler_op {action}")),
                }
            }
            "telemetry_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "register" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        if name.is_empty() || name.starts_with('.') {
                            return Ok(json!({ "registered": false }));
                        }
                        Ok(json!({ "registered": self.telemetry_commands.insert(name.to_owned()) }))
                    }
                    "unregister" => {
                        let name = req.get("name").and_then(Value::as_str).unwrap_or("");
                        Ok(json!({ "removed": self.telemetry_commands.remove(name) }))
                    }
                    "metric" => {
                        let metric = req.get("metric").and_then(Value::as_str).unwrap_or("");
                        let val = req.get("value").cloned().unwrap_or(Value::Null);
                        if self.telemetry_metrics.len() >= 1024 {
                            self.telemetry_metrics.remove(0);
                        }
                        self.telemetry_metrics.push((metric.to_owned(), val));
                        Ok(json!({ "recorded": self.telemetry_metrics.len() }))
                    }
                    "span_metric" => {
                        let metric = req.get("metric").and_then(Value::as_str).unwrap_or("");
                        let start = req
                            .get("startSpanMarker")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        let span_ms = if let Some(end) =
                            req.get("endSpanMarker").and_then(Value::as_f64)
                        {
                            (end - start).max(0.0)
                        } else if let Some(dur) = req.get("durationMs").and_then(Value::as_f64) {
                            dur.max(0.0)
                        } else {
                            0.0
                        };
                        let val = req.get("value").cloned().unwrap_or(Value::Null);
                        let entry = json!({
                            "startSpanMarker": start,
                            "durationMs": span_ms,
                            "value": val,
                        });
                        if self.telemetry_metrics.len() >= 1024 {
                            self.telemetry_metrics.remove(0);
                        }
                        self.telemetry_metrics.push((metric.to_owned(), entry));
                        Ok(
                            json!({ "recorded": self.telemetry_metrics.len(), "durationMs": span_ms }),
                        )
                    }
                    "drain_metrics" => {
                        let list: Vec<Value> = self
                            .telemetry_metrics
                            .iter()
                            .map(|(m, v)| json!({ "metric": m, "value": v }))
                            .collect();
                        Ok(json!({
                            "metrics": list,
                            "commands": self.telemetry_commands.iter().cloned().collect::<Vec<_>>(),
                            "profilerActive": self.profiler_active,
                            "redrawRegions": {
                                "shown": self.redraw_regions_shown,
                                "color": self.redraw_regions_color,
                            }
                        }))
                    }
                    "profile" => {
                        let enable = req.get("enable").and_then(Value::as_bool).unwrap_or(false);
                        self.profiler_active = enable;
                        if enable {
                            self.sampling_active = true;
                            self.sampling_paused = false;
                        } else {
                            self.sampling_active = false;
                            self.sampling_paused = false;
                        }
                        Ok(json!({ "profilerActive": self.profiler_active }))
                    }
                    "show_redraw" => {
                        self.redraw_regions_shown =
                            req.get("on").and_then(Value::as_bool).unwrap_or(false);
                        self.redraw_regions_color =
                            req.get("color").and_then(Value::as_u64).unwrap_or(0xff0000) as u32;
                        Ok(json!({
                            "shown": self.redraw_regions_shown,
                            "color": self.redraw_regions_color,
                        }))
                    }
                    _ => Err(format!("Unknown telemetry_op {action}")),
                }
            }
            "security_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "load_policy_xml" => {
                        let url = req.get("url").and_then(Value::as_str).unwrap_or("");
                        let xml = req.get("xml").and_then(Value::as_str).unwrap_or("");
                        let parsed = parse_cross_domain_policy_xml(xml)?;
                        let snap = serde_json::to_value(&parsed).map_err(|e| e.to_string())?;
                        self.policy_files.push((url.to_owned(), parsed));
                        Ok(json!({ "ok": true, "policy": snap }))
                    }
                    "check_access" => {
                        let requester = req
                            .get("requesterDomain")
                            .and_then(Value::as_str)
                            .unwrap_or("localhost");
                        let target_host = req
                            .get("targetDomain")
                            .or_else(|| req.get("domain"))
                            .and_then(Value::as_str)
                            .map(|s| {
                                let extracted = extract_policy_target_host(s);
                                if !extracted.is_empty() {
                                    extracted
                                } else {
                                    s.trim()
                                        .trim_matches(|c| c == '[' || c == ']')
                                        .to_ascii_lowercase()
                                }
                            })
                            .unwrap_or_default();
                        let port = req
                            .get("port")
                            .and_then(Value::as_u64)
                            .filter(|&p| p > 0)
                            .and_then(|p| u16::try_from(p).ok());
                        let is_secure = req.get("secure").and_then(Value::as_bool).unwrap_or(true);
                        let exact_settings = req
                            .get("exactSettings")
                            .and_then(Value::as_bool)
                            .unwrap_or(true);
                        let allowed = self.policy_files.iter().any(|(pf_url, pf)| {
                            let pf_host = extract_policy_target_host(pf_url);
                            let host_matches = if target_host.is_empty() {
                                true
                            } else if !pf_host.is_empty() {
                                pf_host == target_host
                            } else {
                                pf_url.trim().to_ascii_lowercase().starts_with("data:")
                            };
                            host_matches
                                && policy_allows_access_with_settings(
                                    pf,
                                    requester,
                                    port,
                                    is_secure,
                                    exact_settings,
                                )
                        });
                        Ok(json!({
                            "allowed": allowed,
                            "policyCount": self.policy_files.len(),
                        }))
                    }
                    "set_disable_avm1" => {
                        if req.get("value").and_then(Value::as_bool).unwrap_or(false) {
                            self.disable_avm1_loading = true;
                        }
                        Ok(json!({ "disableAVM1Loading": self.disable_avm1_loading }))
                    }
                    "show_settings" => {
                        let panel = req
                            .get("panel")
                            .and_then(Value::as_str)
                            .unwrap_or("default")
                            .to_owned();
                        self.last_security_panel = Some(panel.clone());
                        Ok(json!({ "panel": panel }))
                    }
                    "state" => Ok(json!({
                        "disableAVM1Loading": self.disable_avm1_loading,
                        "lastPanel": self.last_security_panel,
                        "policyCount": self.policy_files.len(),
                    })),
                    _ => Err(format!("Unknown security_op {action}")),
                }
            }
            "system_op" => {
                let action = req.get("action").and_then(Value::as_str).unwrap_or("");
                match action {
                    "pause" => {
                        self.system_paused = true;
                        Ok(json!({ "paused": true }))
                    }
                    "resume" => {
                        self.system_paused = false;
                        Ok(json!({ "paused": false }))
                    }
                    "exit" => {
                        let code = req.get("code").and_then(Value::as_u64).unwrap_or(0) as u32;
                        self.last_exit_code = Some(code);
                        Ok(json!({ "exitCode": code }))
                    }
                    "state" => Ok(json!({
                        "paused": self.system_paused,
                        "exitCode": self.last_exit_code,
                    })),
                    _ => Err(format!("Unknown system_op {action}")),
                }
            }
            "x509_verify" => {
                let server_b64 = req
                    .get("serverBase64")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let server_der = STANDARD.decode(server_b64.as_bytes()).unwrap_or_default();
                let host = req.get("host").and_then(Value::as_str).unwrap_or("");
                let now_ms = req.get("nowMs").and_then(Value::as_f64).unwrap_or(0.0);
                let os_verified = req
                    .get("osVerified")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut chain = Vec::new();
                if let Some(arr) = req.get("chain").and_then(Value::as_array) {
                    for entry in arr.iter().take(32) {
                        if let Some(b64) = entry.get("base64").and_then(Value::as_str)
                            && let Ok(der) = STANDARD.decode(b64.as_bytes())
                        {
                            let trusted = entry
                                .get("trusted")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            chain.push((der, trusted));
                        }
                    }
                }
                let (status, cert) =
                    verify_x509_certificate_chain(&server_der, host, now_ms, &chain, os_verified);
                Ok(json!({
                    "status": status,
                    "certificate": cert,
                }))
            }
            "x509_build_custom" => {
                let subject = req
                    .get("subjectCN")
                    .and_then(Value::as_str)
                    .unwrap_or("localhost");
                let issuer = req
                    .get("issuerCN")
                    .and_then(Value::as_str)
                    .unwrap_or(subject);
                let nb = req
                    .get("notBefore")
                    .and_then(Value::as_str)
                    .unwrap_or("20250101000000Z");
                let na = req
                    .get("notAfter")
                    .and_then(Value::as_str)
                    .unwrap_or("20350101000000Z");
                let der = build_custom_x509_der(subject, issuer, nb, na);
                Ok(json!({ "base64": STANDARD.encode(&der) }))
            }
            "inspect_av_stream_tags" => {
                let b64 = req.get("base64").and_then(Value::as_str).unwrap_or("");
                let base_time = req.get("baseTime").and_then(Value::as_f64).unwrap_or(0.0);
                let bytes = STANDARD.decode(b64.as_bytes()).unwrap_or_default();
                let tags = crate::as3_media_events::inspect_av_stream_tags(&bytes, base_time);
                Ok(json!({ "tags": tags }))
            }
            "fast_mem_op" => {
                let kind = req.get("kind").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "sxi1" => {
                        let v = req.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                        Ok(json!({ "result": fast_mem_sxi1(v) }))
                    }
                    "sxi8" => {
                        let v = req.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                        Ok(json!({ "result": fast_mem_sxi8(v) }))
                    }
                    "sxi16" => {
                        let v = req.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                        Ok(json!({ "result": fast_mem_sxi16(v) }))
                    }
                    _ => Err(format!("Unknown fast_mem_op '{kind}'")),
                }
            }
            _ => Err(format!("Unsupported As3NetSysKernel op '{op}'")),
        }
    }
}
