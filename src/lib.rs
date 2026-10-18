//! Pickup days for household waste from the Danish kommuner, as an external
//! integration for Homewright.
//!
//! An entry is one address. Each kind of waste picked up there becomes a date
//! sensor with the next pickup, and one more sensor tells the next pickup of
//! any kind. The calendar is fetched four times a day, and the sensors move
//! on at midnight.

pub mod api;
pub mod kommuner;
pub mod pdf;
pub mod schedule;
pub mod sources;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration as StdDuration, Instant};

use chrono::{NaiveDate, Utc};
use homewright_plugin::{async_trait, json, Entity, Entry, Error, Flow, FlowReply, Host, Integration, Map, Value};

use api::{Address, Client, TZ};
use kommuner::{Kommune, KOMMUNER};
use schedule::Fraction;

pub const DOMAIN: &str = "affald";
const POLL: StdDuration = StdDuration::from_secs(6 * 3600);
/// After a failed fetch.
const RETRY: StdDuration = StdDuration::from_secs(1800);
/// How many matches the address step offers.
const MAX_MATCHES: usize = 100;

/// Logging to Homewright's log through stderr.
#[doc(hidden)]
pub fn emit(level: &str, message: String) {
    homewright_plugin::log(level, message);
}

#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::emit("INFO", format!($($t)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => { $crate::emit("WARN", format!($($t)*)) };
}

fn user_schema(kommune: &str, address: &str, zip: &str) -> Value {
    let names: Vec<&str> = KOMMUNER.iter().map(|k| k.name).collect();
    let mut kommune_prop = json!({ "type": "string", "title": "Kommune", "enum": names });
    if !kommune.is_empty() {
        kommune_prop["default"] = json!(kommune);
    }
    json!({
        "type": "object",
        "required": ["kommune", "address"],
        "properties": {
            "kommune": kommune_prop,
            "address": { "type": "string", "title": "Adresse", "default": address, "description": "Vejnavn og husnummer, f.eks. Skolevej 5" },
            "zip": { "type": "string", "title": "Postnummer", "default": zip, "description": "Kan udelades. Begrænser søgningen, når vejnavnet findes flere steder i kommunen" }
        }
    })
}

fn pick_schema(found: &[Address]) -> Value {
    let options: Vec<Value> = found.iter().map(|a| json!({ "const": a.id, "title": a.name })).collect();
    json!({
        "type": "object",
        "required": ["address_id"],
        "properties": { "address_id": { "type": "string", "title": "Adresse", "oneOf": options } }
    })
}

fn with_description(mut r: FlowReply, text: &str) -> FlowReply {
    r.result["description"] = json!(text);
    r
}

fn finish(k: &Kommune, a: &Address, mut data: Map<String, Value>) -> FlowReply {
    data.remove("_found");
    data.insert("kommune".into(), json!(k.name));
    data.insert("address_id".into(), json!(a.id));
    data.insert("address".into(), json!(a.name));
    FlowReply::create(&a.name, Value::Object(data)).with_unique_id(format!("{}-{}", k.code, a.id))
}

fn client_for(data: &Map<String, Value>) -> Client {
    Client::new(data.get("base_url").and_then(Value::as_str))
}

fn today() -> NaiveDate {
    Utc::now().with_timezone(&TZ).date_naive()
}

#[derive(Default)]
pub struct Affald {
    tasks: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
}

#[async_trait]
impl Integration for Affald {
    async fn config_flow(&self, _host: &Host, flow: Flow, step: &str, input: Option<Value>) -> Result<FlowReply, Error> {
        match (step, input) {
            ("user", None) => Ok(FlowReply::form("user", user_schema("", "", ""))),
            ("user", Some(v)) => {
                let name = v["kommune"].as_str().unwrap_or_default();
                let query = v["address"].as_str().unwrap_or_default().trim().to_string();
                let zip = v["zip"].as_str().unwrap_or_default().trim().to_string();
                let schema = user_schema(name, &query, &zip);
                let Some(k) = kommuner::find(name) else {
                    return Ok(FlowReply::form_with_errors("user", schema, json!({ "kommune": "not_supported" })));
                };
                if query.is_empty() {
                    return Ok(FlowReply::form_with_errors("user", schema, json!({ "address": "required" })));
                }
                let found = match client_for(&flow.data).search(k, &query, Some(&zip)).await {
                    Ok(f) => f,
                    Err(e) => {
                        info!("{}: {e}", k.provider.label());
                        return Ok(FlowReply::form_with_errors("user", schema, json!({ "base": "cannot_connect" })));
                    }
                };
                match found.len() {
                    0 => {
                        let r = FlowReply::form_with_errors("user", schema, json!({ "address": "invalid_value" }));
                        Ok(with_description(
                            r,
                            &format!("{} Kommune kender ikke adressen \"{query}\". Skriv vejnavnet, som kommunen staver det, og prøv uden husnummerets bogstav.", k.name),
                        ))
                    }
                    1 => Ok(finish(k, &found[0], flow.data.clone())),
                    n => {
                        let shown = &found[..n.min(MAX_MATCHES)];
                        let mut data = flow.data.clone();
                        data.insert("kommune".into(), json!(k.name));
                        data.insert("_found".into(), shown.iter().map(|a| json!({ "id": a.id, "name": a.name })).collect());
                        let r = FlowReply::form("pick", pick_schema(shown)).with_data(data);
                        let text = if n > MAX_MATCHES {
                            format!("{n} adresser passer. Her er de første {MAX_MATCHES}; skriv husnummeret med, hvis din ikke er der.")
                        } else {
                            format!("{n} adresser passer. Vælg din.")
                        };
                        Ok(with_description(r, &text))
                    }
                }
            }
            ("pick", Some(v)) => {
                let k = flow.data.get("kommune").and_then(Value::as_str).and_then(kommuner::find).ok_or_else(|| Error::invalid("ukendt kommune"))?;
                let id = v["address_id"].as_str().unwrap_or_default();
                let found = flow.data.get("_found").and_then(Value::as_array).cloned().unwrap_or_default();
                let a = found
                    .iter()
                    .find(|a| a["id"].as_str() == Some(id))
                    .map(|a| Address { id: id.to_string(), name: a["name"].as_str().unwrap_or(id).to_string() })
                    .ok_or_else(|| Error::invalid("ukendt adresse"))?;
                Ok(finish(k, &a, flow.data.clone()))
            }
            (s, _) => Err(Error::invalid(format!("ukendt trin {s}"))),
        }
    }

    async fn setup_entry(&self, host: &Host, entry: Entry) -> Result<(), Error> {
        let entry_id = entry.entry_id.clone();
        let k = entry.get_str("kommune").and_then(kommuner::find).ok_or_else(|| Error::failed("kommunen understøttes ikke længere"))?;
        let id = entry.get_str("address_id").ok_or_else(|| Error::failed("adressen mangler"))?.to_string();
        let address = entry.get_str("address").unwrap_or(&id).to_string();
        let client = Client::new(entry.get_str("base_url"));

        let pickups = client.pickups(k, &id).await.map_err(|e| Error::not_ready(format!("{}: {e}", k.provider.label())))?;

        let device = json!({
            "identifiers": [[DOMAIN, format!("{}-{id}", k.code)]],
            "name": format!("Affald {address}"),
            "manufacturer": k.provider.label(),
            "model": format!("{} Kommune", k.name),
            "entry_type": "service",
        });
        let dev = host.request("device/register", json!({ "entry_id": entry_id, "device": device })).await?;
        let device_id = dev["id"].as_str().ok_or_else(|| Error::failed("intet enheds-id"))?.to_string();

        let mut sensors = Sensors { host: host.clone(), entry_id: entry_id.clone(), uid: format!("{}-{id}", k.code), device_id, ids: HashMap::new(), next: None };
        sensors.next = sensors.add("next", "Næste tømning", "mdi:calendar-clock").await?;
        sensors.publish(&pickups, today()).await?;

        let label = k.provider.label();
        let task = tokio::spawn(async move {
            let mut pickups = pickups;
            let mut fetched = Instant::now();
            let mut wait = POLL;
            loop {
                let day = today();
                // wake at the next midnight too, so the sensors move on
                let to_midnight = day
                    .succ_opt()
                    .and_then(|d| d.and_hms_opt(0, 0, 30))
                    .and_then(|t| t.and_local_timezone(TZ).earliest())
                    .map(|t| (t.with_timezone(&Utc) - Utc::now()).to_std().unwrap_or_default())
                    .unwrap_or(POLL);
                let sleep = wait.saturating_sub(fetched.elapsed()).min(to_midnight);
                tokio::time::sleep(sleep).await;
                if fetched.elapsed() >= wait {
                    match client.pickups(k, &id).await {
                        Ok(p) => {
                            pickups = p;
                            wait = POLL;
                        }
                        Err(e) => {
                            warn!("{label}: {e}");
                            wait = RETRY;
                        }
                    }
                    fetched = Instant::now();
                }
                if let Err(e) = sensors.publish(&pickups, today()).await {
                    warn!("kunne ikke opdatere: {e}");
                }
            }
        });
        if let Some(old) = self.tasks.lock().unwrap().insert(entry_id, task) {
            old.abort();
        }
        Ok(())
    }

    async fn unload_entry(&self, _host: &Host, entry_id: &str) -> Result<(), Error> {
        if let Some(t) = self.tasks.lock().unwrap().remove(entry_id) {
            t.abort();
        }
        Ok(())
    }
}

/// The sensors of one address. A kind of waste that shows up later gets its
/// sensor then.
struct Sensors {
    host: Host,
    entry_id: String,
    uid: String,
    device_id: String,
    /// fraction key -> entity_id (`None` if the user disabled it)
    ids: HashMap<String, Option<String>>,
    next: Option<String>,
}

impl Sensors {
    async fn add(&self, key: &str, name: &str, icon: &str) -> Result<Option<String>, Error> {
        let e = Entity::new("sensor", format!("{}-{key}", self.uid)).name(name).device(&self.device_id).icon(icon).device_class("date");
        self.host.add_entity(&self.entry_id, e, false).await
    }

    async fn publish(&mut self, pickups: &[api::Pickup], today: NaiveDate) -> Result<(), Error> {
        let fractions = schedule::fractions(pickups, today);
        for f in &fractions {
            if !self.ids.contains_key(&f.key) {
                let id = self.add(&f.key, &f.name, f.icon).await?;
                self.ids.insert(f.key.clone(), id);
            }
        }
        for (key, id) in &self.ids {
            let Some(id) = id else { continue };
            match fractions.iter().find(|f| &f.key == key) {
                Some(f) => self.host.set_state(id, f.dates[0].to_string(), attributes(f, today)).await?,
                // nothing coming for this one any more
                None => self.host.set_state(id, "unknown", Map::new()).await?,
            }
        }
        if let Some(id) = &self.next {
            match schedule::next(&fractions) {
                Some((day, names)) => {
                    let mut a = Map::new();
                    a.insert("days".into(), json!((day - today).num_days()));
                    a.insert("fractions".into(), json!(names));
                    self.host.set_state(id, day.to_string(), a).await?;
                }
                None => self.host.set_state(id, "unknown", Map::new()).await?,
            }
        }
        Ok(())
    }
}

fn attributes(f: &Fraction, today: NaiveDate) -> Map<String, Value> {
    let mut a = Map::new();
    a.insert("days".into(), json!((f.dates[0] - today).num_days()));
    a.insert("dates".into(), json!(f.dates.iter().take(schedule::MAX_DATES).map(|d| d.to_string()).collect::<Vec<_>>()));
    a.insert("description".into(), json!(f.descriptions.join(", ")));
    a
}
