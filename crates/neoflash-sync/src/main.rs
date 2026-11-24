//! An optional relay for the WebSocket transport used by published remote SharedObjects.
//! Room updates are serialized in arrival order. The server validates and stores AMF values per key.
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use neoflash_core::{AmfNode, AmfValue, amf_decode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Notify, Semaphore, mpsc},
    time::timeout,
};
use tokio_tungstenite::{
    accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
        protocol::WebSocketConfig,
    },
};

const MAX_FRAME: usize = 512 * 1024;
const MAX_STATE: usize = 256 * 1024;
const MAX_FIELDS: usize = 1024;
type Fields = BTreeMap<String, WireValue>;
#[derive(Clone)]
struct Outbox {
    tx: mpsc::Sender<String>,
    overflow: Arc<Notify>,
}

impl Outbox {
    fn send(&self, message: String) -> Result<(), &'static str> {
        self.tx.try_send(message).map_err(|_| {
            self.overflow.notify_one();
            "Client send queue is full."
        })
    }
}
type RoomKey = (String, String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireValue {
    encoding: u8,
    bytes: String,
}

impl WireValue {
    fn version(&self) -> String {
        let mut hash = Sha256::new();
        hash.update([self.encoding]);
        hash.update(self.bytes.as_bytes());
        format!("{:x}", hash.finalize())
    }

    fn valid(&self, arguments: bool) -> bool {
        if self.bytes.len() > MAX_STATE {
            return false;
        }
        let Ok(bytes) = STANDARD.decode(&self.bytes) else {
            return false;
        };
        let Ok(decoded) = amf_decode(self.encoding, &bytes) else {
            return false;
        };
        if decoded.consumed != bytes.len() {
            return false;
        }
        if !arguments {
            return true;
        }
        let AmfValue::Ref(index) = decoded.graph.root else {
            return false;
        };
        matches!(decoded.graph.nodes.get(index), Some(AmfNode::Array{dense,entries,..}) if entries.is_empty() && dense.len() <= 256)
    }
}

#[derive(Default)]
struct Room {
    persistent: bool,
    fields: Fields,
    peers: HashMap<u64, (u64, Outbox)>,
}

#[derive(Default)]
struct Relay {
    store: Option<PathBuf>,
    rooms: HashMap<RoomKey, Room>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
}

fn field_name(name: &str) -> bool {
    valid_name(name) && !matches!(name, "__proto__" | "constructor" | "prototype")
}

fn publication_origin(raw: &str) -> Result<String, &'static str> {
    use tokio_tungstenite::tungstenite::http::Uri;
    let uri: Uri = raw
        .parse()
        .map_err(|_| "Supply an HTTP(S) publication origin.")?;
    let authority = uri
        .authority()
        .ok_or("The publication origin needs a host.")?;
    let scheme = uri
        .scheme_str()
        .ok_or("The publication origin needs a scheme.")?;
    let rest = authority
        .as_str()
        .strip_prefix(authority.host())
        .unwrap_or(authority.as_str());
    let valid_port = rest.is_empty()
        || rest
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok())
            .is_some_and(|p| p > 0);
    if !matches!(scheme, "http" | "https")
        || authority.host().is_empty()
        || !valid_port
        || authority.as_str().contains(['@', '*'])
        || uri.query().is_some()
        || uri.path() != "/"
    {
        return Err("Use an exact HTTP(S) origin with no credentials, path or query.");
    }
    Ok(format!("{scheme}://{authority}"))
}

impl Relay {
    fn file(&self, key: &RoomKey) -> Option<PathBuf> {
        let mut hash = Sha256::new();
        hash.update(key.0.as_bytes());
        hash.update([0]);
        hash.update(key.1.as_bytes());
        self.store
            .as_ref()
            .map(|dir| dir.join(format!("{:x}.json", hash.finalize())))
    }

    fn load(&self, key: &RoomKey) -> Result<Fields, &'static str> {
        let file = self
            .file(key)
            .ok_or("Persistent rooms require --store DIR.")?;
        match fs::metadata(&file) {
            Ok(m) if m.len() <= MAX_STATE as u64 => {
                let bytes = fs::read(&file).map_err(|_| "Cannot read stored room.")?;
                let fields: Fields =
                    serde_json::from_slice(&bytes).map_err(|_| "Invalid stored room.")?;
                if fields.len() > MAX_FIELDS
                    || fields
                        .iter()
                        .any(|(k, v)| !field_name(k) || !v.valid(false))
                {
                    return Err("Invalid stored room fields.");
                }
                Ok(fields)
            }
            Ok(_) => Err("Stored room exceeds its limit."),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Fields::new()),
            Err(_) => Err("Cannot open stored room."),
        }
    }

    fn leave(&mut self, key: &RoomKey, peer: u64) {
        if let Some(room) = self.rooms.get_mut(key) {
            room.peers.remove(&peer);
            if room.peers.is_empty() {
                self.rooms.remove(key);
            }
        }
    }

    fn apply(
        &mut self,
        key: &RoomKey,
        peer: u64,
        sub: u64,
        out: &Outbox,
        message: &Value,
    ) -> Result<(), &'static str> {
        let op = message["op"].as_str().ok_or("Missing operation.")?;
        if op == "join" {
            let persistent = message["persistent"]
                .as_bool()
                .ok_or("Invalid persistence.")?;
            if !self.rooms.contains_key(key) {
                if self.rooms.len() >= 256 {
                    return Err("Active room limit reached.");
                }
                let fields = if persistent {
                    self.load(key)?
                } else {
                    Fields::new()
                };
                self.rooms.insert(
                    key.clone(),
                    Room {
                        persistent,
                        fields,
                        ..Room::default()
                    },
                );
            }
            let room = self.rooms.get_mut(key).unwrap();
            if room.persistent != persistent {
                return Err("Room persistence differs from its existing subscription.");
            }
            if room.peers.contains_key(&peer) {
                return Err("Room is already joined.");
            }
            let versions: BTreeMap<_, _> =
                room.fields.iter().map(|(k, v)| (k, v.version())).collect();
            out.send(
                json!({"type":"so","op":"snapshot","room":key.1,"sub":sub,"fields":room.fields,"versions":versions})
                    .to_string(),
            )?;
            room.peers.insert(peer, (sub, out.clone()));
            return Ok(());
        }
        let file = self.file(key);
        let room = self.rooms.get_mut(key).ok_or("Join the room first.")?;
        if room.peers.get(&peer).map(|p| p.0) != Some(sub) {
            return Err("Unknown room subscription.");
        }
        if op == "flush" {
            return out
                .send(json!({"type":"so","op":"flushed","room":key.1,"sub":sub}).to_string());
        }
        let mut event = json!({"type":"so","op":op,"room":key.1});
        let mut rejected: BTreeMap<String, Option<WireValue>> = BTreeMap::new();
        match op {
            "leave" => {
                self.leave(key, peer);
                return Ok(());
            }
            "patch" | "clear" => {
                let mut fields = Fields::new();
                if op == "patch" {
                    let patch: BTreeMap<String, Option<WireValue>> =
                        serde_json::from_value(message["fields"].clone())
                            .map_err(|_| "Invalid property updates.")?;
                    if patch.len() > MAX_FIELDS {
                        return Err("Too many property updates.");
                    }
                    let expected: Option<BTreeMap<String, Option<String>>> = message
                        .get("expected")
                        .map(|v| {
                            serde_json::from_value(v.clone())
                                .map_err(|_| "Invalid offline field versions.")
                        })
                        .transpose()?;
                    if expected
                        .as_ref()
                        .is_some_and(|values| !values.keys().eq(patch.keys()))
                    {
                        return Err("Missing offline field version.");
                    }
                    if expected.as_ref().is_some_and(|fields| {
                        fields
                            .values()
                            .flatten()
                            .any(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()))
                    }) {
                        return Err("Invalid offline field version.");
                    }
                    let mut accepted = BTreeMap::new();
                    let mut versions = BTreeMap::new();
                    fields = room.fields.clone();
                    for (key, value) in patch {
                        if !field_name(&key) || value.as_ref().is_some_and(|v| !v.valid(false)) {
                            return Err("Invalid AMF property update.");
                        }
                        let current = fields.get(&key);
                        if let Some(checks) = &expected
                            && current.map(WireValue::version) != checks[&key]
                            && current != value.as_ref()
                        {
                            rejected.insert(key, current.cloned());
                            continue;
                        }
                        accepted.insert(key.clone(), value.clone());
                        match value {
                            Some(v) => {
                                versions.insert(key.clone(), v.version());
                                fields.insert(key, v);
                            }
                            None => {
                                fields.remove(&key);
                            }
                        }
                    }
                    event["fields"] = json!(accepted);
                    event["versions"] = json!(versions);
                }
                let bytes = serde_json::to_vec(&fields).map_err(|_| "Cannot encode room.")?;
                if fields.len() > MAX_FIELDS || bytes.len() > MAX_STATE {
                    return Err("Room storage limit reached.");
                }
                if room.persistent {
                    let file = file.ok_or("Persistent storage unavailable.")?;
                    if !file.exists()
                        && fs::read_dir(file.parent().unwrap())
                            .map_err(|_| "Cannot read room directory.")?
                            .take(1024)
                            .count()
                            >= 1024
                    {
                        return Err("Persistent room limit reached.");
                    }
                    let mut tmp = tempfile::NamedTempFile::new_in(file.parent().unwrap())
                        .map_err(|_| "Cannot write stored room.")?;
                    tmp.write_all(&bytes)
                        .and_then(|_| tmp.as_file().sync_all())
                        .map_err(|_| "Cannot save stored room.")?;
                    tmp.persist(file)
                        .map_err(|_| "Cannot commit stored room.")?;
                }
                room.fields = fields;
            }
            "call" => {
                let method = message["method"]
                    .as_str()
                    .filter(|s| field_name(s))
                    .ok_or("Invalid callback name.")?;
                let args: WireValue = serde_json::from_value(message["args"].clone())
                    .map_err(|_| "Invalid callback arguments.")?;
                if !args.valid(true) {
                    return Err("Invalid AMF callback arguments.");
                }
                event["method"] = json!(method);
                event["args"] = message["args"].clone();
            }
            _ => return Err("Unknown room operation."),
        }
        // A slow recipient is disconnected; it will obtain a fresh snapshot when it rejoins.
        for (id, (token, channel)) in &room.peers {
            if *id != peer
                && op == "patch"
                && event["fields"].as_object().is_some_and(|v| v.is_empty())
            {
                continue;
            }
            let mut outgoing = event.clone();
            outgoing["sub"] = json!(token);
            outgoing["own"] = json!(*id == peer);
            if *id == peer && !rejected.is_empty() {
                outgoing["rejected"] = json!(rejected.keys().collect::<Vec<_>>());
                for (key, value) in &rejected {
                    outgoing["fields"][key] = json!(value);
                    if let Some(v) = value {
                        outgoing["versions"][key] = json!(v.version());
                    }
                }
            }
            let _ = channel.send(outgoing.to_string());
        }
        Ok(())
    }
}

async fn session(stream: TcpStream, id: u64, relay: Arc<Mutex<Relay>>, origins: Arc<Vec<String>>) {
    let mut path = String::new();
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    // The tungstenite callback requires an owned HTTP response for handshake rejection.
    #[allow(clippy::result_large_err)]
    let handshake = accept_hdr_async_with_config(
        stream,
        |req: &Request, res: Response| {
            let origin = req.headers().get("origin").and_then(|s| s.to_str().ok());
            if !origins.iter().any(|o| Some(o.as_str()) == origin)
                || req.uri().query().is_some()
                || req.uri().path().len() > 1024
            {
                return Err(tokio_tungstenite::tungstenite::http::Response::builder()
                    .status(403)
                    .body(Some(
                        "Use an allowed publication Origin and a plain application path."
                            .to_owned(),
                    ))
                    .unwrap());
            }
            path = req.uri().path().to_owned();
            Ok(res)
        },
        Some(config),
    );
    let Ok(Ok(mut ws)) = timeout(Duration::from_secs(5), handshake).await else {
        return;
    };
    let (sender, mut rx) = mpsc::channel::<String>(32);
    let tx = Outbox {
        tx: sender,
        overflow: Arc::new(Notify::new()),
    };
    loop {
        tokio::select! {
            biased;
            _=tx.overflow.notified()=>break,
            msg=rx.recv()=> {
                let Some(msg)=msg else {break};
                if !matches!(timeout(Duration::from_secs(5),ws.send(Message::text(msg))).await, Ok(Ok(()))) {break;}
            }
            incoming=ws.next()=> {
                let Some(Ok(message))=incoming else {break};
                if message.is_close() {break;}
                if !message.is_text() {continue;}
                let Ok(msg)=serde_json::from_str::<Value>(message.to_text().unwrap()) else {break};
                if msg["type"]=="nc_hello" {continue;}
                if msg["type"]!="so" {break;}
                let Some(name)=msg["room"].as_str().filter(|n|valid_name(n)) else {break};
                let Some(sub)=msg["sub"].as_u64().filter(|n|*n>0) else {break};
                let key=(path.clone(),name.to_owned());
                let mut broker=relay.lock().unwrap();
                if let Err(reason)=broker.apply(&key,id,sub,&tx,&msg) {
                    if tx.send(json!({"type":"so","op":"error","room":name,"sub":sub,"description":reason}).to_string()).is_err() {break;}
                    if broker.rooms.get(&key).and_then(|r|r.peers.get(&id)).is_some_and(|p|p.0==sub){broker.leave(&key,id);}
                }
            }
        }
    }
    let mut broker = relay.lock().unwrap();
    let joined: Vec<_> = broker.rooms.keys().cloned().collect();
    for key in joined {
        broker.leave(&key, id);
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut bind: SocketAddr = "127.0.0.1:8090".parse()?;
    let mut origins = Vec::new();
    let mut store = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => bind = args.next().ok_or("missing bind address")?.parse()?,
            "--origin" => origins.push(publication_origin(
                &args.next().ok_or("missing publication origin")?,
            )?),
            "--store" => store = Some(PathBuf::from(args.next().ok_or("missing store directory")?)),
            "-h" | "--help" => {
                println!(
                    "neoflash-sync --origin ORIGIN [--origin ORIGIN] [--bind 127.0.0.1:8090] [--store DIR]"
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    if origins.is_empty() {
        return Err("Supply --origin with the exact HTTP(S) publication origin.".into());
    }
    if let Some(dir) = &store {
        fs::create_dir_all(dir)?;
    }
    let relay = Arc::new(Mutex::new(Relay {
        store,
        ..Relay::default()
    }));
    let origins = Arc::new(origins);
    let listener = TcpListener::bind(bind).await?;
    println!("LISTENING {}", listener.local_addr()?);
    let limit = Arc::new(Semaphore::new(64));
    let mut next = 0;
    loop {
        let (s, _) = listener.accept().await?;
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            continue;
        };
        next += 1;
        let relay = relay.clone();
        let origins = origins.clone();
        tokio::spawn(async move {
            session(s, next, relay, origins).await;
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_origins_and_complete_amf_fields() {
        assert_eq!(
            publication_origin("http://localhost:8000").unwrap(),
            "http://localhost:8000"
        );
        assert_eq!(
            publication_origin("https://movie.example/").unwrap(),
            "https://movie.example"
        );
        for origin in [
            "null",
            "*",
            "https://*.example",
            "file://host/",
            "https://movie.example/player",
        ] {
            assert!(publication_origin(origin).is_err());
        }
        let value = |bytes: &[u8]| WireValue {
            encoding: 3,
            bytes: STANDARD.encode(bytes),
        };
        assert!(value(&[3]).valid(false)); // AMF3 true
        assert!(!value(&[3, 3]).valid(false)); // Two top-level values
        assert!(!value(&[3]).valid(true)); // Calls carry a bounded argument array
        assert!(value(&[9, 1, 1]).valid(true)); // Empty array
    }

    const FUZZ_SEED: u64 = 0x4E46_3437_3200;

    struct DeterministicRng {
        state: u64,
    }

    impl DeterministicRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_usize(&mut self, upper: usize) -> usize {
            if upper <= 1 {
                0
            } else {
                (self.next_u64() as usize) % upper
            }
        }

        fn mutate_bytes(&mut self, base: &[u8]) -> Vec<u8> {
            let mut buf = base.to_vec();
            if buf.is_empty() {
                return vec![self.next_u64() as u8];
            }
            let ops = 1 + self.next_usize(4);
            for _ in 0..ops {
                let pos = self.next_usize(buf.len());
                match self.next_usize(5) {
                    0 => buf[pos] ^= 1u8 << (self.next_usize(8) as u8),
                    1 => buf[pos] = self.next_u64() as u8,
                    2 => {
                        let del = 1 + self.next_usize((buf.len() - pos).min(8));
                        buf.drain(pos..pos + del);
                        if buf.is_empty() {
                            buf.push(b'{');
                        }
                    }
                    3 if buf.len() < 4096 => {
                        let snippet = [
                            b"\"__proto__\"".as_slice(),
                            b"\"constructor\"",
                            b"\"prototype\"",
                            b"18446744073709551615",
                            b"-1",
                            b"null",
                            b"\"\\u0000\"",
                        ][self.next_usize(7)];
                        buf.splice(pos..pos, snippet.iter().copied());
                    }
                    _ => buf[pos] = b'"',
                }
            }
            buf
        }
    }

    fn make_outbox(cap: usize) -> (Outbox, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(cap);
        (
            Outbox {
                tx,
                overflow: Arc::new(Notify::new()),
            },
            rx,
        )
    }

    #[test]
    fn t472_sync_relay_protocol_and_storage_fuzz() {
        let mut rng = DeterministicRng::new(FUZZ_SEED);
        let dir = tempfile::tempdir().expect("tempdir");

        // 1. Origin validation boundaries
        for bad_origin in [
            "",
            "http://",
            "https://user:pass@example.com",
            "https://example.com?x=1",
            "https://example.com/subpath",
            "ws://example.com",
            "wss://example.com",
            "http://exam*ple.com",
            "http://example.com:99999",
            "http://\0bad.example",
        ] {
            assert!(publication_origin(bad_origin).is_err(), "{bad_origin}");
        }

        // 2. WireValue bounds (> MAX_STATE, > 256 callback args, associative AMF3 array)
        let oversized_wv = WireValue {
            encoding: 3,
            bytes: "A".repeat(MAX_STATE + 4),
        };
        assert!(!oversized_wv.valid(false));
        assert!(!oversized_wv.valid(true));

        // AMF3 dense array with 257 `true` items: header `(257 << 1) | 1 = 515` -> U29 `[0x84, 0x03]`
        let mut args_257 = vec![0x09, 0x84, 0x03, 0x01];
        args_257.resize(4 + 257, 0x03);
        let wv_257 = WireValue {
            encoding: 3,
            bytes: STANDARD.encode(&args_257),
        };
        assert!(wv_257.valid(false));
        assert!(!wv_257.valid(true)); // > 256 callback arguments must be rejected!

        // 3. Stored room corruption & size/field limits
        let mut relay = Relay {
            store: Some(dir.path().to_path_buf()),
            ..Relay::default()
        };
        let key = ("/app".to_owned(), "lobby".to_owned());
        let room_file = relay.file(&key).unwrap();

        // Oversized file > MAX_STATE
        fs::write(&room_file, vec![b' '; MAX_STATE + 1]).unwrap();
        assert_eq!(relay.load(&key), Err("Stored room exceeds its limit."));

        // Corrupted JSON prefix truncations
        let valid_wv = WireValue {
            encoding: 3,
            bytes: STANDARD.encode([0x04, 0x2A]), // AMF3 int 42
        };
        let valid_room_bytes =
            serde_json::to_vec(&BTreeMap::from([("score".to_owned(), valid_wv.clone())])).unwrap();
        for trunc in 0..valid_room_bytes.len() {
            fs::write(&room_file, &valid_room_bytes[..trunc]).unwrap();
            let _ = relay.load(&key);
        }

        // Prototype-pollution keys in stored file
        for forbidden in ["__proto__", "constructor", "prototype", "bad\u{0007}key"] {
            let bad_bytes =
                serde_json::to_vec(&BTreeMap::from([(forbidden.to_owned(), valid_wv.clone())]))
                    .unwrap();
            fs::write(&room_file, bad_bytes).unwrap();
            assert_eq!(relay.load(&key), Err("Invalid stored room fields."));
        }
        let _ = fs::remove_file(&room_file);

        // 4. Active room cap (256 rooms)
        let (out, mut rx) = make_outbox(512);
        for i in 0..256 {
            let rk = ("/app".to_owned(), format!("r_{i}"));
            relay
                .apply(&rk, 1, 1, &out, &json!({"op":"join","persistent":false}))
                .unwrap();
            let _ = rx.try_recv();
        }
        assert_eq!(
            relay.apply(
                &("/app".to_owned(), "r_overflow".to_owned()),
                1,
                1,
                &out,
                &json!({"op":"join","persistent":false}),
            ),
            Err("Active room limit reached.")
        );
        for i in 0..256 {
            relay.leave(&("/app".to_owned(), format!("r_{i}")), 1);
        }
        assert!(relay.rooms.is_empty());

        // 5. Join persistent room + CAS conflict + forbidden keys + 300 seeded PRNG JSON mutations
        relay
            .apply(&key, 1, 7, &out, &json!({"op":"join","persistent":true}))
            .unwrap();
        let _ = rx.try_recv();

        for forbidden in ["__proto__", "constructor", "prototype", "", "a\0b"] {
            assert!(
                relay
                    .apply(
                        &key,
                        1,
                        7,
                        &out,
                        &json!({"op":"patch","fields":{forbidden: &valid_wv}}),
                    )
                    .is_err()
            );
        }

        // Valid patch followed by stale-CAS patch (exercises `rejected` path)
        relay
            .apply(
                &key,
                1,
                7,
                &out,
                &json!({"op":"patch","fields":{"score": &valid_wv}}),
            )
            .unwrap();
        let _ = rx.try_recv();
        let stale_hex = "0".repeat(64);
        let alt_wv = WireValue {
            encoding: 3,
            bytes: STANDARD.encode([0x04, 0x07]),
        };
        relay
            .apply(
                &key,
                1,
                7,
                &out,
                &json!({
                    "op":"patch",
                    "fields":{"score": &alt_wv},
                    "expected":{"score": stale_hex}
                }),
            )
            .unwrap();
        let cas_reply: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(cas_reply["rejected"], json!(["score"]));

        let seed_frames = [
            json!({"type":"so","room":"lobby","sub":7,"op":"patch","fields":{"hp":{"encoding":3,"bytes":"BC o="}}}),
            json!({"type":"so","room":"lobby","sub":7,"op":"call","method":"ping","args":{"encoding":3,"bytes":"CQEB"}}),
            json!({"type":"so","room":"lobby","sub":7,"op":"flush"}),
            json!({"type":"so","room":"lobby","sub":7,"op":"clear"}),
        ];
        for iter in 0..300 {
            let base = serde_json::to_vec(&seed_frames[iter % seed_frames.len()]).unwrap();
            let mutated = rng.mutate_bytes(&base);
            if let Ok(val) = serde_json::from_slice::<Value>(&mutated) {
                let (fuzz_out, _fuzz_rx) = make_outbox(64);
                let _ = relay.apply(&key, 1, 7, &fuzz_out, &val);
            }
        }
    }

    #[tokio::test]
    async fn t472_sync_websocket_handshake_and_oversized_frame_bounds() {
        use tokio::io::AsyncWriteExt;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;

        let mut rng = DeterministicRng::new(FUZZ_SEED ^ 0x55AA);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let relay = Arc::new(Mutex::new(Relay::default()));
        let origins = Arc::new(vec!["http://localhost:8000".to_owned()]);

        let srv_relay = relay.clone();
        let srv_origins = origins.clone();
        let server_task = tokio::spawn(async move {
            let mut next_id = 0u64;
            while let Ok((stream, _)) = listener.accept().await {
                next_id += 1;
                let r = srv_relay.clone();
                let o = srv_origins.clone();
                tokio::spawn(async move {
                    session(stream, next_id, r, o).await;
                });
            }
        });

        // 1. Raw truncated / garbage TCP bytes must close cleanly without panicking
        for raw_garbage in [
            b"".as_slice(),
            b"GET / HTTP/1.1\r\n",
            b"\xFF\x00\xFE\x01garbage\r\n\r\n",
        ] {
            let mut tcp = TcpStream::connect(addr).await.unwrap();
            let _ = tcp.write_all(raw_garbage).await;
            let _ = tcp.shutdown().await;
        }

        // 2. Disallowed Origin, query string, and >1024-byte path must be rejected with HTTP 403
        let long_path = format!("/{}", "a".repeat(1030));
        for (uri, origin) in [
            (format!("ws://{addr}/room"), "http://evil.example"),
            (format!("ws://{addr}/room?token=1"), "http://localhost:8000"),
            (format!("ws://{addr}{long_path}"), "http://localhost:8000"),
        ] {
            let mut req = uri.into_client_request().unwrap();
            req.headers_mut().insert("Origin", origin.parse().unwrap());
            let tcp = TcpStream::connect(addr).await.unwrap();
            let err = tokio_tungstenite::client_async(req, tcp)
                .await
                .expect_err("invalid handshake must be rejected");
            let msg = err.to_string();
            assert!(msg.contains("403"), "expected HTTP 403, got: {msg}");
        }

        // 3. Valid client sending binary frames, nc_hello, forbidden patch, and PRNG-mutated JSON frames
        {
            let mut req = format!("ws://{addr}/game").into_client_request().unwrap();
            req.headers_mut()
                .insert("Origin", "http://localhost:8000".parse().unwrap());
            let tcp = TcpStream::connect(addr).await.unwrap();
            let (mut ws, _) = tokio_tungstenite::client_async(req, tcp).await.unwrap();

            // Binary frames & nc_hello are ignored without disconnecting
            ws.send(Message::Binary(vec![0xDE, 0xAD, 0xBE, 0xEF].into()))
                .await
                .unwrap();
            ws.send(Message::text(r#"{"type":"nc_hello"}"#))
                .await
                .unwrap();

            // Join + error recovery
            ws.send(Message::text(
                r#"{"type":"so","room":"arena","sub":1,"op":"join","persistent":false}"#,
            ))
            .await
            .unwrap();
            let snap = ws.next().await.unwrap().unwrap();
            let snap_val: Value = serde_json::from_str(snap.to_text().unwrap()).unwrap();
            assert_eq!(snap_val["op"], "snapshot");

            ws.send(Message::text(
                r#"{"type":"so","room":"arena","sub":1,"op":"patch","fields":{"__proto__":null}}"#,
            ))
            .await
            .unwrap();
            let err_frame = ws.next().await.unwrap().unwrap();
            let err_val: Value = serde_json::from_str(err_frame.to_text().unwrap()).unwrap();
            assert_eq!(err_val["op"], "error");
            let _ = ws.close(None).await;
        }

        // 4. Deeply nested JSON (500 levels) & 25 mutated text frames + > MAX_FRAME (512 KiB) rejection
        for payload in [
            format!("{}0{}", "[".repeat(500), "]".repeat(500)),
            "x".repeat(MAX_FRAME + 4096),
        ] {
            let mut req = format!("ws://{addr}/game").into_client_request().unwrap();
            req.headers_mut()
                .insert("Origin", "http://localhost:8000".parse().unwrap());
            let tcp = TcpStream::connect(addr).await.unwrap();
            let (mut ws, _) = tokio_tungstenite::client_async_with_config(
                req,
                tcp,
                Some(
                    WebSocketConfig::default()
                        .max_message_size(None)
                        .max_frame_size(None),
                ),
            )
            .await
            .unwrap();
            let _ = ws.send(Message::text(payload)).await;
            // Server must close or reject cleanly without panicking
            let _ = timeout(Duration::from_millis(200), ws.next()).await;
        }

        for _ in 0..25 {
            let mut req = format!("ws://{addr}/game").into_client_request().unwrap();
            req.headers_mut()
                .insert("Origin", "http://localhost:8000".parse().unwrap());
            let tcp = TcpStream::connect(addr).await.unwrap();
            let (mut ws, _) = tokio_tungstenite::client_async(req, tcp).await.unwrap();
            let raw = rng.mutate_bytes(
                br#"{"type":"so","room":"r","sub":1,"op":"join","persistent":false}"#,
            );
            let txt = String::from_utf8_lossy(&raw).into_owned();
            let _ = ws.send(Message::text(txt)).await;
            let _ = ws.close(None).await;
        }

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            relay.lock().unwrap().rooms.is_empty(),
            "all disconnected fuzz sessions must clean up relay rooms"
        );
        server_task.abort();
    }
}
