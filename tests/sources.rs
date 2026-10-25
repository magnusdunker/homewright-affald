//! The systems of one or a few kommuner each, against answers shaped like
//! the real ones.

use std::io::Write;

use chrono::NaiveDate;
use homewright_affald::api::{Client, Pickup};
use homewright_affald::kommuner;
use homewright_affald::schedule::clean;
use homewright_affald::{pdf, sources};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn p(date: &str, fraction: &str) -> Pickup {
    Pickup { date: d(date), fraction: fraction.into() }
}

#[test]
fn dates_without_a_year_and_in_words() {
    let today = d("2030-10-20");
    assert_eq!(sources::day_month("27-10", today), Some(d("2030-10-27")));
    assert_eq!(sources::day_month(" 3-1", today), Some(d("2031-01-03")));
    assert_eq!(sources::day_month("20-10", today), Some(d("2030-10-20")));
    assert_eq!(sources::danish_date("Tirsdag,  22. oktober 2030"), Some(d("2030-10-22")));
    assert_eq!(sources::danish_date("Mandag, 2. december 2030"), Some(d("2030-12-02")));
}

#[test]
fn names_lose_how_often() {
    assert_eq!(clean("240 ltr cont rest-/madaffald hver 2. uge, bolig"), "Rest-/madaffald");
    assert_eq!(clean("240 ltr supl. cont plast hver 3. uge"), "Plast");
    assert_eq!(clean("660L Rest Tømmes hver 2 uge"), "Rest");
    assert_eq!(clean("140 l madaffald, ugetømning"), "Madaffald");
    assert_eq!(clean("1100 l papaircontainer ugetømning"), "Papaircontainer");
    assert_eq!(clean("770 l plast/kartoner, tømning hver 3. uge"), "Plast/kartoner");
    assert_eq!(clean("660 L beholder til restaffald"), "Restaffald");
    assert_eq!(clean("Tømning"), "Tømning");
}

#[test]
fn json_systems() {
    let v = json!([{ "title": "Rest", "start": "2030-10-21" }, { "title": "Plast & MDK", "start": "2030-10-23" }]);
    assert_eq!(sources::parse_vestfor(&v), vec![p("2030-10-21", "Rest"), p("2030-10-23", "Plast & MDK")]);

    let v = json!([{ "standName": "Testvej 1", "plannedLoads": [
        { "date": "2030-10-21T00:00:00+00:00", "fractions": ["Glas", "Metal"] },
        { "date": "2030-10-22T00:00:00+00:00", "fractions": [] },
    ]}]);
    assert_eq!(sources::parse_kredslob(&v), vec![p("2030-10-21", "Glas, Metal")]);

    let v = json!([{ "planlagtetømninger": [{ "dato": "2030-10-19T00:00:00Z", "fraktioner": ["Restaffald"] }] }]);
    assert_eq!(sources::parse_renosyd(&v), vec![p("2030-10-19", "Restaffald")]);

    let v = json!({ "wastewatch": [{ "container": "Husholdningsaffald", "dato": "2030-10-19", "ugedag": "Lørdag" }] });
    assert_eq!(sources::parse_wastewatch(&v), vec![p("2030-10-19", "Husholdningsaffald")]);

    let v = json!([
        { "containerType": { "description": "240 L beholder til madaffald", "fraction": { "description": "Rest/Madaffald" } },
          "collectCalendar": [{ "collectDate": 20301022 }, { "collectDate": 20301029 }] },
        { "containerType": { "description": "Farligt affald - 21 L" }, "collectCalendar": [] },
    ]);
    assert_eq!(sources::parse_infovision(&v, None), vec![p("2030-10-22", "240 L beholder til madaffald"), p("2030-10-29", "240 L beholder til madaffald")]);

    let v = json!([{ "date": "2030-10-20", "container": { "waste_fraction": { "name": "Pap/papir" } } }]);
    assert_eq!(sources::parse_provas(&v), vec![p("2030-10-20", "Pap/papir")]);
}

#[test]
fn html_systems() {
    let renodjurs = r#"<table class="table table--compact"><thead><tr><th></th><th>Antal</th><th>Ordning</th><th>Tømningsdag</th><th>Næste tømningsdag</th></tr></thead>
        <tbody><tr><td></td><td>3</td><td>140 l madaffald, ugetømning</td><td>15-10-2030</td><td>22-10-2030</td></tr>
        <tr><td></td><td>1</td><td>Fedtudskiller</td><td></td><td></td></tr></tbody></table>"#;
    assert_eq!(sources::parse_renodjurs(renodjurs), vec![p("2030-10-22", "140 l madaffald, ugetømning")]);

    let renomatic = r#"<table class=table><thead><tr><th>Materiel<br /></th><th>Antal</th><th>Ugedag</th><th>Kommende t&#248;mninger</th><th>Pris</th><tr></thead>
        <tbody><tr><td> 3 m3 Nedgravet - Madaffald (<a href="/x">Vis vejninger</a>) </td><td>1</td><td>Tirsdag</td><td>27-10, 10-11</td><td>6.700,00</td></tr></tbody>
        <tfoot><tr><th colspan="4">I alt med moms:</th><th>6.700,00</th></tr></tfoot></table>"#;
    assert_eq!(sources::parse_renomatic(renomatic, d("2030-10-20")), vec![p("2030-10-27", "Madaffald"), p("2030-11-10", "Madaffald")]);

    // Herning: 2030-10-22 is the Tuesday of week 43, 2030-11-12 of week 46
    let herning = r#"<div class="route-card"><div><div class="route-title"> 660 l papir/pap </div></div>
        <div><div class="route-title">Testvej 1</div></div>
        <div class="day-box"><div class="day-name">Tirsdag</div><div>Ugenumre: 43, 46</div></div></div>
        <div class="route-card"><div><div class="route-title">240 l madaffald</div></div>
        <div class="day-box"><div class="day-name">Onsdag</div><div>Ulige uger</div></div></div>"#;
    let h = sources::parse_affaldweb(herning, d("2030-10-20"));
    assert!(h.contains(&p("2030-10-22", "660 l papir/pap")));
    assert!(h.contains(&p("2030-11-12", "660 l papir/pap")));
    assert_eq!(h.iter().filter(|x| x.fraction == "660 l papir/pap").count(), 2);
    // odd weeks: 43 (the 23rd) is odd, 44 (the 30th) is not
    assert!(h.contains(&p("2030-10-23", "240 l madaffald")));
    assert!(!h.contains(&p("2030-10-30", "240 l madaffald")));

    let silkeborg = r#"<table><tr><td colspan="2"><strong>OKTOBER</strong></td></tr>
        <tr><td>22-10</td><td></td></tr><tr><td>29-10</td><td>Restaffald, Madaffald</td></tr></table>"#;
    assert_eq!(
        sources::parse_affaldonline_web(silkeborg, d("2030-10-20")),
        vec![p("2030-10-22", "Tømning"), p("2030-10-29", "Restaffald"), p("2030-10-29", "Madaffald")]
    );

    let kk = r#"<h2>Næste tømning</h2>
        <div class="calendar-waste-date"><div class="date">Tirsdag,  22. oktober 2030
          <div class="waste-fraction"><article><h3> Madaffald </h3></article></div>
          <div class="waste-fraction"><article><h3> Restaffald </h3></article></div></div></div>
        <div class="calendar-waste-date"><div class="date">Onsdag,  23. oktober 2030
          <div class="waste-fraction"><article><h3>Pap</h3></article></div></div></div>"#;
    assert_eq!(sources::parse_copenhagen(kk), vec![p("2030-10-22", "Madaffald"), p("2030-10-22", "Restaffald"), p("2030-10-23", "Pap")]);
}

/// A calendar the way affaldonline.dk draws it: a month header, day rows
/// with icons, and a legend where two icons side by side mean one thing and
/// one of them alone another.
fn calendar_pdf() -> Vec<u8> {
    let content = "\
BT 92.13 680.00 Td (OKTOBER) Tj ET
BT 70.00 660.00 Td (man) Tj ET
BT 92.00 660.00 Td (21) Tj ET
12 0 0 12 110.00 658.75 cm /I1 Do
BT 70.00 645.00 Td (tir) Tj ET
BT 92.00 645.00 Td (22) Tj ET
12 0 0 12 110.00 643.75 cm /I2 Do
12 0 0 12 122.76 643.75 cm /I3 Do
BT 70.00 630.00 Td (ons) Tj ET
BT 92.00 630.00 Td (23) Tj ET
12 0 0 12 110.00 628.75 cm /I3 Do
12 0 0 12 60.00 98.75 cm /I1 Do
BT 80.00 100.00 Td (Restaffald) Tj ET
12 0 0 12 60.00 78.75 cm /I2 Do
12 0 0 12 72.76 78.75 cm /I3 Do
BT 95.00 80.00 Td (Pap/papir) Tj ET
12 0 0 12 60.00 58.75 cm /I3 Do
BT 80.00 60.00 Td (Pap) Tj ET
";
    let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
    z.write_all(content.as_bytes()).unwrap();
    let body = z.finish().unwrap();
    let mut pdf = b"%PDF-1.4\n1 0 obj\n<< /Length 9 /Filter /FlateDecode >>\nstream\n".to_vec();
    pdf.extend_from_slice(&body);
    pdf.extend_from_slice(b"\nendstream\nendobj\n2 0 obj\n<< /Subtype /Image /Filter /FlateDecode >>\nstream\nxx\nendstream\nendobj\n%%EOF\n");
    pdf
}

#[test]
fn affaldonline_pdf() {
    let got = pdf::parse(&calendar_pdf(), 2030);
    assert_eq!(got, vec![p("2030-10-21", "Restaffald"), p("2030-10-22", "Pap/papir"), p("2030-10-23", "Pap")]);
    // a layout read wrong shows up as the wrong weekday, and gives nothing
    assert!(pdf::parse(&calendar_pdf(), 2031).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn kolding_finds_the_house_number() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/publiccitizen/road/roadName/Testvej"))
        .and(header("publicAccessToken", "__NetDialogCitizenPublicAccessToken__"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "guid": "r1", "postalCode": "6000" }])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/publiccitizen/address/info/road/r1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "addressString": "Testvej   3, 6000", "addressGuid": "a3" },
            { "addressString": "Testvej   3A, 6000", "addressGuid": "a3a" },
            { "addressString": "Testvej  30, 6000", "addressGuid": "a30" },
        ])))
        .mount(&server)
        .await;
    let c = Client::new(Some(&server.uri()));
    let k = kommuner::find("Kolding").unwrap();
    let ids = |v: Vec<homewright_affald::api::Address>| v.into_iter().map(|a| a.id).collect::<Vec<_>>();
    assert_eq!(ids(c.search(k, "Testvej 3", None).await.unwrap()), vec!["a3", "a3a"]);
    assert_eq!(ids(c.search(k, "Testvej 3a", None).await.unwrap()), vec!["a3a"]);
}

/// Trimmed from Kolding's answers in October 2026: the building has the
/// bins and the dates, a flat in it only uses them.
#[tokio::test(flavor = "multi_thread")]
async fn kolding_flat_gets_the_building_dates() {
    let container = |chain: &str, owner: &str, name: &str, own: Value| {
        json!({ "chain": chain, "containerDetails": [{
            "container": { "guid": chain, "addressGuid": owner },
            "containerType": { "description": name, "fraction": { "description": "Rest/Madaffald" } },
            "collectCalendar": own,
        }], "calendarDates": [] })
    };
    let mut rest = container("c1", "b1", "400 L beholder til restaffald", json!([{ "collectCalendarGuid": "0", "collectDate": 20301021 }]));
    rest["calendarDates"] = json!([20301021]);
    let mut glass = container("c2", "b1", "400 L beholder til glas og metal", json!([]));
    glass["calendarDates"] = json!([20301025, 20301122]);
    let other = container("c3", "b1", "Farligt affald - 21 L", json!([]));
    let building = json!({ "chainDetailsOwn": [rest, glass, other], "chainDetailsZero": [], "chainDetailsUse": [] });
    let flat = json!({ "chainDetailsOwn": [], "chainDetailsZero": [], "chainDetailsUse": [
        container("c1", "b1", "400 L beholder til restaffald", json!([])),
        container("c2", "b1", "400 L beholder til glas og metal", json!([])),
    ] });

    let server = MockServer::start().await;
    for (id, body) in [("b1", building), ("f1", flat)] {
        Mock::given(method("GET"))
            .and(path(format!("/api/publiccitizen/container/address/active/{id}")))
            .and(header("publicAccessToken", "__NetDialogCitizenPublicAccessToken__"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
    }
    let c = Client::new(Some(&server.uri()));
    let k = kommuner::find("Kolding").unwrap();
    let want = vec![p("2030-10-21", "400 L beholder til restaffald"), p("2030-10-25", "400 L beholder til glas og metal"), p("2030-11-22", "400 L beholder til glas og metal")];
    assert_eq!(c.pickups(k, "b1").await.unwrap(), want);
    assert_eq!(c.pickups(k, "f1").await.unwrap(), want);
}

#[tokio::test(flavor = "multi_thread")]
async fn renosyd_goes_from_address_to_stand() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/husnumre/h1"))
        .and(query_param("token", "adressevaelger123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "husnummer": {
            "husnummertekst": "3", "vejnavn": "Testvej", "navngivenvejkommunedel": { "kommune": "0727", "vejkode": "5893" }
        }})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/adresser/07275893___3_______/standpladser"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "nummer": "009824" }])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/toemmekalender"))
        .and(query_param("nummer", "009824"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{ "planlagtetømninger": [{ "dato": "2030-10-19T00:00:00Z", "fraktioner": ["Glas", "Metal"] }] }])))
        .mount(&server)
        .await;
    let c = Client::new(Some(&server.uri()));
    let got = c.pickups(kommuner::find("Odder").unwrap(), "husnummer:h1").await.unwrap();
    assert_eq!(got, vec![p("2030-10-19", "Glas, Metal")]);
}
