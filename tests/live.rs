//! Against the real systems, with town halls and other public buildings.
//! Not run by default: `cargo test --test live -- --ignored --nocapture`

use homewright_affald::api::Client;
use homewright_affald::kommuner;
use homewright_affald::schedule;

#[tokio::test]
#[ignore]
async fn town_halls() {
    let c = Client::default();
    let today = chrono::Utc::now().with_timezone(&homewright_affald::api::TZ).date_naive();
    let cases = [
        ("Aalborg", "Boulevarden 13", "9000"),
        ("Roskilde", "Køgevej 80", ""),
        ("Vejle", "Skolegade 1", "7100"),
        ("Holstebro", "Kirkestræde 11", "7500"),
        ("Fredericia", "Gothersgade 20", ""),
        ("Odense", "Flakhaven 2", ""),
    ];
    for (name, query, zip) in cases {
        let k = kommuner::find(name).unwrap();
        let found = c.search(k, query, Some(zip)).await;
        println!("{name} ({}): {query}", k.provider.label());
        let found = match found {
            Ok(f) => f,
            Err(e) => {
                println!("  fejl: {e}");
                continue;
            }
        };
        println!("  {} adresser, f.eks. {:?}", found.len(), found.first());
        let Some(a) = found.first() else { continue };
        c.prepare(k, &a.id).await.unwrap();
        let p = c.pickups(k, &a.id).await.unwrap();
        for f in schedule::fractions(&p, today) {
            println!("  {:<40} {} ({} datoer) {:?}", f.name, f.dates[0], f.dates.len(), f.descriptions);
        }
    }
}
