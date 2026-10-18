//! Parsing against responses shaped like the real ones, and the plugin
//! against a fake RenoWeb and Perfect Waste, with Homewright played over a
//! pipe.

mod host;

use chrono::NaiveDate;
use homewright_affald::api::{self, split_address, Pickup};
use homewright_affald::schedule::{self, clean, key};
use homewright_affald::Affald;
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn addresses_split_into_street_and_number() {
    assert_eq!(split_address("Skolevej 5 B"), ("Skolevej".into(), "5B".into()));
    assert_eq!(split_address("  Ny Østergade 12, "), ("Ny Østergade".into(), "12".into()));
    assert_eq!(split_address("Boulevarden"), ("Boulevarden".into(), "".into()));
}

#[test]
fn names_lose_sizes_and_districts() {
    assert_eq!(clean("240 L Dagrenovation 2-delt"), "Dagrenovation 2-delt");
    assert_eq!(clean("Cube 1500 L (Flasker)"), "Cube (Flasker)");
    assert_eq!(clean("Vippecontainer 10 m3 (Restaffald)"), "Vippecontainer (Restaffald)");
    assert_eq!(clean("Restaffald/Madaffald 240L"), "Restaffald/Madaffald");
    assert_eq!(clean("Asbest - 9000 Aalborg"), "Asbest");
    assert_eq!(clean("PAPPI"), "Pappi");
    assert_eq!(clean("Dagrenovation "), "Dagrenovation");
    assert_eq!(key("Papir & småt pap"), "papir_smaat_pap");
    assert_eq!(key("Glas/Metal"), "glas_metal");
    assert_eq!(schedule::icon("Restaffald-Madaffald"), "mdi:trash-can");
    assert_eq!(schedule::icon("Papir & småt pap"), "mdi:newspaper-variant");
}

#[test]
fn renoweb_gives_the_next_date_per_container() {
    let v = json!({ "list": [
        { "name": "Restaffald 240 L", "nextpickupdatetimestamp": "1792368000" },
        { "name": "Bestillerordning - 9000 Aalborg", "nextpickupdatetimestamp": "" },
    ]});
    // midnight UTC is 02:00 in Copenhagen in October: the same day
    assert_eq!(api::parse_renoweb(&v), vec![Pickup { date: d("2026-10-19"), fraction: "Restaffald 240 L".into() }]);
}

#[test]
fn perfect_waste_affaldonline_and_openexperience() {
    let pw = json!({ "result": [
        { "date": "2030-10-28T00:00:00.000Z", "fractions": [{ "fractionName": "Restaffald-Madaffald" }, { "fractionName": "PAPPI" }] },
    ]});
    assert_eq!(api::parse_perfect_waste(&pw).len(), 2);
    assert_eq!(api::parse_perfect_waste(&pw)[1], Pickup { date: d("2030-10-28"), fraction: "PAPPI".into() });

    let ao = json!([
        { "date": "2030-10-23", "collections": [{ "fraction": { "name": "Dagrenovation " }, "containers": [{ "description": "240 L Dagrenovation 2-delt" }] }] },
        { "date": "2030-10-26", "collections": [{ "fraction": { "name": "" }, "containers": [{ "description": "240 L Genanvendeligt 2-delt" }] }] },
    ]);
    let p = api::parse_affaldonline(&ao);
    assert_eq!(p[0].fraction, "Dagrenovation");
    assert_eq!(p[1].fraction, "240 L Genanvendeligt 2-delt");

    let oe = json!({ "collections": [{ "fraction": { "name": "Papir/pap" }, "dates": ["2030-10-20", "2030-11-03"] }] });
    assert_eq!(api::parse_openexp(&oe).len(), 2);
    let live = json!([{ "fraction": { "name": "Mad/Rest" }, "upcoming_dates": [{ "date": "2030-10-19" }, { "date": "2030-10-22" }] }]);
    assert_eq!(api::parse_openexp_live(&live)[1], Pickup { date: d("2030-10-22"), fraction: "Mad/Rest".into() });
}

#[test]
fn odense_calendar() {
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Europe/Copenhagen:20301103T070000\r\n\
               SUMMARY:Tømning af Restaffald\\, Papir & småt pap på Testvej\r\n 1\r\nEND:VEVENT\r\n\
               BEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20301110\r\nSUMMARY:Tømning af Glas & Metal på Testvej 1\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let p = api::parse_ics(ics);
    assert_eq!(
        p,
        vec![
            Pickup { date: d("2030-11-03"), fraction: "Restaffald".into() },
            Pickup { date: d("2030-11-03"), fraction: "Papir & småt pap".into() },
            Pickup { date: d("2030-11-10"), fraction: "Glas & Metal".into() },
        ]
    );
}

#[test]
fn fractions_group_sort_and_drop_the_past() {
    let p = |date: &str, f: &str| Pickup { date: d(date), fraction: f.into() };
    let pickups = vec![
        p("2030-01-01", "Restaffald 240 L"),
        p("2030-01-20", "Restaffald 240 L"),
        p("2030-01-06", "Restaffald 140 L"),
        p("2030-01-06", "Restaffald 140 L"),
        p("2030-01-10", "Pap"),
    ];
    let f = schedule::fractions(&pickups, d("2030-01-05"));
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].name, "Restaffald");
    assert_eq!(f[0].dates, vec![d("2030-01-06"), d("2030-01-20")]);
    assert_eq!(f[0].descriptions, vec!["Restaffald 240 L", "Restaffald 140 L"]);
    assert_eq!(schedule::next(&f), Some((d("2030-01-06"), vec!["Restaffald".to_string()])));
}

async fn renoweb(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1_13/GetJSONRoad.aspx"))
        .and(query_param("municipalitycode", "851"))
        .and(query_param("roadname", "Testvej"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "list": [
            { "id": 11, "name": "Testvej (9000)" }, { "id": 12, "name": "Testvej (9400)" }, { "id": 13, "name": "Testvejen (9000)" }
        ]})))
        .mount(server)
        .await;
    for (road, zip) in [("11", "9000 Aalborg"), ("12", "9400 Nørresundby")] {
        Mock::given(method("GET"))
            .and(path("/v1_13/GetJSONAdress.aspx"))
            .and(query_param("roadid", road))
            .and(query_param("streetBuildingIdentifier", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "list": [
                { "id": format!("{road}01"), "presentationString": format!("Testvej 1, {zip}") },
                { "id": format!("{road}02"), "presentationString": format!("Testvej 1A, {zip}") },
            ]})))
            .mount(server)
            .await;
    }
    // Latin-1, as RenoWeb sometimes sends
    let mut body = br#"{"list":[{"name":"Restaffald/Madaffald 240 L","nextpickupdatetimestamp":"1919980800","pickupdates":"Ugent"#.to_vec();
    body.push(0xF8);
    body.extend_from_slice(br#"mning"},{"name":"Papir 140 L","nextpickupdatetimestamp":"1920585600"},{"name":"Bestillerordning","nextpickupdatetimestamp":""}]}"#);
    Mock::given(method("GET"))
        .and(path("/v1_13/GetJSONContainerList.aspx"))
        .and(query_param("adressId", "1101"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(server)
        .await;
}

fn flow(data: Value) -> Value {
    json!({ "flow_id": "f1", "source": "user", "data": data })
}

#[tokio::test(flavor = "multi_thread")]
async fn renoweb_flow_and_sensors() {
    let server = MockServer::start().await;
    renoweb(&server).await;
    let h = host::FakeHost::start(Affald::default());
    let base = json!({ "base_url": server.uri() });

    let r = h.call("config_flow", json!({ "flow": flow(base.clone()), "step": "user", "input": null })).await.unwrap();
    assert_eq!(r["result"]["type"], "form");
    assert!(r["result"]["schema"]["properties"]["kommune"]["enum"].as_array().unwrap().contains(&json!("Aalborg")));

    // the street in two districts, house number 1 and 1A in each
    let input = json!({ "kommune": "Aalborg", "address": "Testvej 1", "zip": "" });
    let r = h.call("config_flow", json!({ "flow": flow(base.clone()), "step": "user", "input": input })).await.unwrap();
    assert_eq!(r["result"]["step_id"], "pick");
    let options = r["result"]["schema"]["properties"]["address_id"]["oneOf"].as_array().unwrap().clone();
    assert_eq!(options.len(), 4);
    assert_eq!(options[0], json!({ "const": "1101", "title": "Testvej 1, 9000 Aalborg" }));

    // a postal code and a letter narrow it to one, which ends the flow
    let input = json!({ "kommune": "Aalborg", "address": "Testvej 1 a", "zip": "9400" });
    let r = h.call("config_flow", json!({ "flow": flow(base.clone()), "step": "user", "input": input })).await.unwrap();
    assert_eq!(r["result"]["type"], "create_entry");
    assert_eq!(r["result"]["data"]["address_id"], "1202");
    assert_eq!(r["unique_id"], "851-1202");

    // the pick step
    let data = pick_data(&options, &base);
    let r = h.call("config_flow", json!({ "flow": flow(data), "step": "pick", "input": { "address_id": "1101" } })).await.unwrap();
    assert_eq!(r["result"]["type"], "create_entry");
    assert_eq!(r["result"]["title"], "Testvej 1, 9000 Aalborg");
    let entry_data = r["result"]["data"].clone();
    assert!(entry_data.get("_found").is_none());

    h.call("setup_entry", json!({ "entry": { "entry_id": "e1", "domain": "affald", "data": entry_data } })).await.unwrap();
    let rest = h.entity_ending("restaffald_madaffald");
    let (state, attrs) = h.state(&rest).unwrap();
    assert_eq!(state, "2030-11-04");
    assert_eq!(attrs["dates"], json!(["2030-11-04"]));
    assert_eq!(attrs["description"], "Restaffald/Madaffald 240 L");
    let (spec, _) = h.rec.lock().unwrap().entities[&rest].clone();
    assert_eq!(spec["device_class"], "date");
    assert_eq!(spec["unique_id"], "851-1101-restaffald_madaffald");
    assert_eq!(h.state(&h.entity_ending("papir")).unwrap().0, "2030-11-11");
    let (next, attrs) = h.state(&h.entity_ending("t_mning")).unwrap();
    assert_eq!(next, "2030-11-04");
    assert_eq!(attrs["fractions"], json!(["Restaffald/Madaffald"]));
    // no date, no sensor
    assert_eq!(h.rec.lock().unwrap().entities.len(), 3);
    let devices = h.rec.lock().unwrap().devices.clone();
    assert_eq!(devices[0]["name"], "Affald Testvej 1, 9000 Aalborg");
    assert_eq!(devices[0]["manufacturer"], "RenoWeb");
}

/// The flow data the pick step gets: what the first step stored.
fn pick_data(options: &[Value], base: &Value) -> Value {
    let mut data = base.clone();
    data["kommune"] = json!("Aalborg");
    data["_found"] = options.iter().map(|o| json!({ "id": o["const"], "name": o["title"] })).collect();
    data
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_address_and_dead_server() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/searchExternalAddresses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": [] })))
        .mount(&server)
        .await;
    let h = host::FakeHost::start(Affald::default());
    let input = json!({ "kommune": "Roskilde", "address": "Ingenvej 9" });
    let r = h.call("config_flow", json!({ "flow": flow(json!({ "base_url": server.uri() })), "step": "user", "input": input.clone() })).await.unwrap();
    assert_eq!(r["result"]["errors"]["address"], "invalid_value");
    assert!(r["result"]["description"].as_str().unwrap().contains("Roskilde Kommune"));
    // the form keeps what was typed
    assert_eq!(r["result"]["schema"]["properties"]["address"]["default"], "Ingenvej 9");

    let r = h.call("config_flow", json!({ "flow": flow(json!({ "base_url": "http://127.0.0.1:9" })), "step": "user", "input": input })).await.unwrap();
    assert_eq!(r["result"]["errors"]["base"], "cannot_connect");
}

#[tokio::test(flavor = "multi_thread")]
async fn perfect_waste_looks_up_an_address_it_does_not_know() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fetchAddressAndSaveToDb"))
        .and(body_partial_json(json!({ "data": { "addressID": "77", "municipality": 265 } })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": {} })))
        .expect(1)
        .mount(&server)
        .await;
    // nothing the first time, as for an address its app has not seen
    Mock::given(method("POST"))
        .and(path("/getAddressCollections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": [] })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/getAddressCollections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": [
            { "date": "2030-10-28T00:00:00.000Z", "fractions": [{ "fractionName": "Restaffald-Madaffald" }, { "fractionName": "PAPPI" }] },
            { "date": "2030-11-11T00:00:00.000Z", "fractions": [{ "fractionName": "Restaffald-Madaffald" }] },
        ]})))
        .mount(&server)
        .await;
    let h = host::FakeHost::start(Affald::default());
    let data = json!({ "base_url": server.uri(), "kommune": "Roskilde", "address_id": "77", "address": "Testvej 1, 4000 Roskilde" });
    h.call("setup_entry", json!({ "entry": { "entry_id": "e1", "domain": "affald", "data": data } })).await.unwrap();
    let (state, attrs) = h.state(&h.entity_ending("restaffald_madaffald")).unwrap();
    assert_eq!(state, "2030-10-28");
    assert_eq!(attrs["dates"], json!(["2030-10-28", "2030-11-11"]));
    let (_, attrs) = h.state(&h.entity_ending("t_mning")).unwrap();
    assert_eq!(attrs["fractions"], json!(["Pappi", "Restaffald-Madaffald"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn perfect_waste_leaves_a_known_address_alone() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/fetchAddressAndSaveToDb"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": {} })))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/getAddressCollections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": [
            { "date": "2030-10-28T00:00:00.000Z", "fractions": [{ "fractionName": "Rest" }] },
        ]})))
        .mount(&server)
        .await;
    let k = homewright_affald::kommuner::find("Albertslund").unwrap();
    let p = api::Client::new(Some(&server.uri())).pickups(k, "77").await.unwrap();
    assert_eq!(p, vec![Pickup { date: d("2030-10-28"), fraction: "Rest".into() }]);
}
