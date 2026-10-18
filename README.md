# Affaldskalender til Homewright

En ekstern integration til [Homewright](https://github.com/magnusdunker/homewright), der henter tømmedagene for affald fra kommunerne.

## Installation

I Homewright: **Indstillinger → Integrationer → Fra GitHub**. Indsæt

```
https://github.com/magnusdunker/homewright-affald
```

og tryk **Installér**. Tilføj derefter **Affaldskalender** under **Tilføj integration**. Vælg kommunen og skriv vejnavn og husnummer. Passer flere adresser, vælger du din på en liste; et postnummer eller husnummerets bogstav gør listen kortere. Hver adresse er sin egen opsætning, så sommerhuset kan komme med ved siden af.

## Hvad du får

Adressen bliver en enhed, "Affald Skolevej 5, ...", med en datosensor for hver slags affald, kommunen henter der:

- tilstanden er næste tømmedag, f.eks. `2026-10-28`
- `days`: dage til da, `0` på selve dagen
- `dates`: de næste op til 12 tømmedage
- `description`: kommunens egne navne for det, sensoren dækker

Sensoren **Næste tømning** viser den næste dag, hvor der bliver hentet noget, og i `fractions` hvad der hentes.

Navnene er kommunens egne, uden beholderstørrelser og postdistrikt: "240 L Dagrenovation" bliver til "Dagrenovation". To beholdere med samme slags affald bliver til én sensor. Dukker en ny slags op i kalenderen, får den en sensor med det samme.

Kalenderen hentes fire gange i døgnet, og sensorerne rykker videre ved midnat. RenoWeb oplyser kun den næste tømning for hver beholder, så i de kommuner har `dates` kun én dato.

## Kommuner

72 af landets 98 kommuner, fordelt på de systemer, de bruger:

| System | Kommuner |
|---|---|
| Perfect Waste | Allerød, Ballerup, Faxe, Frederiksberg, Frederikssund, Gladsaxe, Greve, Gribskov, Guldborgsund, Halsnæs, Helsingør, Hillerød, Horsens, Hvidovre, Høje-Taastrup, Hørsholm, Kalundborg, Køge, Lejre, Lolland, Lyngby-Taarbæk, Næstved, Odsherred, Ringkøbing-Skjern, Roskilde, Rudersdal, Slagelse, Solrød, Stevns, Tårnby, Vejen |
| RenoWeb | Aabenraa, Aalborg, Billund, Bornholm, Brøndby, Brønderslev, Dragør, Egedal, Esbjerg, Fredensborg, Gentofte, Glostrup, Hjørring, Jammerbugt, Kerteminde, Mariagerfjord, Randers, Rødovre, Samsø, Svendborg, Sønderborg, Varde, Vordingborg |
| AffaldOnline | Assens, Faaborg-Midtfyn, Favrskov, Holbæk, Langeland, Morsø, Rebild, Vejle, Viborg, Ærø |
| OpenExperience | Fredericia, Holstebro, Lemvig, Nordfyns, Skive, Struer, Thisted |
| Odense Renovation | Odense |

Blandt dem, der mangler, er København, Aarhus, Kolding, Herning, Silkeborg og Vestforbrændings kommuner (Albertslund, Furesø, Herlev, Ishøj og Vallensbæk). De har hver deres system.

Hvilken kommune der bruger hvilket system, og hvordan systemerne svarer, kommer fra [AffaldDK](https://github.com/briis/affalddk) til Home Assistant (MIT). Integrationen bruger de samme åbne adresser og nøgler som kommunernes egne apps og hjemmesider og logger ikke ind nogen steder.

## Udvikling

Bygget i Rust på SDK'en `thehome-plugin` fra [homewright-example-integration-rust](https://github.com/magnusdunker/homewright-example-integration-rust).

- `src/api.rs`: systemerne, adressesøgning og tømmedage
- `src/kommuner.rs`: kommunerne og deres system
- `src/schedule.rs`: fra tømninger til én sensor pr. slags affald
- `src/lib.rs`: forbindelsen til Homewright
- `tests/affald.rs`: tests mod falske servere, og en falsk Homewright i `tests/host`
- `tests/live.rs`: slår rådhuse op i de rigtige systemer, `cargo test --test live -- --ignored --nocapture`

```
cargo test
cargo build --release
```

En ny version udgives med et tag, f.eks. `git tag v0.2.0 && git push origin v0.2.0`. Workflowet bygger til Windows, Linux (x86_64 og arm64, statisk musl) og macOS og lægger binærerne og `homewright.json` på releasen.

## Licens

MIT
