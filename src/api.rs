//! The systems the kommuner publish their pickup calendars through. Each one
//! can search for an address and list the coming pickups for it.
//!
//! The endpoints are the ones the kommuners own apps and web pages use, as
//! mapped by the AffaldDK project (https://github.com/briis/affalddk, MIT).

use std::time::Duration;

use chrono::{DateTime, NaiveDate};
use serde_json::{json, Value};

use crate::kommuner::Kommune;

pub const TZ: chrono_tz::Tz = chrono_tz::Europe::Copenhagen;

/// RenoWeb's key for its public calendar service.
const RENOWEB_KEY: &str = "479D40F4-B3E1-4038-9130-76453188C74C";
const RENOWEB: &str = "servicesgh.renoweb.dk/v1_13/";
const PERFECT_WASTE: &str = "europe-west3-perfect-waste.cloudfunctions.net/";
const AFFALDONLINE: &str = "www.affaldonline.dk/api/address/";
const OPENEXP_LIVE: &str = "live.affaldsapi.dk/";
const ODENSE: &str = "mit.odenserenovation.dk/api/Calendar/";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Provider {
    Renoweb,
    PerfectWaste,
    /// The kommune's client id.
    AffaldOnline(&'static str),
    /// Host and path of the kommune's own server.
    OpenExperience(&'static str),
    /// The kommune's number and token.
    OpenExperienceLive(u8, &'static str),
    Odense,
    /// Albertslund, Furesø, Herlev, Ishøj and Vallensbæk.
    Vestfor,
    /// Aarhus.
    Kredslob,
    /// Odder and Skanderborg.
    RenoSyd,
    /// Kolding.
    Infovision,
    /// Haderslev.
    Provas,
    /// Norddjurs and Syddjurs.
    RenoDjurs,
    /// Herning.
    AffaldWeb,
    /// Host and path of the kommune's Renomatic self-service.
    Renomatic(&'static str),
    /// The utility's name in WasteWatch, e.g. Tønder Forsyning's `tonfor`.
    WasteWatch(&'static str),
    /// The kommune's calendar on affaldonline.dk, which has the dates only.
    AffaldOnlineWeb(&'static str),
    /// The kommune's calendar on affaldonline.dk, read from its PDF.
    AffaldOnlinePdf(&'static str),
    Copenhagen,
}

impl Provider {
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Renoweb => "RenoWeb",
            Provider::PerfectWaste => "Perfect Waste",
            Provider::AffaldOnline(_) | Provider::AffaldOnlineWeb(_) | Provider::AffaldOnlinePdf(_) => "AffaldOnline",
            Provider::OpenExperience(_) | Provider::OpenExperienceLive(..) => "OpenExperience",
            Provider::Odense => "Odense Renovation",
            Provider::Vestfor => "Vestforbrænding",
            Provider::Kredslob => "Kredsløb",
            Provider::RenoSyd => "RenoSyd",
            Provider::Infovision => "Infovision",
            Provider::Provas => "Provas",
            Provider::RenoDjurs => "Reno Djurs",
            Provider::AffaldWeb => "Affaldsweb",
            Provider::Renomatic(_) => "Renomatic",
            Provider::WasteWatch(_) => "WasteWatch",
            Provider::Copenhagen => "Affald KBH",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Address {
    pub id: String,
    pub name: String,
}

/// One pickup of one kind of waste, named as the kommune names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pickup {
    pub date: NaiveDate,
    pub fraction: String,
}

#[derive(Debug)]
pub struct ApiError(pub String);

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn err(m: impl std::fmt::Display) -> ApiError {
    ApiError(m.to_string())
}

/// Street and house number from what the user typed: "Skolevej 5 B" gives
/// ("Skolevej", "5B"). The number starts at the first word with a digit.
pub fn split_address(q: &str) -> (String, String) {
    let words: Vec<&str> = q.split_whitespace().collect();
    match words.iter().position(|w| w.starts_with(|c: char| c.is_ascii_digit())) {
        Some(i) if i > 0 => (words[..i].join(" "), words[i..].concat().trim_end_matches(',').to_string()),
        _ => (words.join(" "), String::new()),
    }
}

/// Percent-encode one path segment.
pub(crate) fn segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Clone)]
pub struct Client {
    pub(crate) http: reqwest::Client,
    /// Sends every request here instead, keeping the path. For tests.
    base: Option<String>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Client {
    pub fn new(base: Option<&str>) -> Self {
        Self { http: Self::builder().use_rustls_tls().build().unwrap_or_default(), base: base.map(|b| b.trim_end_matches('/').to_string()) }
    }

    fn builder() -> reqwest::ClientBuilder {
        // some of the servers answer browsers only
        reqwest::Client::builder()
            .user_agent(concat!("Mozilla/5.0 (compatible; HomewrightHQ-affald/", env!("CARGO_PKG_VERSION"), ")"))
            .timeout(Duration::from_secs(30))
    }

    /// A client of its own for Vestforbrænding: it keeps cookies, since the
    /// site remembers the chosen address in the session, and it uses the
    /// system's TLS, since the server offers only the CBC ciphers that
    /// rustls leaves out.
    pub(crate) fn vestfor_client(&self) -> reqwest::Client {
        Self::builder().use_native_tls().cookie_store(true).build().unwrap_or_default()
    }

    /// `at` is host and path prefix, e.g. `servicesgh.renoweb.dk/v1_13/`.
    pub(crate) fn url(&self, at: &str, rest: &str) -> String {
        match &self.base {
            Some(b) => format!("{b}/{}{rest}", at.split_once('/').map(|(_, p)| p).unwrap_or("")),
            None => format!("https://{at}{rest}"),
        }
    }

    /// The body as text. Some servers send Latin-1 without saying so.
    pub(crate) async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, ApiError> {
        let bytes = self.bytes(req).await?;
        Ok(match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
        })
    }

    pub(crate) async fn bytes(&self, req: reqwest::RequestBuilder) -> Result<Vec<u8>, ApiError> {
        let resp = req.send().await.map_err(|e| err(e.without_url()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(err(format!("HTTP {status}")));
        }
        Ok(resp.bytes().await.map_err(|e| err(e.without_url()))?.to_vec())
    }

    pub(crate) async fn json(&self, req: reqwest::RequestBuilder) -> Result<Value, ApiError> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| err(format!("ugyldig JSON: {e}")))
    }

    /// Addresses matching `query` ("Vejnavn 12"), limited to `zip` if given.
    pub async fn search(&self, k: &Kommune, query: &str, zip: Option<&str>) -> Result<Vec<Address>, ApiError> {
        let query = query.trim();
        let mut found = match k.provider {
            Provider::Renoweb => self.renoweb_search(k.code, query).await?,
            Provider::PerfectWaste => self.perfect_waste_search(k.code, query).await?,
            Provider::AffaldOnline(id) => self.affaldonline_search(id, query).await?,
            Provider::OpenExperience(at) => {
                let v = self.json(self.http.get(self.url(at, &format!("search/address/{}/limit/200", segment(query))))).await?;
                list(&v["results"], "addressId", "displayName")
            }
            Provider::OpenExperienceLive(mid, token) => {
                let url = self.url(OPENEXP_LIVE, &format!("address/v1/search/{mid}/{}/100", segment(query)));
                let v = self.json(self.http.get(url).header("X-Auth-Token", token)).await?;
                list(&v, "id", "name")
            }
            Provider::Odense => {
                let req = self.http.get(self.url(ODENSE, "CommunicationHouseNumbers")).query(&[("addressString", query)]);
                list(&self.json(req).await?, "AddressNo", "FullAddress")
            }
            Provider::Vestfor => self.vestfor_search(k.code, query).await?,
            Provider::Kredslob => self.av_search(k.code, query, true).await?,
            Provider::RenoSyd | Provider::WasteWatch(_) => self.av_search(k.code, query, false).await?,
            Provider::Infovision => self.infovision_search(query).await?,
            Provider::Provas => self.provas_search(query).await?,
            Provider::RenoDjurs => self.renodjurs_search(query).await?,
            Provider::AffaldWeb => self.affaldweb_search(query).await?,
            Provider::Renomatic(at) => self.renomatic_search(at, query).await?,
            Provider::AffaldOnlineWeb(slug) | Provider::AffaldOnlinePdf(slug) => self.affaldonline_web_search(slug, query).await?,
            Provider::Copenhagen => self.copenhagen_search(query).await?,
        };
        if let Some(zip) = zip.map(str::trim).filter(|z| !z.is_empty()) {
            found.retain(|a| a.name.contains(zip));
        }
        let mut seen = std::collections::HashSet::new();
        found.retain(|a| seen.insert(a.id.clone()));
        Ok(found)
    }

    async fn renoweb_search(&self, code: u16, query: &str) -> Result<Vec<Address>, ApiError> {
        let (street, number) = split_address(query);
        let code = code.to_string();
        let req = self.http.get(self.url(RENOWEB, "GetJSONRoad.aspx")).query(&[("apikey", RENOWEB_KEY), ("municipalitycode", &code), ("roadname", &street)]);
        let roads = self.json(req).await?;
        let roads: Vec<(String, String)> = roads["list"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| Some((id_of(&r["id"])?, r["name"].as_str()?.to_string())))
            .collect();
        // "Boulevarden (9000)": the same name in each postal district
        let exact: Vec<&(String, String)> =
            roads.iter().filter(|(_, n)| n.split(" (").next().unwrap_or(n).eq_ignore_ascii_case(&street)).collect();
        let roads: Vec<&(String, String)> = if exact.is_empty() { roads.iter().take(5).collect() } else { exact };
        let digits: String = number.chars().take_while(char::is_ascii_digit).collect();
        let mut out = vec![];
        for (road, _) in roads {
            let mut q = vec![("apikey", RENOWEB_KEY.to_string()), ("municipalitycode", code.clone()), ("roadid", road.clone())];
            if !digits.is_empty() {
                q.push(("streetBuildingIdentifier", digits.clone()));
            }
            let v = self.json(self.http.get(self.url(RENOWEB, "GetJSONAdress.aspx")).query(&q)).await?;
            out.extend(list(&v["list"], "id", "presentationString"));
        }
        // "5B" should not offer 5A
        let letter: String = number[digits.len()..].to_lowercase();
        if !letter.is_empty() {
            let wanted = format!("{digits}{letter}");
            out.retain(|a| a.name.to_lowercase().replace(' ', "").contains(&wanted));
        }
        Ok(out)
    }

    async fn perfect_waste_search(&self, code: u16, query: &str) -> Result<Vec<Address>, ApiError> {
        let mut out = vec![];
        for page in 1..=5 {
            let body = json!({ "data": { "query": query, "municipality": code, "page": page, "onlyManual": false } });
            let v = self.json(self.http.post(self.url(PERFECT_WASTE, "searchExternalAddresses")).json(&body)).await?;
            let found = list(&v["result"], "addressID", "displayName");
            let full = found.len() >= 30;
            out.extend(found);
            if !full {
                break;
            }
        }
        Ok(out)
    }

    async fn affaldonline_search(&self, client_id: &str, query: &str) -> Result<Vec<Address>, ApiError> {
        let mut out = vec![];
        for page in 1..=5 {
            let req = self.affaldonline(client_id, "search").query(&[("q", query), ("page", &page.to_string())]);
            let v = self.json(req).await?;
            let found = list(&v["results"], "addressId", "displayName");
            let full = found.len() >= 50;
            out.extend(found);
            if !full {
                break;
            }
        }
        Ok(out)
    }

    fn affaldonline(&self, client_id: &str, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(self.url(AFFALDONLINE, path))
            .header("X-Client-Provider", client_id)
            .header("X-Client-Type", "Kunde app")
            .header("X-Client-Version", "22")
    }

    /// The coming pickups for an address. RenoWeb gives only the next one
    /// for each container.
    pub async fn pickups(&self, k: &Kommune, id: &str) -> Result<Vec<Pickup>, ApiError> {
        match k.provider {
            Provider::Renoweb => {
                let code = k.code.to_string();
                let q = [("apikey", RENOWEB_KEY), ("municipalitycode", &code), ("adressId", id), ("fullinfo", "0"), ("supportsSharedEquipment", "0")];
                let v = self.json(self.http.get(self.url(RENOWEB, "GetJSONContainerList.aspx")).query(&q)).await?;
                Ok(parse_renoweb(&v))
            }
            Provider::PerfectWaste => {
                let body = json!({ "data": { "addressID": id, "municipality": k.code } });
                let url = self.url(PERFECT_WASTE, "getAddressCollections");
                let mut found = parse_perfect_waste(&self.json(self.http.post(&url).json(&body)).await?);
                if found.is_empty() {
                    // Perfect Waste knows only addresses its app has looked up
                    // once; look this one up and ask again
                    let save = json!({ "data": { "addressID": id, "municipality": k.code, "caretakerCode": null, "isCaretaker": null } });
                    self.json(self.http.post(self.url(PERFECT_WASTE, "fetchAddressAndSaveToDb")).json(&save)).await?;
                    found = parse_perfect_waste(&self.json(self.http.post(&url).json(&body)).await?);
                }
                Ok(found)
            }
            Provider::AffaldOnline(client_id) => {
                let v = self.json(self.affaldonline(client_id, "collections").query(&[("groupBy", "date"), ("addressId", id)])).await?;
                Ok(parse_affaldonline(&v))
            }
            Provider::OpenExperience(at) => {
                let v = self.json(self.http.get(self.url(at, &format!("address/{}/collections", segment(id))))).await?;
                Ok(parse_openexp(&v))
            }
            Provider::OpenExperienceLive(mid, token) => {
                let url = self.url(OPENEXP_LIVE, &format!("arrangements/v1/collections/calendar/{mid}/{}", segment(id)));
                let v = self.json(self.http.get(url).header("X-Auth-Token", token)).await?;
                Ok(parse_openexp_live(&v))
            }
            Provider::Odense => {
                let ics = self.text(self.http.get(self.url(ODENSE, "GetICalCalendar")).query(&[("addressNo", id)])).await?;
                Ok(parse_ics(&ics))
            }
            Provider::Vestfor => self.vestfor_pickups(id).await,
            Provider::Kredslob => self.kredslob_pickups(id).await,
            Provider::RenoSyd => self.renosyd_pickups(id).await,
            Provider::Infovision => self.infovision_pickups(id).await,
            Provider::Provas => self.provas_pickups(id).await,
            Provider::RenoDjurs => self.renodjurs_pickups(id).await,
            Provider::AffaldWeb => self.affaldweb_pickups(id).await,
            Provider::Renomatic(at) => self.renomatic_pickups(at, id).await,
            Provider::WasteWatch(utility) => self.wastewatch_pickups(utility, id).await,
            Provider::AffaldOnlineWeb(slug) => self.affaldonline_web_pickups(slug, id).await,
            Provider::AffaldOnlinePdf(slug) => self.affaldonline_pdf_pickups(slug, id).await,
            Provider::Copenhagen => self.copenhagen_pickups(id).await,
        }
    }
}

/// An id that may be a number or a string.
pub(crate) fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn list(v: &Value, id: &str, name: &str) -> Vec<Address> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some(Address { id: id_of(&a[id])?, name: a[name].as_str()?.split_whitespace().collect::<Vec<_>>().join(" ") }))
        .collect()
}

/// "2026-10-28", "2026-10-28T00:00:00.000Z": the date as written.
pub(crate) fn date(v: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v.as_str()?.get(..10)?, "%Y-%m-%d").ok()
}

pub(crate) fn str_of(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

pub fn parse_renoweb(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for c in v["list"].as_array().into_iter().flatten() {
        let ts = c["nextpickupdatetimestamp"].as_str().and_then(|s| s.parse::<i64>().ok());
        let (Some(ts), Some(name)) = (ts, str_of(&c["name"])) else { continue };
        if let Some(t) = DateTime::from_timestamp(ts, 0) {
            out.push(Pickup { date: t.with_timezone(&TZ).date_naive(), fraction: name });
        }
    }
    out
}

pub fn parse_perfect_waste(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for day in v["result"].as_array().into_iter().flatten() {
        let Some(d) = date(&day["date"]) else { continue };
        for f in day["fractions"].as_array().into_iter().flatten() {
            if let Some(name) = str_of(&f["fractionName"]) {
                out.push(Pickup { date: d, fraction: name });
            }
        }
    }
    out
}

pub fn parse_affaldonline(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for day in v.as_array().into_iter().flatten() {
        let Some(d) = date(&day["date"]) else { continue };
        for c in day["collections"].as_array().into_iter().flatten() {
            let name = str_of(&c["fraction"]["name"]).or_else(|| str_of(&c["containers"][0]["description"]));
            if let Some(name) = name {
                out.push(Pickup { date: d, fraction: name });
            }
        }
    }
    out
}

pub fn parse_openexp(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for c in v["collections"].as_array().into_iter().flatten() {
        let Some(name) = str_of(&c["fraction"]["name"]) else { continue };
        for d in c["dates"].as_array().into_iter().flatten().filter_map(date) {
            out.push(Pickup { date: d, fraction: name.clone() });
        }
    }
    out
}

pub fn parse_openexp_live(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for c in v.as_array().into_iter().flatten() {
        let Some(name) = str_of(&c["fraction"]["name"]) else { continue };
        for d in c["upcoming_dates"].as_array().into_iter().flatten().filter_map(|d| date(&d["date"])) {
            out.push(Pickup { date: d, fraction: name.clone() });
        }
    }
    out
}

/// Odense's iCalendar: one event per pickup day, the summary says what,
/// e.g. "Tømning af Restaffald\, Papir & småt pap på Flakhaven 2".
pub fn parse_ics(ics: &str) -> Vec<Pickup> {
    // unfold continuation lines first
    let text = ics.replace("\r\n", "\n").replace("\n ", "").replace("\n\t", "");
    let mut out = vec![];
    let (mut start, mut summary) = (None, None);
    for line in text.lines() {
        if line == "BEGIN:VEVENT" {
            (start, summary) = (None, None);
        } else if let Some(rest) = line.strip_prefix("DTSTART") {
            let value = rest.rsplit(':').next().unwrap_or("");
            start = value.get(..8).and_then(|d| NaiveDate::parse_from_str(d, "%Y%m%d").ok());
        } else if let Some(rest) = line.strip_prefix("SUMMARY") {
            summary = rest.split_once(':').map(|(_, s)| s.to_string());
        } else if line == "END:VEVENT" {
            if let (Some(d), Some(s)) = (start, &summary) {
                let s = s.strip_prefix("Tømning af ").unwrap_or(s);
                let s = s.split(" på ").next().unwrap_or(s);
                for part in s.split("\\,") {
                    if let Some(name) = Some(part.trim()).filter(|p| !p.is_empty()) {
                        out.push(Pickup { date: d, fraction: name.to_string() });
                    }
                }
            }
        }
    }
    out
}
