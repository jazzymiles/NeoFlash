//! Nonvisual AS2 collections/data views. Rust owns identity, ordering, cursors,
//! bounds, sorting, filtering and local change transactions; host objects remain
//! opaque handles. References and partial boundaries: docs/AS2-DATA-BATCH.md.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
static NEXT_MODEL: AtomicU32 = AtomicU32::new(1);

const MAX_ROWS: usize = 10_000;
const MAX_VIEWS: usize = 256;
const MAX_CHANGES: usize = 20_000;

#[derive(Clone)]
struct Row {
    id: u32,
    handle: u32,
    keys: BTreeMap<String, Value>,
}
#[derive(Clone, Default)]
struct View {
    ids: Vec<u32>,
    position: i64,
    sort: Option<String>,
    accepted: Option<BTreeSet<u32>>,
    range: Option<(Vec<Value>, Vec<Value>)>,
}
#[derive(Clone, Deserialize)]
struct Sort {
    fields: Vec<String>,
    flags: u32,
    #[serde(default)]
    natural: bool,
}
#[derive(Clone, Serialize)]
struct FieldChange {
    name: String,
    old: u32,
    new: u32,
}
#[derive(Clone, Serialize)]
struct Change {
    id: u32,
    handle: u32,
    operation: u8,
    fields: BTreeMap<String, FieldChange>,
}

pub struct ScriptAS2Data {
    instance: u32,
    rows: Vec<Row>,
    row_index: BTreeMap<u32, usize>,
    next_id: u32,
    views: BTreeMap<u32, View>,
    active: u32,
    next_view: u32,
    sorts: BTreeMap<String, Sort>,
    pending: BTreeMap<u32, Change>,
    log_changes: bool,
    read_only: bool,
    disabled: u32,
    transaction: u32,
    iterators: BTreeMap<u32, (Vec<u32>, usize)>,
    next_iterator: u32,
    next_handle: u32,
}

impl Default for ScriptAS2Data {
    fn default() -> Self {
        Self::new()
    }
}
impl ScriptAS2Data {
    pub fn new() -> Self {
        Self {
            instance: NEXT_MODEL.fetch_add(1, AtomicOrdering::Relaxed),
            rows: Vec::new(),
            row_index: BTreeMap::new(),
            next_id: 1,
            views: BTreeMap::from([(
                0,
                View {
                    position: -1,
                    ..View::default()
                },
            )]),
            active: 0,
            next_view: 1,
            sorts: BTreeMap::new(),
            pending: BTreeMap::new(),
            log_changes: false,
            read_only: false,
            disabled: 0,
            transaction: 0,
            iterators: BTreeMap::new(),
            next_iterator: 1,
            next_handle: 1,
        }
    }
    pub fn execute(&mut self, input: &str) -> Result<String, String> {
        if input.len() > 4 * 1024 * 1024 {
            return Err("AS2 data request exceeds 4 MiB".into());
        }
        let request: Value =
            serde_json::from_str(input).map_err(|e| format!("Invalid AS2 data request: {e}"))?;
        self.apply(&request).map(|v| v.to_string())
    }
    fn writable(&self) -> Result<(), String> {
        if self.read_only {
            return Err("This DataSet is readOnly".into());
        }
        if self.pending.len() >= MAX_CHANGES {
            return Err("Apply pending changes before recording more DataSet edits".into());
        }
        Ok(())
    }
    fn allocate_id(&mut self) -> Result<u32, String> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("DataSet identity limit reached; restart the publication")?;
        Ok(id)
    }
    fn row(&self, id: u32) -> Option<&Row> {
        self.row_index
            .get(&id)
            .and_then(|index| self.rows.get(*index))
    }
    fn index_rows(&mut self) {
        self.row_index = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| (row.id, index))
            .collect();
    }
    fn view(&self, id: u32) -> Result<&View, String> {
        self.views
            .get(&id)
            .ok_or_else(|| "Unknown DataSet iterator".into())
    }
    fn snapshot(&self, view: u32) -> Result<Value, String> {
        let view = self.view(view)?;
        let handles: Vec<u32> = view
            .ids
            .iter()
            .filter_map(|id| self.row(*id).map(|row| row.handle))
            .collect();
        let current = usize::try_from(view.position)
            .ok()
            .and_then(|i| handles.get(i))
            .copied()
            .unwrap_or(0);
        Ok(
            json!({"ids":view.ids,"handles":handles,"position":view.position,"current":current,
            "length":view.ids.len(),"total":self.rows.len(),"logChanges":self.log_changes,
            "readOnly":self.read_only,"disabled":self.disabled,"pending":!self.pending.is_empty(),"active":self.active,
            "filtered":view.accepted.is_some(),"hasNext":view.position>=0&&view.position<(view.ids.len() as i64-1),"hasPrevious":view.position>0}),
        )
    }
    fn rebuilt(&self, view: &View) -> Result<View, String> {
        let current = usize::try_from(view.position)
            .ok()
            .and_then(|i| view.ids.get(i))
            .copied();
        let mut ids: Vec<u32> = self
            .rows
            .iter()
            .filter(|r| view.accepted.as_ref().is_none_or(|a| a.contains(&r.id)))
            .map(|r| r.id)
            .collect();
        if let Some(name) = &view.sort {
            let sort = self
                .sorts
                .get(name)
                .ok_or("The current sort no longer exists")?;
            for id in &ids {
                for field in &sort.fields {
                    if !self.row(*id).unwrap().keys.contains_key(field) {
                        return Err(format!(
                            "Sort field '{field}' is unavailable; refresh primitive field values"
                        ));
                    }
                }
            }
            ids.sort_by(|a, b| compare_rows(self.row(*a).unwrap(), self.row(*b).unwrap(), sort));
            if sort.flags & 4 != 0
                && ids.windows(2).any(|p| {
                    compare_rows(self.row(p[0]).unwrap(), self.row(p[1]).unwrap(), sort)
                        == Ordering::Equal
                })
            {
                return Err("DataSet unique sort contains duplicate key values".into());
            }
            if let Some((start, end)) = &view.range {
                ids.retain(|id| {
                    let row = self.row(*id).unwrap();
                    compare_key(row, start, sort) != Ordering::Less
                        && compare_key(row, end, sort) != Ordering::Greater
                });
            }
        }
        let position = current
            .and_then(|id| ids.iter().position(|value| *value == id))
            .map(|p| p as i64)
            .unwrap_or_else(|| {
                if ids.is_empty() {
                    -1
                } else {
                    view.position.max(0).min(ids.len() as i64 - 1)
                }
            });
        Ok(View {
            ids,
            position,
            ..view.clone()
        })
    }
    fn rebuild(&mut self) -> Result<(), String> {
        let next = self
            .views
            .iter()
            .map(|(id, view)| Ok((*id, self.rebuilt(view)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        self.views = next;
        Ok(())
    }
    fn remove_id(&mut self, id: u32) -> bool {
        let Some(index) = self.rows.iter().position(|r| r.id == id) else {
            return false;
        };
        let row = self.rows.remove(index);
        self.index_rows();
        if self.log_changes {
            if self
                .pending
                .get(&id)
                .is_some_and(|change| change.operation == 0)
            {
                self.pending.remove(&id);
            } else {
                self.pending.insert(
                    id,
                    Change {
                        id,
                        handle: row.handle,
                        operation: 1,
                        fields: BTreeMap::new(),
                    },
                );
            }
        }
        for view in self.views.values_mut() {
            let current = usize::try_from(view.position)
                .ok()
                .and_then(|i| view.ids.get(i))
                .copied();
            view.ids.retain(|value| *value != id);
            view.position = if view.ids.is_empty() {
                -1
            } else {
                current
                    .and_then(|id| view.ids.iter().position(|value| *value == id))
                    .map(|i| i as i64)
                    .unwrap_or_else(|| view.position.max(0).min(view.ids.len() as i64 - 1))
            };
        }
        true
    }
    fn apply(&mut self, r: &Value) -> Result<Value, String> {
        let op = string(r, "op")?;
        let view = r
            .get("view")
            .map(|v| unsigned_value(v, "view"))
            .transpose()?
            .unwrap_or(self.active);
        match op {
            "reference" => {
                if self.next_handle > 100_000 {
                    return Err(
                        "A data model supports at most 100000 referenced values before Restart"
                            .into(),
                    );
                }
                let handle = self.next_handle;
                self.next_handle += 1;
                Ok(json!(handle))
            }
            "snapshot" => self.snapshot(view),
            "all" => Ok(
                json!({"ids":self.rows.iter().map(|v|v.id).collect::<Vec<_>>(),"handles":self.rows.iter().map(|v|v.handle).collect::<Vec<_>>()}),
            ),
            "check_write" => {
                self.writable()?;
                Ok(Value::Null)
            }
            "find_handle" => {
                let handle = unsigned(r, "handle")?;
                let ids = if r["all"].as_bool().unwrap_or(false) {
                    self.rows.iter().map(|row| row.id).collect()
                } else {
                    self.view(view)?.ids.clone()
                };
                Ok(ids
                    .iter()
                    .position(|id| self.row(*id).is_some_and(|row| row.handle == handle))
                    .map(|index| json!({"id":ids[index],"index":index}))
                    .unwrap_or(Value::Null))
            }
            "validate_keys" => {
                self.writable()?;
                let id = r["id"].as_u64().unwrap_or(0) as u32;
                let keys: BTreeMap<String, Value> = serde_json::from_value(r["keys"].clone())
                    .map_err(|_| "Invalid candidate sort keys")?;
                let candidate = Row {
                    id,
                    handle: 0,
                    keys,
                };
                for sort in self.sorts.values() {
                    for field in &sort.fields {
                        if !candidate.keys.contains_key(field) {
                            return Err(format!("Candidate item lacks sort field '{field}'"));
                        }
                    }
                    if sort.flags & 4 != 0
                        && self.rows.iter().any(|row| {
                            row.id != id && compare_rows(row, &candidate, sort) == Ordering::Equal
                        })
                    {
                        return Err("A unique DataSet sort rejects this duplicate item key".into());
                    }
                }
                if let Some(field) = r["field"].as_str()
                    && self.pending.get(&id).is_some_and(|delta| {
                        delta.fields.len() >= 256 && !delta.fields.contains_key(field)
                    })
                {
                    return Err("A local row transaction supports at most 256 fields".into());
                }
                Ok(Value::Null)
            }
            "load" => {
                self.writable()?;
                let handles = handles(r, "handles")?;
                let mut rows = Vec::with_capacity(handles.len());
                for handle in handles {
                    rows.push(Row {
                        id: self.allocate_id()?,
                        handle,
                        keys: BTreeMap::new(),
                    });
                }
                self.rows = rows;
                self.index_rows();
                self.pending.clear();
                self.sorts.clear();
                for view in self.views.values_mut() {
                    *view = View::default();
                }
                self.rebuild()?;
                self.snapshot(view)
            }
            "add" => {
                self.writable()?;
                if self.rows.len() >= MAX_ROWS {
                    return Err("AS2 collections support at most 10000 rows".into());
                }
                let handle = unsigned(r, "handle")?;
                if handle == 0 {
                    return Err("Zero is not an item handle".into());
                }
                if r["unique"].as_bool().unwrap_or(false)
                    && self.rows.iter().any(|row| row.handle == handle)
                {
                    return Ok(json!({"added":false}));
                }
                let index = integer(r, "index")?;
                let current = self.view(view)?;
                if index < 0 || index > current.ids.len() as i64 {
                    return Ok(json!({"added":false}));
                }
                let physical = current
                    .ids
                    .get(index as usize)
                    .and_then(|id| self.rows.iter().position(|row| row.id == *id))
                    .unwrap_or(self.rows.len());
                let id = self.allocate_id()?;
                self.rows.insert(
                    physical,
                    Row {
                        id,
                        handle,
                        keys: BTreeMap::new(),
                    },
                );
                self.index_rows();
                if self.log_changes {
                    self.pending.insert(
                        id,
                        Change {
                            id,
                            handle,
                            operation: 0,
                            fields: BTreeMap::new(),
                        },
                    );
                }
                // The host supplies fresh sort keys and filter outcomes before a view is observed.
                for view in self.views.values_mut() {
                    view.ids.push(id);
                }
                Ok(json!({"added":true,"id":id}))
            }
            "remove" => {
                self.writable()?;
                let id = unsigned(r, "id")?;
                Ok(json!(self.remove_id(id)))
            }
            "remove_many" => {
                self.writable()?;
                let ids = handles(r, "ids")?;
                let removed: Vec<u32> = ids.into_iter().filter(|id| self.remove_id(*id)).collect();
                Ok(json!(removed))
            }
            "contains" => Ok(json!(self.rows.iter().any(|row| {
                Some(row.handle) == r["handle"].as_u64().and_then(|n| u32::try_from(n).ok())
            }))),
            "keys" => {
                let values = r["rows"]
                    .as_array()
                    .ok_or("Data keys require a row array")?;
                if values.len() > MAX_ROWS {
                    return Err("Too many data keys".into());
                }
                let mut updates = Vec::new();
                for value in values {
                    let id = unsigned(value, "id")?;
                    let keys =
                        serde_json::from_value::<BTreeMap<String, Value>>(value["keys"].clone())
                            .map_err(|_| "Invalid data keys")?;
                    if keys.len() > 64
                        || keys
                            .iter()
                            .any(|(name, v)| name.len() > 256 || v.is_array() || v.is_object())
                    {
                        return Err("Sort/search keys require at most 64 primitive fields".into());
                    }
                    updates.push((id, keys));
                }
                for (id, keys) in updates {
                    if let Some(row) = self
                        .row_index
                        .get(&id)
                        .copied()
                        .and_then(|index| self.rows.get_mut(index))
                    {
                        row.keys = keys;
                    }
                }
                Ok(Value::Null)
            }
            "fields" => Ok(json!(
                self.sorts
                    .values()
                    .flat_map(|sort| sort.fields.clone())
                    .collect::<BTreeSet<_>>()
            )),
            "refresh" => {
                self.rebuild()?;
                self.snapshot(view)
            }
            "filter" => {
                let accepted = if r["ids"].is_null() {
                    None
                } else {
                    Some(handles(r, "ids")?.into_iter().collect())
                };
                let mut next = self.view(view)?.clone();
                next.accepted = accepted;
                let next = self.rebuilt(&next)?;
                self.views.insert(view, next);
                self.snapshot(view)
            }
            "sort_add" => {
                let name = string(r, "name")?.to_owned();
                if name.is_empty() || name.len() > 256 || self.sorts.len() >= 64 {
                    return Err(
                        "DataSet sort names require 1–256 characters; at most 64 sorts".into(),
                    );
                }
                if self.sorts.contains_key(&name) {
                    return Err("A DataSet sort with that name already exists".into());
                }
                let sort: Sort = serde_json::from_value(r["sort"].clone())
                    .map_err(|_| "Invalid DataSet sort specification")?;
                if sort.fields.is_empty()
                    || sort.fields.len() > 16
                    || sort.fields.iter().any(|s| s.is_empty() || s.len() > 256)
                    || sort.flags & !23 != 0
                {
                    return Err(
                        "Sort supports 1–16 fields and case/descending/unique/numeric flags".into(),
                    );
                }
                self.sorts.insert(name.clone(), sort);
                let mut next = self.view(view)?.clone();
                next.sort = Some(name.clone());
                next.range = None;
                match self.rebuilt(&next) {
                    Ok(next) => {
                        self.views.insert(view, next);
                        self.snapshot(view)
                    }
                    Err(error) => {
                        self.sorts.remove(&name);
                        Err(error)
                    }
                }
            }
            "sort_has" => Ok(json!(self.sorts.contains_key(string(r, "name")?))),
            "sort_use" => {
                let name = string(r, "name")?.to_owned();
                if !self.sorts.contains_key(&name) {
                    return Err("Unknown DataSet sort".into());
                }
                let mut next = self.view(view)?.clone();
                next.sort = Some(name);
                next.range = None;
                let next = self.rebuilt(&next)?;
                self.views.insert(view, next);
                self.snapshot(view)
            }
            "sort_remove" => {
                let name = string(r, "name")?;
                if self.sorts.remove(name).is_none() {
                    return Err("Unknown DataSet sort".into());
                }
                for v in self.views.values_mut() {
                    if v.sort.as_deref() == Some(name) {
                        v.sort = None;
                        v.range = None;
                    }
                }
                self.rebuild()?;
                self.snapshot(view)
            }
            "range" => {
                let mut next = self.view(view)?.clone();
                if next.sort.is_none() {
                    return Err("setRange requires an active named sort".into());
                }
                next.range = Some((primitive_array(r, "start")?, primitive_array(r, "end")?));
                let next = self.rebuilt(&next)?;
                self.views.insert(view, next);
                self.snapshot(view)
            }
            "range_clear" => {
                self.views
                    .get_mut(&view)
                    .ok_or("Unknown DataSet iterator")?
                    .range = None;
                self.rebuild()?;
                self.snapshot(view)
            }
            "move" => {
                let mode = string(r, "mode")?;
                let v = self
                    .views
                    .get_mut(&view)
                    .ok_or("Unknown DataSet iterator")?;
                let before = v.position;
                let end = v.ids.len() as i64 - 1;
                let next = match mode {
                    "first" => 0,
                    "last" => end,
                    "index" => integer(r, "value")?,
                    "skip" => before.saturating_add(integer(r, "value")?),
                    _ => return Err("Unknown DataSet cursor move".into()),
                };
                v.position = if v.ids.is_empty() {
                    -1
                } else {
                    next.clamp(0, end)
                };
                let moved = v.position - before;
                let mut snapshot = self.snapshot(view)?;
                snapshot["scrolled"] = json!(moved);
                Ok(snapshot)
            }
            "locate" => {
                let id = unsigned(r, "id")?;
                let v = self
                    .views
                    .get_mut(&view)
                    .ok_or("Unknown DataSet iterator")?;
                if let Some(position) = v.ids.iter().position(|v| *v == id) {
                    v.position = position as i64;
                    Ok(json!(true))
                } else {
                    Ok(json!(false))
                }
            }
            "find" => {
                let values = primitive_array(r, "values")?;
                let current = self.view(view)?;
                let sort = self
                    .sorts
                    .get(
                        current
                            .sort
                            .as_deref()
                            .ok_or("find requires an active named sort")?,
                    )
                    .ok_or("Unknown sort")?;
                if values.is_empty() || values.len() > sort.fields.len() {
                    return Err(
                        "Search values must match a nonempty prefix of the active sort fields"
                            .into(),
                    );
                }
                let mut positions = current
                    .ids
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| {
                        compare_key(self.row(**id).unwrap(), &values, sort) == Ordering::Equal
                    })
                    .map(|(i, _)| i);
                let found = if r["last"].as_bool().unwrap_or(false) {
                    positions.next_back()
                } else {
                    positions.next()
                };
                if let Some(position) = found {
                    self.views.get_mut(&view).unwrap().position = position as i64;
                }
                Ok(json!(found.is_some()))
            }
            "clone_view" => {
                if self.views.len() >= MAX_VIEWS {
                    return Err("A DataSet supports at most 256 iterators before Restart".into());
                }
                let next = self.view(view)?.clone();
                let id = self.next_view;
                self.next_view += 1;
                self.views.insert(id, next);
                Ok(json!(id))
            }
            "set_view" => {
                self.view(view)?;
                self.active = view;
                self.snapshot(view)
            }
            "flags" => {
                if let Some(v) = r.get("readOnly") {
                    self.read_only = v.as_bool().ok_or("readOnly requires Boolean")?;
                }
                if let Some(v) = r.get("logChanges") {
                    self.log_changes = v.as_bool().ok_or("logChanges requires Boolean")?;
                }
                self.snapshot(view)
            }
            "disable" => {
                self.disabled = self
                    .disabled
                    .checked_add(1)
                    .ok_or("Too many disableEvents calls")?;
                self.snapshot(view)
            }
            "enable" => {
                self.disabled = self.disabled.saturating_sub(1);
                self.snapshot(view)
            }
            "edit" => {
                self.writable()?;
                let id = unsigned(r, "id")?;
                let name = string(r, "field")?.to_owned();
                if name.is_empty() || name.len() > 256 {
                    return Err("Data fields require 1–256 characters".into());
                }
                let handle = self
                    .row(id)
                    .ok_or("The edited item is no longer in this DataSet")?
                    .handle;
                let old = unsigned(r, "old")?;
                let new = unsigned(r, "new")?;
                if self.log_changes {
                    let delta = self.pending.entry(id).or_insert(Change {
                        id,
                        handle,
                        operation: 2,
                        fields: BTreeMap::new(),
                    });
                    if delta.fields.len() >= 256 && !delta.fields.contains_key(&name) {
                        return Err("A local row transaction supports at most 256 fields".into());
                    }
                    delta
                        .fields
                        .entry(name.clone())
                        .and_modify(|f| f.new = new)
                        .or_insert(FieldChange { name, old, new });
                }
                Ok(Value::Null)
            }
            "apply" => {
                if self.disabled > 0 {
                    return Ok(Value::Null);
                }
                self.transaction = self
                    .transaction
                    .checked_add(1)
                    .ok_or("Data transaction limit reached")?;
                let deltas = std::mem::take(&mut self.pending);
                Ok(
                    json!({"transaction":format!("neoflash-as2-data-{}-{}",self.instance,self.transaction),"changes":deltas.into_values().collect::<Vec<_>>()}),
                )
            }
            "iterator" => {
                if self.iterators.len() >= 1024 {
                    return Err(
                        "At most 1024 snapshot iterators per data model before Restart".into(),
                    );
                }
                let values = handles(r, "handles")?;
                let id = self.next_iterator;
                self.next_iterator += 1;
                self.iterators.insert(id, (values, 0));
                Ok(json!(id))
            }
            "has_next" => {
                let id = unsigned(r, "id")?;
                let (values, index) = self
                    .iterators
                    .get(&id)
                    .ok_or("Unknown collection iterator")?;
                Ok(json!(*index < values.len()))
            }
            "next" => {
                let id = unsigned(r, "id")?;
                let (values, index) = self
                    .iterators
                    .get_mut(&id)
                    .ok_or("Unknown collection iterator")?;
                let handle = values.get(*index).copied().unwrap_or(0);
                if handle != 0 {
                    *index += 1;
                }
                Ok(json!(handle))
            }
            _ => Err(format!("Unknown AS2 data operation '{op}'")),
        }
    }
}
fn string<'a>(r: &'a Value, key: &str) -> Result<&'a str, String> {
    r[key]
        .as_str()
        .ok_or_else(|| format!("{key} requires a string"))
}
fn unsigned_value(v: &Value, key: &str) -> Result<u32, String> {
    v.as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| format!("{key} requires an unsigned integer"))
}
fn unsigned(r: &Value, key: &str) -> Result<u32, String> {
    unsigned_value(&r[key], key)
}
fn integer(r: &Value, key: &str) -> Result<i64, String> {
    r[key]
        .as_i64()
        .ok_or_else(|| format!("{key} requires an integer"))
}
fn handles(r: &Value, key: &str) -> Result<Vec<u32>, String> {
    let values = r[key]
        .as_array()
        .ok_or_else(|| format!("{key} requires an array"))?;
    if values.len() > MAX_ROWS {
        return Err("AS2 collections support at most 10000 rows".into());
    }
    values.iter().map(|v| unsigned_value(v, key)).collect()
}
fn primitive_array(r: &Value, key: &str) -> Result<Vec<Value>, String> {
    let values = r[key]
        .as_array()
        .ok_or_else(|| format!("{key} requires an array"))?;
    if values.len() > 16 || values.iter().any(|v| v.is_array() || v.is_object()) {
        return Err("Search/range values require at most 16 primitives".into());
    }
    Ok(values.clone())
}
fn primitive_string(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        v => v.to_string(),
    }
}
fn compare_value(a: &Value, b: &Value, flags: u32, natural: bool) -> Ordering {
    let order = if (natural || flags & 16 != 0) && a.is_number() && b.is_number() {
        a.as_f64()
            .unwrap()
            .partial_cmp(&b.as_f64().unwrap())
            .unwrap_or(Ordering::Equal)
    } else {
        let mut a = primitive_string(a);
        let mut b = primitive_string(b);
        if flags & 1 != 0 {
            a = a.to_lowercase();
            b = b.to_lowercase();
        }
        a.encode_utf16().cmp(b.encode_utf16())
    };
    if flags & 2 != 0 {
        order.reverse()
    } else {
        order
    }
}
fn compare_rows(a: &Row, b: &Row, sort: &Sort) -> Ordering {
    for field in &sort.fields {
        let order = compare_value(&a.keys[field], &b.keys[field], sort.flags, sort.natural);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}
fn compare_key(row: &Row, values: &[Value], sort: &Sort) -> Ordering {
    for (field, value) in sort.fields.iter().zip(values) {
        let order = compare_value(&row.keys[field], value, sort.flags, sort.natural);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn as2_data_family() {
        let mut data = ScriptAS2Data::new();
        let call = |data: &mut ScriptAS2Data, value: Value| -> Value {
            serde_json::from_str(&data.execute(&value.to_string()).unwrap()).unwrap()
        };
        call(&mut data, json!({"op":"load","handles":[101,102,103]}));
        call(
            &mut data,
            json!({"op":"keys","rows":[{"id":1,"keys":{"rank":3}},{"id":2,"keys":{"rank":1}},{"id":3,"keys":{"rank":2}}]}),
        );
        let sorted = call(
            &mut data,
            json!({"op":"sort_add","name":"rank","sort":{"fields":["rank"],"flags":0,"natural":true}}),
        );
        assert_eq!(sorted["handles"], json!([102, 103, 101]));
        assert_eq!(call(&mut data, json!({"op":"find","values":[2]})), true);
        let clone = call(&mut data, json!({"op":"clone_view"}));
        call(&mut data, json!({"op":"move","view":clone,"mode":"first"}));
        assert_eq!(call(&mut data, json!({"op":"snapshot"}))["current"], 103);
        assert_eq!(
            call(&mut data, json!({"op":"range","start":[2],"end":[3]}))["handles"],
            json!([103, 101])
        );
        assert_eq!(
            call(&mut data, json!({"op":"filter","ids":[1]}))["handles"],
            json!([101])
        );
        call(&mut data, json!({"op":"flags","logChanges":true}));
        call(
            &mut data,
            json!({"op":"edit","id":1,"field":"name","old":400,"new":401}),
        );
        call(
            &mut data,
            json!({"op":"edit","id":1,"field":"name","old":401,"new":402}),
        );
        call(&mut data, json!({"op":"disable"}));
        assert_eq!(call(&mut data, json!({"op":"apply"})), Value::Null);
        call(&mut data, json!({"op":"enable"}));
        let packet = call(&mut data, json!({"op":"apply"}));
        assert_eq!(packet["changes"][0]["fields"]["name"]["old"], 400);
        assert_eq!(packet["changes"][0]["fields"]["name"]["new"], 402);
        assert_eq!(call(&mut data, json!({"op":"snapshot"}))["pending"], false);
        let iterator = call(&mut data, json!({"op":"iterator","handles":[500,600]}));
        assert_eq!(call(&mut data, json!({"op":"next","id":iterator})), 500);
        assert_eq!(call(&mut data, json!({"op":"next","id":iterator})), 600);
        assert_eq!(
            call(&mut data, json!({"op":"has_next","id":iterator})),
            false
        );
        call(&mut data, json!({"op":"flags","readOnly":true}));
        assert!(data.execute(r#"{"op":"remove","id":1}"#).is_err());
    }
}
