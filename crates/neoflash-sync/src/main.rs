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

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireValue {
    encoding: u8,
    bytes: String,
}

impl WireValue {
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
    if !matches!(scheme, "http" | "https")
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
            out.send(
                json!({"type":"so","op":"snapshot","room":key.1,"sub":sub,"fields":room.fields})
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
                    fields = room.fields.clone();
                    for (key, value) in patch {
                        if !field_name(&key) {
                            return Err("Invalid property name.");
                        }
                        match value {
                            Some(v) if v.valid(false) => {
                                fields.insert(key, v);
                            }
                            None => {
                                fields.remove(&key);
                            }
                            _ => return Err("Invalid AMF envelope."),
                        }
                    }
                    event["fields"] = message["fields"].clone();
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
            event["sub"] = json!(token);
            event["own"] = json!(*id == peer);
            let _ = channel.send(event.to_string());
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
}
