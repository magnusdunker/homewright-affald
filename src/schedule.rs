//! From a list of pickups to one entry per kind of waste.
//!
//! The kommuner name the same waste in many ways: "Restaffald/Madaffald 240
//! L", "240 L Dagrenovation 2-delt", "PAPPI". The name is kept as the kommune
//! writes it, minus container sizes and the postal district, so a sensor
//! reads the way the kommune's own calendar does.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::api::Pickup;

/// How many coming dates a sensor lists.
pub const MAX_DATES: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct Fraction {
    pub key: String,
    pub name: String,
    pub icon: &'static str,
    /// Coming dates, sorted, today included.
    pub dates: Vec<NaiveDate>,
    /// The kommune's own names behind this one.
    pub descriptions: Vec<String>,
}

fn is_volume(w: &str) -> bool {
    let w = w.to_lowercase();
    let digits = w.trim_start_matches(|c: char| c.is_ascii_digit());
    digits.len() < w.len() && matches!(digits, "" | "l" | "l." | "ltr" | "ltr." | "liter" | "m3")
}

/// The name to show for a fraction as the kommune writes it.
pub fn clean(name: &str) -> String {
    let mut s = name.trim().to_string();
    // "Asbest - 9000 Aalborg": the district adds nothing
    if let Some((head, tail)) = s.rsplit_once(" - ") {
        if tail.len() >= 4 && tail[..4].chars().all(|c| c.is_ascii_digit()) {
            s = head.to_string();
        }
    }
    // "Restaffald hver 2. uge, bolig", "madaffald, ugetømning", "660L Rest
    // Tømmes hver 2 uge": how often belongs in the dates, not the name
    let lower = s.to_lowercase();
    let cut = [" hver ", " tømmes ", ", ugetømning", " ugetømning", ", tømning", ", bolig"].iter().filter_map(|p| lower.find(p)).min();
    if let Some(at) = cut.filter(|at| *at > 0) {
        s.truncate(at);
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    let mut kept: Vec<&str> = vec![];
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        // "240 ltr supl. cont plast", "660 L beholder til restaffald"
        let lw = w.to_lowercase();
        if matches!(lw.as_str(), "cont" | "cont." | "supl." | "suppleringscont" | "suppleringscontainer") {
            i += 1;
            continue;
        }
        if lw == "beholder" && words.get(i + 1).map(|n| n.to_lowercase()) == Some("til".into()) {
            i += 2;
            continue;
        }
        let unit = words.get(i + 1).map(|n| matches!(n.to_lowercase().as_str(), "l" | "l." | "ltr" | "ltr." | "liter" | "m3"));
        if w.chars().all(|c| c.is_ascii_digit()) && unit == Some(true) {
            i += 2;
            continue;
        }
        if is_volume(w) {
            i += 1;
            continue;
        }
        kept.push(w);
        i += 1;
    }
    let s = kept.join(" ");
    let s = s.trim_matches(|c: char| c == ',' || c == '-' || c.is_whitespace());
    let s = if s.is_empty() { name.trim() } else { s };
    // "PAPPI" and "Dagrenovation" alike: first letter upper, the rest as written,
    // unless it is all capitals
    let s = if s.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase) && s.chars().count() > 4 {
        s.to_lowercase()
    } else {
        s.to_string()
    };
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => s,
    }
}

/// A stable id part: lower case ASCII, æøå spelled out.
pub fn key(name: &str) -> String {
    let mut out = String::new();
    for ch in name.to_lowercase().chars() {
        match ch {
            'æ' => out.push_str("ae"),
            'ø' => out.push_str("oe"),
            'å' => out.push_str("aa"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            _ => {
                if !out.ends_with('_') {
                    out.push('_');
                }
            }
        }
    }
    out.trim_matches('_').to_string()
}

pub fn icon(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |w: &[&str]| w.iter().any(|w| n.contains(w));
    if has(&["farlig", "miljø", "batteri"]) {
        "mdi:biohazard"
    } else if has(&["have", "grene"]) {
        "mdi:leaf"
    } else if has(&["storskrald"]) {
        "mdi:sofa"
    } else if has(&["tekstil"]) {
        "mdi:tshirt-crew"
    } else if has(&["elektronik"]) {
        "mdi:power-plug"
    } else if has(&["juletræ"]) {
        "mdi:pine-tree"
    } else if has(&["rest", "dagrenovation"]) {
        "mdi:trash-can"
    } else if has(&["mad", "bio"]) {
        "mdi:food-apple"
    } else if has(&["genbrug", "genanvend", "pappi"]) {
        "mdi:recycle"
    } else if has(&["papir", "pap"]) {
        "mdi:newspaper-variant"
    } else if has(&["plast"]) {
        "mdi:bottle-soda-classic"
    } else if has(&["glas"]) {
        "mdi:glass-fragile"
    } else if has(&["metal", "jern"]) {
        "mdi:anvil"
    } else {
        "mdi:trash-can-outline"
    }
}

/// One entry per kind of waste with dates from `today` on, sorted by the
/// next date.
pub fn fractions(pickups: &[Pickup], today: NaiveDate) -> Vec<Fraction> {
    let mut by_key: BTreeMap<String, Fraction> = BTreeMap::new();
    for p in pickups.iter().filter(|p| p.date >= today) {
        let name = clean(&p.fraction);
        let k = key(&name);
        if k.is_empty() {
            continue;
        }
        let f = by_key.entry(k.clone()).or_insert_with(|| Fraction { key: k, icon: icon(&name), name, dates: vec![], descriptions: vec![] });
        f.dates.push(p.date);
        if !f.descriptions.contains(&p.fraction) {
            f.descriptions.push(p.fraction.clone());
        }
    }
    let mut out: Vec<Fraction> = by_key.into_values().collect();
    for f in &mut out {
        f.dates.sort();
        f.dates.dedup();
    }
    out.sort_by(|a, b| a.dates.first().cmp(&b.dates.first()).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The next pickup day and what is picked up then.
pub fn next(fractions: &[Fraction]) -> Option<(NaiveDate, Vec<String>)> {
    let day = fractions.iter().filter_map(|f| f.dates.first()).min().copied()?;
    let names = fractions.iter().filter(|f| f.dates.first() == Some(&day)).map(|f| f.name.clone()).collect();
    Some((day, names))
}
