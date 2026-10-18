//! The printable tømningskalender from affaldonline.dk, the only place some
//! kommuner say what is picked up when.
//!
//! The PDF is generated: each month is a column of day rows, and the
//! fractions of a day are icons next to it. The legend at the bottom puts the
//! same icons next to the fraction names. So an icon is named by the closest
//! label to its right on the same line, icons side by side with one label
//! form a group, and a day row is read greedily from the largest group it
//! holds. This follows the parser in AffaldDK (https://github.com/briis/affalddk, MIT).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

use chrono::{Datelike, NaiveDate};
use flate2::read::ZlibDecoder;

use crate::api::Pickup;

const MONTHS: [&str; 12] = ["januar", "februar", "marts", "april", "maj", "juni", "juli", "august", "september", "oktober", "november", "december"];
const WEEKDAYS: [&str; 7] = ["man", "tir", "ons", "tor", "fre", "lør", "søn"];
/// Points; icons sit about 1.25 pt below the baseline of their row.
const ROW_TOLERANCE: f64 = 4.0;
/// Points; icons in one legend entry are about 12.76 pt apart.
const GROUP_GAP: f64 = 14.0;

/// A legend entry: its line, its label and its icons with their x.
type Entry<'a> = (f64, &'a str, Vec<(&'a str, f64)>);

enum Op {
    Text(f64, f64, String),
    Icon(f64, f64, String),
}

/// Find `needle` in `hay` from `from`.
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

/// The decompressed content streams: deflated, and not an image or font.
fn streams(pdf: &[u8]) -> Vec<String> {
    let mut out = vec![];
    let mut at = 0;
    while let Some(s) = find(pdf, b"stream", at) {
        at = s + 6;
        // "endstream" contains "stream" too
        if s >= 3 && &pdf[s - 3..s] == b"end" {
            continue;
        }
        let Some(dict_start) = pdf[..s].windows(2).rposition(|w| w == b"<<") else { continue };
        let dict = &pdf[dict_start..s];
        let mut body = s + 6;
        if pdf.get(body) == Some(&b'\r') {
            body += 1;
        }
        if pdf.get(body) == Some(&b'\n') {
            body += 1;
        }
        let Some(end) = find(pdf, b"endstream", body) else { break };
        at = end + 9;
        if find(dict, b"/FlateDecode", 0).is_none() || find(dict, b"/Subtype", 0).is_some() {
            continue;
        }
        let mut raw = vec![];
        if ZlibDecoder::new(&pdf[body..end]).read_to_end(&mut raw).is_ok() || !raw.is_empty() {
            out.push(raw.iter().map(|&b| b as char).collect());
        }
    }
    out
}

fn num(s: &str) -> Option<f64> {
    s.parse().ok()
}

/// "BT 92.13 679.90 Td (JANUAR) Tj ET" and "12 0 0 12 77.5 570.1 cm /I3 Do".
fn ops(content: &str) -> Vec<Op> {
    let mut out = vec![];
    for line in content.lines() {
        if let Some(bt) = line.find("BT ") {
            let rest = &line[bt + 3..];
            let Some(td) = rest.find(" Td (") else { continue };
            let xy: Vec<&str> = rest[..td].split_whitespace().collect();
            let Some(close) = rest.rfind(") Tj") else { continue };
            if xy.len() >= 2 {
                if let (Some(x), Some(y)) = (num(xy[xy.len() - 2]), num(xy[xy.len() - 1])) {
                    out.push(Op::Text(x, y, rest[td + 5..close].trim().to_string()));
                }
            }
        } else if let Some(cm) = line.find(" cm /") {
            let nums: Vec<&str> = line[..cm].split_whitespace().collect();
            let name: String = line[cm + 5..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if nums.len() >= 6 && line[cm..].contains(" Do") && name.starts_with('I') {
                if let (Some(x), Some(y)) = (num(nums[nums.len() - 2]), num(nums[nums.len() - 1])) {
                    out.push(Op::Icon(x, y, name));
                }
            }
        }
    }
    out
}

/// The pickups in the calendar for `year`.
pub fn parse(pdf: &[u8], year: i32) -> Vec<Pickup> {
    let ops: Vec<Op> = streams(pdf).iter().flat_map(|s| ops(s)).collect();
    let is_label = |t: &str| !t.is_empty() && !WEEKDAYS.contains(&t.to_lowercase().as_str()) && !t.chars().all(|c| c.is_ascii_digit());

    // legend: each icon gets the nearest label to its right on its line
    let labels: Vec<(f64, f64, &str)> = ops.iter().filter_map(|o| if let Op::Text(x, y, t) = o { is_label(t).then_some((*x, *y, t.as_str())) } else { None }).collect();
    let mut named: Vec<(f64, f64, &str, &str)> = vec![];
    for o in &ops {
        let Op::Icon(x, y, icon) = o else { continue };
        let best = labels
            .iter()
            .filter(|(lx, ly, _)| lx > x && (ly - y).abs() < ROW_TOLERANCE)
            .min_by(|a, b| (a.0 - x).total_cmp(&(b.0 - x)).then(a.1.total_cmp(&b.1)));
        if let Some((_, _, label)) = best {
            if !named.iter().any(|(nx, ny, ni, _)| nx == x && ny == y && ni == icon) {
                named.push((*x, *y, icon.as_str(), label));
            }
        }
    }
    named.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
    // icons side by side with the same label form one entry
    let mut entries: Vec<Entry> = vec![];
    for (x, y, icon, label) in named {
        match entries.last_mut() {
            Some((ey, el, members)) if *ey == y && *el == label && x - members.last().map(|m| m.1).unwrap_or(x) < GROUP_GAP => {
                members.push((icon, x));
            }
            _ => entries.push((y, label, vec![(icon, x)])),
        }
    }
    let mut groups: Vec<(BTreeSet<&str>, &str)> = entries.into_iter().map(|(_, l, m)| (m.into_iter().map(|(i, _)| i).collect(), l)).collect();
    groups.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.1.cmp(b.1)));
    groups.dedup();

    // the calendar: month headers, then weekday and day number per row
    let mut headers: Vec<(f64, f64, u32)> = vec![];
    let mut weekday: Option<usize> = None;
    let mut current: Option<(NaiveDate, f64)> = None;
    let mut rows: BTreeMap<NaiveDate, Vec<&str>> = BTreeMap::new();
    for o in &ops {
        match o {
            Op::Text(x, y, t) => {
                let lower = t.to_lowercase();
                if let Some(m) = MONTHS.iter().position(|m| *m == lower) {
                    headers.retain(|h| (h.1 - y).abs() < 1.0);
                    headers.push((*x, *y, m as u32 + 1));
                } else if let Some(w) = WEEKDAYS.iter().position(|w| *w == lower) {
                    weekday = Some(w);
                } else if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) && !headers.is_empty() {
                    let Some(w) = weekday.take() else { continue };
                    let month = headers.iter().min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs())).map(|h| h.2).unwrap_or(1);
                    let Some(d) = t.parse().ok().and_then(|day| NaiveDate::from_ymd_opt(year, month, day)) else { continue };
                    // a misread layout shows as the wrong weekday
                    if d.weekday().num_days_from_monday() as usize != w {
                        return vec![];
                    }
                    current = Some((d, *y));
                    rows.entry(d).or_default();
                }
            }
            Op::Icon(_, y, icon) => {
                if let Some((d, cy)) = current {
                    if (cy - y).abs() < ROW_TOLERANCE {
                        rows.entry(d).or_default().push(icon.as_str());
                    }
                }
            }
        }
    }

    let mut out = vec![];
    for (d, icons) in rows {
        let mut left: BTreeSet<&str> = icons.into_iter().collect();
        let mut names: HashMap<&str, ()> = HashMap::new();
        for (set, label) in &groups {
            if set.is_subset(&left) {
                left = left.difference(set).copied().collect();
                names.insert(label, ());
            }
        }
        let mut names: Vec<&str> = names.into_keys().collect();
        names.sort();
        for n in names {
            out.push(Pickup { date: d, fraction: n.to_string() });
        }
    }
    out
}
