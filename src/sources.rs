//! The systems used by one or a few kommuner each. Most of them are the
//! kommune's or the utility's own web pages, read the way a browser does.

use std::collections::HashSet;

use chrono::{Datelike, Days, NaiveDate, Utc, Weekday};
use scraper::{ElementRef, Html, Selector};
use serde_json::{json, Value};

use crate::api::{date, err, id_of, list, segment, split_address, str_of, Address, ApiError, Client, Pickup, TZ};

const VESTFOR: &str = "selvbetjening.vestfor.dk/";
/// Klimadatastyrelsen's address search, with its public demo token.
const AV: &str = "adressevaelger.dk/";
const AV_TOKEN: &str = "adressevaelger123";
const KREDSLOB: &str = "portal-api.kredslob.dk/api/calendar/";
const RENOSYD: &str = "skoda-selvbetjeningsapi.renosyd.dk/api/v1/";
const INFOVISION: &str = "koldingivapi.infovision.dk/api/publiccitizen/";
/// Infovision's citizen pages send this literal value.
const INFOVISION_TOKEN: &str = "__NetDialogCitizenPublicAccessToken__";
const PROVAS: &str = "platform-api.wastehero.io/api-crm-portal/v1/";
const RENODJURS: &str = "minside.renodjurs.dk/";
const AFFALDWEB: &str = "affaldsweb.net/";
const WASTEWATCH: &str = "wastewatch.forsyningonline.dk/prod/";
const AFFALDONLINE_WEB: &str = "www.affaldonline.dk/kalender/";
const KK_SEARCH: &str = "datascience.kk.dk/affaldkbh/adresse/";
const KK: &str = "affald.kk.dk/";

const MONTHS: [&str; 12] = ["januar", "februar", "marts", "april", "maj", "juni", "juli", "august", "september", "oktober", "november", "december"];

fn today() -> NaiveDate {
    Utc::now().with_timezone(&TZ).date_naive()
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("selector")
}

fn text_of(e: ElementRef) -> String {
    e.text().collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "27-10" without a year: the next such day from `today` on.
pub fn day_month(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let (d, m) = s.trim().split_once(['-', '.', '/'])?;
    let (d, m): (u32, u32) = (d.trim().parse().ok()?, m.trim().trim_end_matches('.').parse().ok()?);
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this >= today {
        Some(this)
    } else {
        NaiveDate::from_ymd_opt(today.year() + 1, m, d)
    }
}

/// "dd-mm-yyyy".
fn dmy(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%d-%m-%Y").ok()
}

/// "Tirsdag, 20. oktober 2026".
pub fn danish_date(s: &str) -> Option<NaiveDate> {
    let words: Vec<String> = s.split([' ', ',', '.']).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let m = words.iter().position(|w| MONTHS.contains(&w.as_str()))?;
    let day: u32 = words.get(m.checked_sub(1)?)?.parse().ok()?;
    let year: i32 = words.get(m + 1)?.parse().ok()?;
    NaiveDate::from_ymd_opt(year, MONTHS.iter().position(|x| *x == words[m])? as u32 + 1, day)
}

fn weekday(name: &str) -> Option<Weekday> {
    Some(match name.trim().to_lowercase().as_str() {
        "mandag" => Weekday::Mon,
        "tirsdag" => Weekday::Tue,
        "onsdag" => Weekday::Wed,
        "torsdag" => Weekday::Thu,
        "fredag" => Weekday::Fri,
        "lørdag" => Weekday::Sat,
        "søndag" => Weekday::Sun,
        _ => return None,
    })
}

/// Percent-encode in Windows-1252, as Affaldsweb expects. Only the Latin-1
/// part is needed for Danish addresses.
fn cp1252(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let b = if (c as u32) < 256 { c as u32 as u8 } else { b'?' };
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// "Street 12B" against an address that begins "Street 12B," or "Street 12 B ".
fn starts_with_address(name: &str, street: &str, number: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let name = norm(name);
    let street = norm(street);
    let Some(rest) = name.strip_prefix(&street) else { return false };
    let rest: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    if number.is_empty() {
        return true;
    }
    let number = number.to_lowercase();
    let Some(after) = rest.strip_prefix(&number) else { return false };
    // "3" finds 3 and 3A but not 30; "3a" finds only 3A
    if number.ends_with(|c: char| c.is_ascii_digit()) {
        !after.starts_with(|c: char| c.is_ascii_digit())
    } else {
        !after.starts_with(|c: char| c.is_ascii_alphanumeric())
    }
}

/// The rows of the first table under `table_sel` as header -> cell text.
fn table_rows(doc: &Html, table_sel: &str) -> Vec<Vec<(String, String)>> {
    let Some(table) = doc.select(&sel(table_sel)).next() else { return vec![] };
    // the header row, not the totals in tfoot
    let head = table.select(&sel("thead th")).map(text_of).collect::<Vec<_>>();
    let headers: Vec<String> = if head.is_empty() { table.select(&sel("tr")).next().map(|tr| tr.select(&sel("th")).map(text_of).collect()).unwrap_or_default() } else { head };
    let mut rows = vec![];
    for tr in table.select(&sel("tr")) {
        let cells: Vec<String> = tr.select(&sel("td")).map(text_of).collect();
        if cells.len() == headers.len() && !cells.is_empty() {
            rows.push(headers.iter().cloned().zip(cells).collect());
        }
    }
    rows
}

fn cell<'a>(row: &'a [(String, String)], header: &str) -> Option<&'a str> {
    row.iter().find(|(h, _)| h.eq_ignore_ascii_case(header)).map(|(_, c)| c.as_str()).filter(|c| !c.is_empty())
}

impl Client {
    // Vestforbrænding -------------------------------------------------------

    pub(crate) async fn vestfor_search(&self, code: u16, query: &str) -> Result<Vec<Address>, ApiError> {
        let req = self.vestfor_client().get(self.url(VESTFOR, "Adresse/AddressByName")).query(&[("term", query), ("numberOfResults", "100")]);
        let v = self.json(req).await?;
        let code = code.to_string();
        let ours: Vec<Value> = v.as_array().into_iter().flatten().filter(|a| a["Kommunekode"].as_str() == Some(&code)).cloned().collect();
        Ok(list(&Value::Array(ours), "Id", "FuldtVejnavn"))
    }

    /// The site keeps the chosen address in the session.
    pub(crate) async fn vestfor_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let http = self.vestfor_client();
        self.text(http.get(self.url(VESTFOR, "Home/MinSide")).query(&[("address-selected-id", id)])).await?;
        let from = today();
        let to = from + Days::new(180);
        let q = [("start", format!("{from}T00:00:00")), ("end", format!("{to}T00:00:00"))];
        match self.json(http.get(self.url(VESTFOR, "Adresse/ToemmeDates")).query(&q)).await {
            Ok(v) => Ok(parse_vestfor(&v)),
            // the answer for an address Vestforbrænding collects nothing from
            Err(e) if e.0.contains("500") => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    // Adressevælger (Aarhus, Odder, Skanderborg, Tønder) ----------------------

    /// Addresses from Klimadatastyrelsen's search. The id is "adresse:<id>"
    /// or "husnummer:<id>"; `only_addresses` keeps the first kind.
    pub(crate) async fn av_search(&self, code: u16, query: &str, only_addresses: bool) -> Result<Vec<Address>, ApiError> {
        let (street, number) = split_address(query);
        let mut q = vec![("vejnavn", street), ("kommunekode", format!("{code:04}")), ("maksimum", "200".into()), ("token", AV_TOKEN.into())];
        if !number.is_empty() {
            q.push(("husnummer", number));
        }
        let v = self.json(self.http.get(self.url(AV, "adresser/soeg")).query(&q)).await?;
        let mut out = vec![];
        for f in v["fund"].as_array().into_iter().flatten() {
            let kind = f["type"].as_str().unwrap_or_default();
            if kind == "adresse" || (kind == "husnummer" && !only_addresses) {
                if let (Some(id), Some(name)) = (str_of(&f["id"]), str_of(&f["titel"])) {
                    out.push(Address { id: if only_addresses { id } else { format!("{kind}:{id}") }, name });
                }
            }
        }
        Ok(out)
    }

    /// The house number part of an Adressevælger address, plus floor and door.
    async fn av_item(&self, id: &str) -> Result<(Value, Option<String>, Option<String>), ApiError> {
        let (kind, id) = id.split_once(':').unwrap_or(("adresse", id));
        let path = if kind == "husnummer" { "husnumre" } else { "adresser" };
        let v = self.json(self.http.get(self.url(AV, &format!("{path}/{}", segment(id)))).query(&[("token", AV_TOKEN)])).await?;
        if kind == "husnummer" {
            return Ok((v["husnummer"].clone(), None, None));
        }
        let a = &v["adresse"];
        Ok((a["husnummer"].clone(), str_of(&a["etagebetegnelse"]), str_of(&a["doerbetegnelse"])))
    }

    // Kredsløb (Aarhus) -------------------------------------------------------

    pub(crate) async fn kredslob_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let v = self.json(self.http.get(self.url(KREDSLOB, &format!("address/{}", segment(id))))).await?;
        Ok(parse_kredslob(&v))
    }

    // RenoSyd (Odder, Skanderborg) -------------------------------------------

    pub(crate) async fn renosyd_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let (h, floor, door) = self.av_item(id).await?;
        let k = &h["navngivenvejkommunedel"];
        let (Some(kom), Some(vej), Some(nr)) = (str_of(&k["kommune"]), str_of(&k["vejkode"]), str_of(&h["husnummertekst"])) else {
            return Err(err("adressen mangler kommune- eller vejkode"));
        };
        // kommune(4) vej(4) husnummer(4) etage(3) dør(4), padded with _
        let kvhx = format!("{kom:_>4}{vej:_>4}{nr:_>4}{:_>3}{:_>4}", floor.unwrap_or_default(), door.unwrap_or_default());
        let stands = match self.json(self.http.get(self.url(RENOSYD, &format!("adresser/{kvhx}/standpladser")))).await {
            Ok(v) => v,
            // an address RenoSyd collects nothing from
            Err(e) if e.0.contains("404") => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let Some(number) = str_of(&stands[0]["nummer"]) else { return Ok(vec![]) };
        let v = self.json(self.http.get(self.url(RENOSYD, "toemmekalender")).query(&[("nummer", &number)])).await?;
        Ok(parse_renosyd(&v))
    }

    // WasteWatch (Tønder) ----------------------------------------------------

    pub(crate) async fn wastewatch_pickups(&self, utility: &str, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let (h, _, _) = self.av_item(id).await?;
        let (Some(road), Some(nr), Some(zip)) = (str_of(&h["vejnavn"]), str_of(&h["husnummertekst"]), str_of(&h["postnummer"]["postnr"])) else {
            return Err(err("adressen mangler vejnavn, husnummer eller postnummer"));
        };
        let digits: String = nr.chars().take_while(char::is_ascii_digit).collect();
        let letter = nr[digits.len()..].trim().to_string();
        let road = road.replace('\'', "''");
        let base = format!("roadName eq '{road}' and houseNumber eq {digits} and postCode eq '{zip}'");
        let url = self.url(WASTEWATCH, utility);
        let mut found = vec![];
        if !letter.is_empty() {
            let v = self.json(self.http.get(&url).query(&[("$filter", format!("{base} and Letter eq '{letter}'"))])).await?;
            found = parse_wastewatch(&v);
        }
        if found.is_empty() {
            found = parse_wastewatch(&self.json(self.http.get(&url).query(&[("$filter", base)])).await?);
        }
        Ok(found)
    }

    // Infovision (Kolding) ---------------------------------------------------

    fn infovision(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(self.url(INFOVISION, path)).header("Accept", "application/json, text/plain, */*").header("publicAccessToken", INFOVISION_TOKEN)
    }

    pub(crate) async fn infovision_search(&self, query: &str) -> Result<Vec<Address>, ApiError> {
        let (street, number) = split_address(query);
        let roads = self.json(self.infovision(&format!("road/roadName/{}", segment(&street)))).await?;
        let mut out = vec![];
        for r in roads.as_array().into_iter().flatten().take(10) {
            let Some(guid) = str_of(&r["guid"]) else { continue };
            let v = self.json(self.infovision(&format!("address/info/road/{}", segment(&guid)))).await?;
            for a in list(&v, "addressGuid", "addressString") {
                if starts_with_address(&a.name, &street, &number) {
                    out.push(a);
                }
            }
        }
        Ok(out)
    }

    /// A flat shares the building's bins, and the dates are only on the
    /// building's own address, so those are looked up when the flat has none.
    pub(crate) async fn infovision_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let v = self.json(self.infovision(&format!("container/address/active/{}", segment(id)))).await?;
        let mut out = parse_infovision(&v, None);
        if out.is_empty() {
            let (owners, chains) = infovision_shared(&v, id);
            for owner in owners.iter().take(3) {
                let w = self.json(self.infovision(&format!("container/address/active/{}", segment(owner)))).await?;
                out.extend(parse_infovision(&w, Some(&chains)));
            }
        }
        Ok(out)
    }

    // Provas (Haderslev) -----------------------------------------------------

    /// The portal logs everyone in anonymously with an empty user.
    async fn provas_key(&self) -> Result<String, ApiError> {
        let v = self.json(self.http.post(self.url(PROVAS, "company/provas-portal.wastehero.io/login")).json(&json!({ "username": "", "password": "" }))).await?;
        str_of(&v["token"]).ok_or_else(|| err("Provas gav ingen nøgle"))
    }

    fn provas(&self, key: &str, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(self.url(PROVAS, path))
            .header("X-API-Key", key)
            .header("Origin", "https://provas-portal.wastehero.io")
            .header("Referer", "https://provas-portal.wastehero.io/")
    }

    pub(crate) async fn provas_search(&self, query: &str) -> Result<Vec<Address>, ApiError> {
        let key = self.provas_key().await?;
        let names = |v: &Value| -> Vec<Address> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some(Address { id: id_of(&p["id"])?, name: str_of(&p["location"]["name"])? }))
                .collect()
        };
        let found = names(&self.json(self.provas(&key, "property/").query(&[("search", query), ("limit", "100")])).await?);
        if !found.is_empty() {
            return Ok(found);
        }
        // the search finds "Gåskærgade" but not always "Gåskærgade 26"
        let (street, number) = split_address(query);
        let all = names(&self.json(self.provas(&key, "property/").query(&[("search", street.as_str()), ("limit", "500")])).await?);
        Ok(all.into_iter().filter(|a| starts_with_address(&a.name, &street, &number)).collect())
    }

    pub(crate) async fn provas_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let key = self.provas_key().await?;
        let from = today();
        let q = [("from_date", from.to_string()), ("to_date", (from + Days::new(120)).to_string())];
        let v = self.json(self.provas(&key, &format!("property/{}/collection_log", segment(id))).query(&q)).await?;
        Ok(parse_provas(&v))
    }

    // Reno Djurs (Norddjurs, Syddjurs) ---------------------------------------

    pub(crate) async fn renodjurs_search(&self, query: &str) -> Result<Vec<Address>, ApiError> {
        let (street, number) = split_address(query);
        let term = if number.is_empty() { format!("{street}*") } else { format!("{street} {number}*") };
        let v = self.json(self.http.post(self.url(RENODJURS, "Default.aspx/GetAddress")).json(&json!({ "address": term }))).await?;
        Ok(list(&v["d"], "value", "label").into_iter().filter(|a| a.id != "0").collect())
    }

    pub(crate) async fn renodjurs_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let html = self.text(self.http.get(self.url(RENODJURS, "Ordninger.aspx")).query(&[("id", id)])).await?;
        Ok(parse_renodjurs(&html))
    }

    // Affaldsweb (Herning) ---------------------------------------------------

    pub(crate) async fn affaldweb_search(&self, query: &str) -> Result<Vec<Address>, ApiError> {
        let url = format!("{}?getCountriesByLetters=1&letters={}", self.url(AFFALDWEB, "dagrenovation/find_veje.php"), cp1252(query));
        // the server says 403 to a POST without a body
        let text = self.text(self.http.post(url).form(&[("getCountriesByLetters", "1")])).await?;
        Ok(text
            .split('|')
            .filter_map(|l| l.split_once("###"))
            .map(|(id, name)| Address { id: id.trim().to_string(), name: name.trim().to_string() })
            .filter(|a| !a.id.is_empty())
            .collect())
    }

    pub(crate) async fn affaldweb_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let html = self.text(self.http.get(self.url(AFFALDWEB, "affaldweb/ruter.php")).query(&[("ejdnr", id)])).await?;
        Ok(parse_affaldweb(&html, today()))
    }

    // Renomatic (Ikast-Brande, Frederikshavn, Vesthimmerland) -----------------

    pub(crate) async fn renomatic_search(&self, at: &str, query: &str) -> Result<Vec<Address>, ApiError> {
        let v = self.json(self.http.post(self.url(at, "Adresse/Typeahead")).json(&json!({ "query": query, "limit": 200 }))).await?;
        Ok(list(&v, "AdresseId", "Beliggenhed"))
    }

    pub(crate) async fn renomatic_pickups(&self, at: &str, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let form = [("hiddenPrevController", "Tomningsinfo"), ("hiddenPrevAction", "Index"), ("hiddenSiteType", "R"), ("hiddenId", id)];
        let html = self.text(self.http.post(self.url(at, "Adresse/SubmitAdresse")).form(&form)).await?;
        Ok(parse_renomatic(&html, today()))
    }

    // affaldonline.dk calendars (Silkeborg; Fanø, Middelfart, Nyborg, Sorø) --

    pub(crate) async fn affaldonline_web_search(&self, slug: &str, query: &str) -> Result<Vec<Address>, ApiError> {
        let (street, number) = split_address(query);
        let streets = self.json(self.http.get(self.url(AFFALDONLINE_WEB, &format!("{slug}/acCal.php"))).query(&[("term", street.as_str())])).await?;
        let mut out = vec![];
        for s in streets.as_array().into_iter().flatten().take(10) {
            let (Some(vej), Some(postnr)) = (str_of(&s["vejnavn"]), id_of(&s["postnr"])) else { continue };
            let by = str_of(&s["Bynavn"]).unwrap_or_default();
            let req = self.http.get(self.url(AFFALDONLINE_WEB, &format!("{slug}/husnrCal.php"))).query(&[("vejnavn", vej.as_str()), ("postnr", postnr.as_str())]);
            let html = self.text(req).await?;
            let doc = Html::parse_document(&html);
            for opt in doc.select(&sel("option")) {
                let (Some(value), nr) = (opt.value().attr("value").filter(|v| !v.is_empty()), text_of(opt)) else { continue };
                let name = format!("{vej} {nr}, {postnr} {by}");
                if starts_with_address(&name, &vej, &number) {
                    out.push(Address { id: value.to_string(), name });
                }
            }
        }
        Ok(out)
    }

    async fn affaldonline_info(&self, slug: &str, id: &str) -> Result<String, ApiError> {
        self.text(self.http.post(self.url(AFFALDONLINE_WEB, &format!("{slug}/showInfo.php"))).form(&[("values", id)])).await
    }

    pub(crate) async fn affaldonline_web_pickups(&self, slug: &str, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let html = self.affaldonline_info(slug, id).await?;
        Ok(parse_affaldonline_web(&html, today()))
    }

    /// The fractions are only in the printable PDF calendar, one per year.
    pub(crate) async fn affaldonline_pdf_pickups(&self, slug: &str, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let html = self.affaldonline_info(slug, id).await?;
        let Some(start) = html.find("showToemCal.php?") else { return Err(err("ingen kalender til adressen")) };
        let query: String = html[start + "showToemCal.php?".len()..].chars().take_while(|c| !matches!(c, '\'' | '"' | ' ' | '>')).collect();
        let params: Vec<(String, String)> = query
            .replace("&amp;", "&")
            .split('&')
            .filter_map(|kv| kv.split_once('='))
            .filter(|(k, _)| *k != "year")
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let now = today();
        let mut out = vec![];
        for year in [now.year(), now.year() + 1] {
            let mut q = params.clone();
            q.push(("year".into(), year.to_string()));
            let pdf = self.bytes(self.http.get(self.url(AFFALDONLINE_WEB, &format!("{slug}/showToemCal.php"))).query(&q)).await;
            // next year's calendar comes late in the year
            let Ok(pdf) = pdf else { continue };
            if pdf.starts_with(b"%PDF") {
                out.extend(crate::pdf::parse(&pdf, year));
            }
        }
        Ok(out)
    }

    // Affald KBH (København) -------------------------------------------------

    /// The search answers with ids only when the query ends with a space.
    pub(crate) async fn copenhagen_search(&self, query: &str) -> Result<Vec<Address>, ApiError> {
        let v = self.json(self.http.post(self.url(KK_SEARCH, "address")).form(&[("searchQuery", format!("{query} "))])).await?;
        let mut out = vec![];
        for a in v["addresses"].as_array().into_iter().flatten() {
            let (Some(id), Some(q)) = (str_of(&a["id"]), str_of(&a["searchQuery"])) else { continue };
            let zip = [str_of(&a["postalCode"]), str_of(&a["city"])].into_iter().flatten().collect::<Vec<_>>().join(" ");
            out.push(Address { id, name: if zip.is_empty() { q } else { format!("{q}, {zip}") } });
        }
        Ok(out)
    }

    pub(crate) async fn copenhagen_pickups(&self, id: &str) -> Result<Vec<Pickup>, ApiError> {
        let html = self.text(self.http.get(self.url(KK, "din-kalender")).query(&[("address_id", id)])).await?;
        Ok(parse_copenhagen(&html))
    }
}

pub fn parse_vestfor(v: &Value) -> Vec<Pickup> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some(Pickup { date: date(&e["start"])?, fraction: str_of(&e["title"])? }))
        .collect()
}

/// One load can take several fractions, e.g. ["Glas", "Metal"] from one
/// two-chamber bin; they become one name.
pub fn parse_kredslob(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for stand in v.as_array().into_iter().flatten() {
        for load in stand["plannedLoads"].as_array().into_iter().flatten() {
            let Some(d) = date(&load["date"]) else { continue };
            let names: Vec<String> = load["fractions"].as_array().into_iter().flatten().filter_map(str_of).collect();
            if !names.is_empty() {
                out.push(Pickup { date: d, fraction: names.join(", ") });
            }
        }
    }
    out
}

pub fn parse_renosyd(v: &Value) -> Vec<Pickup> {
    let mut out = vec![];
    for stand in v.as_array().into_iter().flatten() {
        for t in stand["planlagtetømninger"].as_array().into_iter().flatten() {
            let Some(d) = date(&t["dato"]) else { continue };
            let names: Vec<String> = t["fraktioner"].as_array().into_iter().flatten().filter_map(str_of).collect();
            if !names.is_empty() {
                out.push(Pickup { date: d, fraction: names.join(", ") });
            }
        }
    }
    out
}

pub fn parse_wastewatch(v: &Value) -> Vec<Pickup> {
    v["wastewatch"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some(Pickup { date: date(&e["dato"])?, fraction: str_of(&e["container"])? }))
        .collect()
}

/// The containers as (chain, container details, the chain's dates). The
/// answer is grouped in chains the address owns, has with no fee and uses;
/// an older form was the containers as a plain list.
fn infovision_containers(v: &Value) -> Vec<(Option<String>, &Value, &Value)> {
    if let Some(a) = v.as_array() {
        return a.iter().map(|c| (None, c, &Value::Null)).collect();
    }
    let mut out = vec![];
    for group in ["chainDetailsOwn", "chainDetailsZero", "chainDetailsUse"] {
        for chain in v[group].as_array().into_iter().flatten() {
            for c in chain["containerDetails"].as_array().into_iter().flatten() {
                out.push((str_of(&chain["chain"]), c, &chain["calendarDates"]));
            }
        }
    }
    out
}

/// The containers' dates, from `collectCalendar` or else the chain's
/// `calendarDates`. With `only`, just those chains.
pub fn parse_infovision(v: &Value, only: Option<&HashSet<String>>) -> Vec<Pickup> {
    let mut out = vec![];
    for (chain, c, chain_dates) in infovision_containers(v) {
        if let Some(only) = only {
            if !chain.as_ref().is_some_and(|ch| only.contains(ch)) {
                continue;
            }
        }
        let t = &c["containerType"];
        let Some(name) = str_of(&t["description"]).or_else(|| str_of(&t["fraction"]["description"])) else { continue };
        let own: Vec<&Value> = c["collectCalendar"].as_array().into_iter().flatten().map(|d| &d["collectDate"]).collect();
        let days = if own.is_empty() { chain_dates.as_array().into_iter().flatten().collect() } else { own };
        for d in days {
            let raw = d.as_i64().map(|n| n.to_string()).or_else(|| str_of(d));
            if let Some(day) = raw.and_then(|r| NaiveDate::parse_from_str(&r, "%Y%m%d").ok()) {
                out.push(Pickup { date: day, fraction: name.clone() });
            }
        }
    }
    out
}

/// For an address that only uses other addresses' containers: those
/// addresses and the chains in question.
pub fn infovision_shared(v: &Value, id: &str) -> (Vec<String>, HashSet<String>) {
    let mut owners = vec![];
    let mut chains = HashSet::new();
    for (chain, c, _) in infovision_containers(v) {
        let Some(owner) = str_of(&c["container"]["addressGuid"]) else { continue };
        if owner != id {
            if !owners.contains(&owner) {
                owners.push(owner);
            }
            chains.extend(chain);
        }
    }
    (owners, chains)
}

pub fn parse_provas(v: &Value) -> Vec<Pickup> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some(Pickup { date: date(&e["date"])?, fraction: str_of(&e["container"]["waste_fraction"]["name"])? }))
        .collect()
}

/// The table of schemes with "Ordning" and "Næste tømningsdag".
pub fn parse_renodjurs(html: &str) -> Vec<Pickup> {
    let doc = Html::parse_document(html);
    table_rows(&doc, "table.table--compact")
        .iter()
        .filter_map(|r| Some(Pickup { date: dmy(cell(r, "Næste tømningsdag")?)?, fraction: cell(r, "Ordning")?.to_string() }))
        .collect()
}

/// Route cards with a weekday and the weeks: "Ugenumre: 43, 46, 49",
/// "Ulige uger" or "Lige uger". The dates are worked out for the next four
/// months.
pub fn parse_affaldweb(html: &str, today: NaiveDate) -> Vec<Pickup> {
    let doc = Html::parse_document(html);
    let mut out = vec![];
    let mut seen = HashSet::new();
    for card in doc.select(&sel("div.route-card")) {
        let Some(fraction) = card.select(&sel("div.route-title")).next().map(text_of) else { continue };
        let Some(day_box) = card.select(&sel("div.day-box")).next() else { continue };
        let Some(day) = day_box.select(&sel("div.day-name")).next().map(text_of).and_then(|d| weekday(&d)) else { continue };
        let rule = text_of(day_box).to_lowercase();
        let weeks: Option<HashSet<u32>> =
            rule.split_once("ugenumre:").map(|(_, w)| w.split(',').filter_map(|n| n.split_whitespace().next()?.parse().ok()).collect());
        for i in 0..120 {
            let d = today + Days::new(i);
            if d.weekday() != day {
                continue;
            }
            let week = d.iso_week().week();
            let ok = match &weeks {
                Some(set) => set.contains(&week),
                None if rule.contains("ulige") => week % 2 == 1,
                None if rule.contains("lige") => week.is_multiple_of(2),
                None => true,
            };
            if ok && seen.insert((d, fraction.clone())) {
                out.push(Pickup { date: d, fraction: fraction.clone() });
            }
        }
    }
    out
}

/// "Materiel" and "Kommende tømninger" as "27-10, 10-11, 24-11".
pub fn parse_renomatic(html: &str, today: NaiveDate) -> Vec<Pickup> {
    let doc = Html::parse_document(html);
    let mut out = vec![];
    for r in table_rows(&doc, "table") {
        let (Some(name), Some(dates)) = (cell(&r, "Materiel"), cell(&r, "Kommende tømninger")) else { continue };
        // "3 m3 Nedgravet - Madaffald (Vis vejninger)"
        let name = match name.find(" (") {
            Some(at) if name[at..].contains("vejninger") => name[..at].trim(),
            _ => name.trim(),
        };
        let name = name.rsplit_once(" - ").map(|(_, n)| n).unwrap_or(name);
        for d in dates.split(',').filter_map(|d| day_month(d, today)) {
            out.push(Pickup { date: d, fraction: name.to_string() });
        }
    }
    out
}

/// A table of "dd-mm" and what is picked up. Silkeborg leaves the second
/// column empty, so those days are just "Tømning".
pub fn parse_affaldonline_web(html: &str, today: NaiveDate) -> Vec<Pickup> {
    let doc = Html::parse_document(html);
    let mut out = vec![];
    for tr in doc.select(&sel("tr")) {
        let cells: Vec<String> = tr.select(&sel("td")).map(text_of).collect();
        if cells.len() != 2 {
            continue;
        }
        let names: Vec<String> = cells[1].split(", ").map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        let names = if names.is_empty() { vec!["Tømning".to_string()] } else { names };
        for d in cells[0].split(',').filter_map(|d| day_month(d, today)) {
            for n in &names {
                out.push(Pickup { date: d, fraction: n.clone() });
            }
        }
    }
    out
}

/// Day blocks "Tirsdag, 20. oktober 2026" with the fractions as headings.
pub fn parse_copenhagen(html: &str) -> Vec<Pickup> {
    let doc = Html::parse_document(html);
    let mut out = vec![];
    for block in doc.select(&sel("div.calendar-waste-date")) {
        let Some(date_div) = block.select(&sel("div.date")).next() else { continue };
        let first: String = date_div.text().map(str::trim).find(|t| !t.is_empty()).unwrap_or_default().to_string();
        let Some(d) = danish_date(&first) else { continue };
        for h in block.select(&sel("h3")) {
            let name = text_of(h);
            if !name.is_empty() {
                out.push(Pickup { date: d, fraction: name });
            }
        }
    }
    out
}
