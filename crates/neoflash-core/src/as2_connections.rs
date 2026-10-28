//! AS2 same-origin LocalConnection and AMF0 HTTP remoting protocol/state.
//! Reuses the owned AMF codec. Ruffle MIT contract reference is pinned in
//! docs/AS2-CONNECTIONS-BATCH.md; no AVM or foreign transport engine is embedded.
use crate::amf::{AmfNode, AmfValue, amf_decode};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
const LOCAL_BYTES: usize = 40 * 1024;
const REMOTE_BYTES: usize = 2 * 1024 * 1024;
#[derive(Default)]
struct Local {
    name: Option<String>,
    claimed: bool,
    locked: bool,
}
struct Pending {
    owner: u32,
    wire: String,
    expires: f64,
    next: f64,
}
#[derive(Default)]
struct Remote {
    uri: Option<String>,
    pending: BTreeSet<u32>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u8,
    sender: String,
    id: u32,
    target: String,
    method: String,
    body: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    version: u8,
    ack: u32,
    sender: String,
}
pub struct ScriptAS2Connections {
    origin: String,
    domain: String,
    nonce: String,
    next: u32,
    locals: BTreeMap<u32, Local>,
    pending: BTreeMap<u32, Pending>,
    seen: BTreeMap<(String, u32), f64>,
    remotes: BTreeMap<u32, Remote>,
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str()
        .ok_or_else(|| format!("Missing connection string {k}"))
}
fn id(v: &Value, k: &str) -> Result<u32, String> {
    v[k].as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("Invalid connection identity {k}"))
}
fn args(bytes: &[u8], max: usize) -> Result<(), String> {
    if bytes.len() > max {
        return Err(format!("AMF arguments exceed {max} bytes"));
    }
    let d = amf_decode(0, bytes).map_err(|e| e.to_string())?;
    if d.consumed != bytes.len() {
        return Err("Trailing AMF argument bytes".into());
    }
    if !matches!(d.graph.root,AmfValue::Ref(i) if matches!(d.graph.nodes.get(i),Some(AmfNode::Array{..})))
    {
        return Err("Connection arguments must be an AMF Array".into());
    }
    Ok(())
}
fn method(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 1024
        || value.chars().any(|c| c.is_control())
        || matches!(
            value,
            "__proto__"
                | "prototype"
                | "constructor"
                | "connect"
                | "close"
                | "send"
                | "domain"
                | "onStatus"
                | "allowDomain"
                | "allowInsecureDomain"
        )
    {
        return Err("Invalid or reserved LocalConnection method name".into());
    }
    Ok(())
}
impl ScriptAS2Connections {
    pub fn new(input: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        let origin = text(&v, "origin")?;
        let domain = text(&v, "domain")?;
        let nonce = text(&v, "clientNonce")?;
        if !(origin.starts_with("http://") || origin.starts_with("https://"))
            || domain.is_empty()
            || origin.len() > 4096
            || domain.len() > 256
            || nonce.is_empty()
            || nonce.len() > 128
        {
            return Err(
                "LocalConnection requires HTTP(S) origin, host and browser client identity".into(),
            );
        }
        Ok(Self {
            origin: origin.into(),
            domain: domain.into(),
            nonce: nonce.into(),
            next: 0,
            locals: BTreeMap::new(),
            pending: BTreeMap::new(),
            seen: BTreeMap::new(),
            remotes: BTreeMap::new(),
        })
    }
    fn next(&mut self) -> Result<u32, String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("Connection identity budget exhausted")?;
        Ok(self.next)
    }
    fn name(&self, value: &str, connect: bool) -> Result<String, String> {
        if value.is_empty() || value.len() > 1024 || value.chars().any(|c| c.is_control()) {
            return Err("LocalConnection name requires 1–1024 printable characters".into());
        }
        let name = if let Some((domain, name)) = value.split_once(':') {
            if connect || domain != self.domain || name.is_empty() || name.contains(':') {
                return Err("Only same-origin LocalConnection names are supported; connect names cannot include a domain".into());
            }
            name
        } else {
            value
        };
        Ok(format!("{}|{}", self.origin, name))
    }
    pub fn execute(&mut self, input: &str, bytes: &[u8]) -> Result<Value, String> {
        if input.len() > 128 * 1024 {
            return Err("Connection command exceeds 128 KiB".into());
        }
        let v: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
        let now = v["now"].as_f64().unwrap_or(0.0);
        if !now.is_finite() {
            return Err("Connection clock must be finite".into());
        }
        match text(&v, "op")? {
            "localNew" => {
                if self.locals.len() >= 256 {
                    return Err("At most 256 LocalConnection objects per publication".into());
                }
                let n = self.next()?;
                self.locals.insert(n, Local::default());
                Ok(json!(n))
            }
            "domain" => Ok(json!(self.domain)),
            "user" => {
                let l = self
                    .locals
                    .get_mut(&id(&v, "owner")?)
                    .ok_or("Unknown LocalConnection")?;
                if v["set"] == true {
                    if l.locked {
                        return Err("isPerUser is locked after connect/send/close".into());
                    }
                    if v["value"] != true {
                        return Err("Browser LocalConnection is isolated to the current browser profile; machine-wide scope is unsupported".into());
                    }
                }
                Ok(json!(true))
            }
            "connect" => {
                let n = id(&v, "owner")?;
                let name = self.name(text(&v, "name")?, true)?;
                if !self.locals.contains_key(&n) {
                    return Err("Unknown LocalConnection".into());
                }
                if self.locals[&n].name.is_some()
                    || self
                        .locals
                        .values()
                        .any(|l| l.name.as_deref() == Some(&name))
                {
                    return Ok(json!({"accepted":false}));
                }
                let l = self.locals.get_mut(&n).unwrap();
                l.name = Some(name.clone());
                l.locked = true;
                Ok(json!({"accepted":true,"name":name}))
            }
            "claim" => {
                let l = self
                    .locals
                    .get_mut(&id(&v, "owner")?)
                    .ok_or("Unknown LocalConnection")?;
                if l.name.as_deref() != v["name"].as_str() {
                    return Ok(json!(false));
                }
                l.claimed = v["accepted"] == true;
                if !l.claimed {
                    l.name = None;
                }
                Ok(json!(true))
            }
            "close" => {
                let l = self
                    .locals
                    .get_mut(&id(&v, "owner")?)
                    .ok_or("Unknown LocalConnection")?;
                l.name = None;
                l.claimed = false;
                l.locked = true;
                Ok(Value::Null)
            }
            "send" => {
                let owner = id(&v, "owner")?;
                let target = self.name(text(&v, "name")?, false)?;
                let method_name = text(&v, "method")?;
                method(method_name)?;
                args(bytes, LOCAL_BYTES)?;
                self.locals
                    .get_mut(&owner)
                    .ok_or("Unknown LocalConnection")?
                    .locked = true;
                if self.pending.len() >= 256 {
                    return Err("At most 256 pending LocalConnection messages".into());
                }
                let n = self.next()?;
                let wire = serde_json::to_string(&Wire {
                    version: 1,
                    sender: self.nonce.clone(),
                    id: n,
                    target,
                    method: method_name.into(),
                    body: STANDARD.encode(bytes),
                })
                .map_err(|e| e.to_string())?;
                self.pending.insert(
                    n,
                    Pending {
                        owner,
                        wire: wire.clone(),
                        expires: now + 2000.0,
                        next: now + 100.0,
                    },
                );
                Ok(json!({"id":n,"wire":wire}))
            }
            "incoming" => {
                let wire = text(&v, "wire")?;
                if wire.len() > 65536 {
                    return Err("LocalConnection wire exceeds 64 KiB".into());
                }
                let parsed: Value = serde_json::from_str(wire).map_err(|e| e.to_string())?;
                let acknowledgement = parsed.get("ack").is_some();
                let event = self.execute(&json!({"op":if acknowledgement {"ack"} else {"receive"},"wire":wire,"now":now}).to_string(), &[])?;
                Ok(json!({"kind":if acknowledgement {"ack"} else {"delivery"},"event":event}))
            }
            "receive" => {
                let source = text(&v, "wire")?;
                if source.len() > 65536 {
                    return Err("LocalConnection wire exceeds 64 KiB".into());
                }
                let wire: Wire = serde_json::from_str(source).map_err(|e| e.to_string())?;
                if wire.version != 1
                    || wire.id == 0
                    || wire.sender.is_empty()
                    || wire.sender.len() > 128
                {
                    return Err("Invalid LocalConnection wire identity".into());
                }
                method(&wire.method)?;
                let data = STANDARD.decode(&wire.body).map_err(|e| e.to_string())?;
                args(&data, LOCAL_BYTES)?;
                let Some((&owner, _)) = self
                    .locals
                    .iter()
                    .find(|(_, l)| l.claimed && l.name.as_deref() == Some(&wire.target))
                else {
                    return Ok(Value::Null);
                };
                let ack = serde_json::to_string(&Ack {
                    version: 1,
                    ack: wire.id,
                    sender: wire.sender.clone(),
                })
                .map_err(|e| e.to_string())?;
                let key = (wire.sender, wire.id);
                self.seen.retain(|_, expiry| *expiry > now);
                let duplicate = self.seen.contains_key(&key);
                if !duplicate {
                    if self.seen.len() >= 4096 {
                        return Err("LocalConnection receive budget exceeded".into());
                    }
                    self.seen.insert(key, now + 10_000.0);
                }
                Ok(
                    json!({"owner":owner,"method":wire.method,"body":wire.body,"ack":ack,"duplicate":duplicate}),
                )
            }
            "ack" => {
                let a: Ack = serde_json::from_str(text(&v, "wire")?).map_err(|e| e.to_string())?;
                if a.version != 1 || a.sender != self.nonce {
                    return Ok(Value::Null);
                }
                Ok(self
                    .pending
                    .remove(&a.ack)
                    .map(|p| json!({"owner":p.owner,"id":a.ack,"level":"status"}))
                    .unwrap_or(Value::Null))
            }
            "tick" => {
                let mut outgoing = Vec::new();
                let mut statuses = Vec::new();
                self.pending.retain(|id, p| {
                    if p.expires <= now {
                        statuses.push(json!({"owner":p.owner,"id":id,"level":"error"}));
                        false
                    } else {
                        if p.next <= now {
                            outgoing.push(p.wire.clone());
                            p.next = now + 100.0;
                        }
                        true
                    }
                });
                self.seen.retain(|_, expiry| *expiry > now);
                Ok(json!({"outgoing":outgoing,"statuses":statuses,"pending":self.pending.len()}))
            }
            "remoteNew" => {
                if self.remotes.len() >= 256 {
                    return Err("At most 256 NetConnection records per publication".into());
                }
                let n = self.next()?;
                self.remotes.insert(n, Remote::default());
                Ok(json!(n))
            }
            "remoteConnect" => {
                let uri = text(&v, "uri")?;
                if uri.len() > 8192 || !(uri.starts_with("http://") || uri.starts_with("https://"))
                {
                    return Err("AS2 remoting supports AMF0 HTTP(S), not RTMP/TCP".into());
                }
                let r = self
                    .remotes
                    .get_mut(&id(&v, "owner")?)
                    .ok_or("Unknown NetConnection")?;
                r.pending.clear();
                r.uri = Some(uri.into());
                Ok(json!(true))
            }
            "remoteClose" => {
                let r = self
                    .remotes
                    .get_mut(&id(&v, "owner")?)
                    .ok_or("Unknown NetConnection")?;
                r.pending.clear();
                r.uri = None;
                Ok(Value::Null)
            }
            "remoteState" => {
                let r = self
                    .remotes
                    .get(&id(&v, "owner")?)
                    .ok_or("Unknown NetConnection")?;
                Ok(json!({"uri":r.uri,"isConnected":false}))
            }
            "remoteBegin" => {
                let owner = id(&v, "owner")?;
                let r = self.remotes.get(&owner).ok_or("Unknown NetConnection")?;
                let uri = r
                    .uri
                    .clone()
                    .ok_or("Connect to an AMF HTTP(S) endpoint before call")?;
                if r.pending.len() >= 32 {
                    return Err("At most 32 pending calls per NetConnection".into());
                }
                let n = self.next()?;
                self.remotes.get_mut(&owner).unwrap().pending.insert(n);
                Ok(json!({"id":n,"uri":uri,"response":format!("/{n}")}))
            }
            "remoteFinish" => Ok(json!(
                self.remotes
                    .get_mut(&id(&v, "owner")?)
                    .ok_or("Unknown NetConnection")?
                    .pending
                    .remove(&id(&v, "id")?)
            )),
            "reset" => {
                self.locals.clear();
                self.pending.clear();
                self.seen.clear();
                self.remotes.clear();
                Ok(Value::Null)
            }
            _ => Err("Unknown AS2 connection operation".into()),
        }
    }
}
fn short(out: &mut Vec<u8>, value: &str) -> Result<(), String> {
    if value.len() > 4096 {
        return Err("AMF envelope URI exceeds 4096 UTF-8 bytes".into());
    }
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
pub fn remoting_request(target: &str, response: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    if target.is_empty() || target.chars().any(|c| c.is_control()) || !response.starts_with('/') {
        return Err("Invalid AMF remoting target/response URI".into());
    }
    args(body, REMOTE_BYTES)?;
    let mut out = vec![0, 0, 0, 0, 0, 1];
    short(&mut out, target)?;
    short(&mut out, response)?;
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if out.len() > REMOTE_BYTES {
        return Err("AMF packet exceeds 2 MiB".into());
    }
    Ok(out)
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).ok_or("AMF packet overflow")?;
        let b = self.bytes.get(self.at..end).ok_or("Truncated AMF packet")?;
        self.at = end;
        Ok(b)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        if n > 4096 {
            return Err("AMF URI exceeds 4096 bytes".into());
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "AMF URI must be UTF-8".into())
    }
}
pub fn remoting_response(bytes: &[u8], response: &str) -> Result<Value, String> {
    if bytes.len() > REMOTE_BYTES {
        return Err("AMF response exceeds 2 MiB".into());
    }
    let mut r = Reader { bytes, at: 0 };
    if r.u16()? != 0 {
        return Err("Only AMF0 remoting envelopes are supported".into());
    }
    if r.u16()? != 0 {
        return Err("AMF response envelope headers are not yet supported".into());
    }
    if r.u16()? != 1 {
        return Err("This remoting call expects one response message".into());
    }
    let target = r.string()?;
    let _return_uri = r.string()?;
    let len = r.u32()?;
    let remaining = &r.bytes[r.at..];
    let body = if len == u32::MAX {
        remaining
    } else {
        remaining
            .get(..len as usize)
            .ok_or("Truncated AMF response body")?
    };
    let decoded = amf_decode(0, body).map_err(|e| e.to_string())?;
    if decoded.consumed != body.len() || r.at + body.len() != bytes.len() {
        return Err("AMF response has trailing or inconsistent body bytes".into());
    }
    let kind = if target == format!("{response}/onResult") {
        "result"
    } else if target == format!("{response}/onStatus") {
        "status"
    } else {
        return Err("AMF response target does not match the pending call".into());
    };
    Ok(json!({"kind":kind,"body":STANDARD.encode(body)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amf::{AmfGraph, amf_encode};
    fn execute(core: &mut ScriptAS2Connections, value: Value) -> Value {
        core.execute(&value.to_string(), &[]).unwrap()
    }
    #[test]
    fn as2_connections_family() {
        let config = |nonce: &str| {
            json!({"origin":"https://example.test","domain":"example.test","clientNonce":nonce})
                .to_string()
        };
        let mut sender = ScriptAS2Connections::new(&config("sender")).unwrap();
        let mut receiver = ScriptAS2Connections::new(&config("receiver")).unwrap();
        let sid = execute(&mut sender, json!({"op":"localNew"}));
        let rid = execute(&mut receiver, json!({"op":"localNew"}));
        let claim = execute(
            &mut receiver,
            json!({"op":"connect","owner":rid,"name":"demo"}),
        );
        assert_eq!(claim["accepted"], true);
        assert_eq!(
            execute(
                &mut receiver,
                json!({"op":"connect","owner":rid,"name":"demo"})
            )["accepted"],
            false
        );
        execute(
            &mut receiver,
            json!({"op":"claim","owner":rid,"name":claim["name"],"accepted":true}),
        );
        let arguments = AmfGraph {
            root: AmfValue::Ref(0),
            nodes: vec![AmfNode::Array {
                dense: vec![
                    AmfValue::String("hello".into()),
                    AmfValue::Number("42".into()),
                ],
                entries: vec![],
            }],
        };
        let bytes = amf_encode(0, &arguments).unwrap();
        let outgoing=sender.execute(&json!({"op":"send","owner":sid,"name":"example.test:demo","method":"receive","now":0}).to_string(),&bytes).unwrap();
        let delivered = execute(
            &mut receiver,
            json!({"op":"incoming","wire":outgoing["wire"],"now":0}),
        );
        assert_eq!(delivered["kind"], "delivery");
        assert_eq!(delivered["event"]["duplicate"], false);
        assert_eq!(delivered["event"]["owner"], rid);
        assert_eq!(
            execute(
                &mut receiver,
                json!({"op":"incoming","wire":outgoing["wire"],"now":50})
            )["event"]["duplicate"],
            true
        );
        let ack = execute(
            &mut sender,
            json!({"op":"incoming","wire":delivered["event"]["ack"],"now":50}),
        );
        assert_eq!(ack["event"]["level"], "status");
        assert_eq!(
            execute(
                &mut sender,
                json!({"op":"incoming","wire":delivered["event"]["ack"],"now":60})
            )["event"],
            Value::Null
        );
        sender
            .execute(
                &json!({"op":"send","owner":sid,"name":"absent","method":"receive","now":0})
                    .to_string(),
                &bytes,
            )
            .unwrap();
        assert_eq!(
            execute(&mut sender, json!({"op":"tick","now":150}))["outgoing"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            execute(&mut sender, json!({"op":"tick","now":2100}))["statuses"][0]["level"],
            "error"
        );
        assert!(
            sender
                .execute(
                    &json!({"op":"user","owner":sid,"set":true,"value":false}).to_string(),
                    &[]
                )
                .is_err()
        );
        let packet = remoting_request("demo.echo", "/7", &bytes).unwrap();
        assert_eq!(&packet[..6], &[0, 0, 0, 0, 0, 1]);
        let mut reader = Reader {
            bytes: &packet,
            at: 6,
        };
        assert_eq!(reader.string().unwrap(), "demo.echo");
        assert_eq!(reader.string().unwrap(), "/7");
        assert_eq!(reader.u32().unwrap() as usize, bytes.len());
        assert_eq!(
            amf_decode(0, reader.take(bytes.len()).unwrap())
                .unwrap()
                .graph,
            arguments
        );
        let body = amf_encode(
            0,
            &AmfGraph {
                root: AmfValue::String("actual result".into()),
                nodes: vec![],
            },
        )
        .unwrap();
        let mut response = vec![0, 0, 0, 0, 0, 1];
        short(&mut response, "/7/onResult").unwrap();
        short(&mut response, "null").unwrap();
        response.extend_from_slice(&u32::MAX.to_be_bytes());
        response.extend_from_slice(&body);
        let decoded = remoting_response(&response, "/7").unwrap();
        assert_eq!(decoded["kind"], "result");
        assert_eq!(
            STANDARD.decode(decoded["body"].as_str().unwrap()).unwrap(),
            body
        );
        assert!(remoting_response(&response, "/8").is_err());
        response[2] = 1;
        assert!(remoting_response(&response, "/7").is_err());
        let remote = execute(&mut sender, json!({"op":"remoteNew"}));
        execute(
            &mut sender,
            json!({"op":"remoteConnect","owner":remote,"uri":"https://example.test/amf"}),
        );
        let call = execute(&mut sender, json!({"op":"remoteBegin","owner":remote}));
        execute(&mut sender, json!({"op":"remoteClose","owner":remote}));
        assert_eq!(
            execute(
                &mut sender,
                json!({"op":"remoteFinish","owner":remote,"id":call["id"]})
            ),
            false
        );
    }
}
