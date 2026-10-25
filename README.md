# Affaldskalender for HomewrightHQ

An external integration for [HomewrightHQ](https://github.com/magnusdunker/homewrighthq) that fetches the waste collection days from the Danish municipalities.

## Installation

In HomewrightHQ: **Settings → Integrations → From GitHub**. Paste

```
https://github.com/magnusdunker/homewrighthq-affald
```

and press **Install**. Then add **Affaldskalender** under **Add integration**. Choose the municipality and enter the street name and house number. If several addresses match, you pick yours from a list; a postcode or the house number's letter makes the list shorter. Each address is its own entry, so the summer house can be added alongside.

## What you get

The address becomes a device, "Affald Skolevej 5, ...", with a date sensor for each kind of waste the municipality collects there:

- the state is the next collection day, e.g. `2026-10-28`
- `days`: days until then, `0` on the day itself
- `dates`: the next up to 12 collection days
- `description`: the municipality's own names for what the sensor covers

The sensor **Næste tømning** (next collection) shows the next day something is collected, and `fractions` lists what is collected.

The names are the municipality's own, without container sizes and postal district: "240 L Dagrenovation" becomes "Dagrenovation". Two containers with the same kind of waste become one sensor. If a new kind shows up in the calendar, it gets a sensor straight away.

The calendar is fetched four times a day, and the sensors move on at midnight. Some systems only give the next collection for each container (RenoWeb, Reno Djurs and Affald KBH), so there `dates` has only one date.

## Municipalities

95 of the country's 98 municipalities, grouped by the system they use:

| System | Municipalities |
|---|---|
| Perfect Waste | Albertslund, Allerød, Ballerup, Faxe, Frederiksberg, Frederikssund, Gladsaxe, Greve, Gribskov, Guldborgsund, Halsnæs, Helsingør, Hillerød, Horsens, Hvidovre, Høje-Taastrup, Hørsholm, Kalundborg, Køge, Lejre, Lolland, Lyngby-Taarbæk, Næstved, Odsherred, Ringkøbing-Skjern, Roskilde, Rudersdal, Slagelse, Solrød, Stevns, Tårnby, Vejen |
| RenoWeb | Aabenraa, Aalborg, Billund, Bornholm, Brøndby, Brønderslev, Dragør, Egedal, Esbjerg, Fredensborg, Gentofte, Glostrup, Hjørring, Jammerbugt, Kerteminde, Mariagerfjord, Randers, Rødovre, Samsø, Svendborg, Sønderborg, Varde, Vordingborg |
| AffaldOnline | Assens, Faaborg-Midtfyn, Favrskov, Holbæk, Langeland, Morsø, Rebild, Vejle, Viborg, Ærø |
| AffaldOnline, collection calendar as PDF | Fanø, Middelfart, Nyborg, Sorø |
| AffaldOnline, dates only | Silkeborg |
| OpenExperience | Fredericia, Holstebro, Lemvig, Nordfyns, Skive, Struer, Thisted |
| Vestforbrænding | Furesø, Herlev, Ishøj, Vallensbæk |
| Renomatic | Frederikshavn, Ikast-Brande, Vesthimmerlands |
| RenoSyd | Odder, Skanderborg |
| Reno Djurs | Norddjurs, Syddjurs |
| Kredsløb | Aarhus |
| Affald KBH | København |
| Infovision | Kolding |
| Affaldsweb | Herning |
| Provas | Haderslev |
| WasteWatch | Tønder |
| Odense Renovation | Odense |

Silkeborg's calendar only shows the days, not what is collected, so there it becomes one sensor, "Tømning". Herning gives the weekday and weeks for each container, and the dates are worked out from them; moves because of public holidays are not included.

In Kolding the containers belong to the building's address. If you choose a flat, the integration fetches the dates from the building's address. If the flat is not linked to any container in Kolding's system, it gets no sensors; in that case choose the building's address without a floor.

Three municipalities are missing:

- Ringsted and Hedensted: the collection calendar is only in RenoWeb's self-service, which requires login with MitID.
- Læsø: Læsø Forsyning has no collection calendar online.

Not tested:

- Kolding: containers the address has without a fee (`chainDetailsZero`) are read like the others, but no address with one has been found.
- Kolding: a flat that shares containers with more than three addresses only gets the dates from the first three.

Vestforbrænding's server only understands older encryption methods, which rustls does not have. The integration therefore uses the system's own TLS for it, and OpenSSL is built into the Linux version.

Which municipality uses which system, and how the systems respond, comes from [AffaldDK](https://github.com/briis/affalddk) for Home Assistant (MIT). The integration uses the same open addresses and keys as the municipalities' own apps and websites and does not log in anywhere. For the address search in Aarhus, Odder, Skanderborg and Tønder it uses Klimadatastyrelsen's Adressevælger with its public demo key.

## Development

Built in Rust on the `thehome-plugin` SDK from [homewright-example-integration-rust](https://github.com/magnusdunker/homewrighthq-example-integration-rust).

- `src/api.rs`: the systems many municipalities share, and the lookup in them
- `src/sources.rs`: the systems with one or a few municipalities each
- `src/pdf.rs`: the collection calendar as PDF from affaldonline.dk
- `src/kommuner.rs`: the municipalities and their system
- `src/schedule.rs`: from collections to one sensor per kind of waste
- `src/lib.rs`: the connection to HomewrightHQ
- `tests/affald.rs` and `tests/sources.rs`: tests against fake servers, and a fake HomewrightHQ in `tests/host`
- `tests/live.rs`: looks up town halls and other addresses in the real systems, `cargo test --test live -- --ignored --nocapture`

```
cargo test
cargo build --release
```

A new version is released with a tag, e.g. `git tag v0.2.0 && git push origin v0.2.0`. The workflow builds for Windows, Linux (x86_64 and arm64, static musl) and macOS and puts the binaries and `homewright.json` on the release.

## License

MIT
