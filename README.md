# Affaldskalender til HomewrightHQ

En ekstern integration til [HomewrightHQ](https://github.com/magnusdunker/homewrighthq), der henter tømmedagene for affald fra kommunerne.

## Installation

I HomewrightHQ: **Indstillinger → Integrationer → Fra GitHub**. Indsæt

```
https://github.com/magnusdunker/homewrighthq-affald
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

Kalenderen hentes fire gange i døgnet, og sensorerne rykker videre ved midnat. Nogle systemer oplyser kun den næste tømning for hver beholder (RenoWeb, Reno Djurs og Affald KBH), så der har `dates` kun én dato.

## Kommuner

95 af landets 98 kommuner, fordelt på de systemer, de bruger:

| System | Kommuner |
|---|---|
| Perfect Waste | Albertslund, Allerød, Ballerup, Faxe, Frederiksberg, Frederikssund, Gladsaxe, Greve, Gribskov, Guldborgsund, Halsnæs, Helsingør, Hillerød, Horsens, Hvidovre, Høje-Taastrup, Hørsholm, Kalundborg, Køge, Lejre, Lolland, Lyngby-Taarbæk, Næstved, Odsherred, Ringkøbing-Skjern, Roskilde, Rudersdal, Slagelse, Solrød, Stevns, Tårnby, Vejen |
| RenoWeb | Aabenraa, Aalborg, Billund, Bornholm, Brøndby, Brønderslev, Dragør, Egedal, Esbjerg, Fredensborg, Gentofte, Glostrup, Hjørring, Jammerbugt, Kerteminde, Mariagerfjord, Randers, Rødovre, Samsø, Svendborg, Sønderborg, Varde, Vordingborg |
| AffaldOnline | Assens, Faaborg-Midtfyn, Favrskov, Holbæk, Langeland, Morsø, Rebild, Vejle, Viborg, Ærø |
| AffaldOnline, tømningskalenderen som PDF | Fanø, Middelfart, Nyborg, Sorø |
| AffaldOnline, kun datoerne | Silkeborg |
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

Silkeborgs kalender viser kun dagene, ikke hvad der hentes, så der bliver det én sensor, "Tømning". Herning oplyser ugedag og uger for hver beholder, og datoerne regnes ud fra dem; flytninger på grund af helligdage kommer ikke med.

I Kolding hører beholderne til bygningens adresse. Vælger du en lejlighed, henter integrationen datoerne fra bygningens adresse. Er lejligheden ikke knyttet til nogen beholder i Koldings system, får den ingen sensorer; vælg så bygningens adresse uden etage.

Tre kommuner mangler:

- Ringsted og Hedensted: tømmekalenderen findes kun i RenoWebs selvbetjening, der kræver login med MitID.
- Læsø: Læsø Forsyning har ingen tømmekalender på nettet.

Ikke afprøvet:

- Kolding: beholdere, som adressen har uden gebyr (`chainDetailsZero`), læses som de andre, men der er ikke fundet en adresse med sådan en.
- Kolding: en lejlighed, der deler beholdere med mere end tre adresser, får kun datoerne fra de tre første.

Vestforbrændings server forstår kun ældre krypteringsmetoder, som rustls ikke har. Integrationen bruger derfor systemets egen TLS til den, og OpenSSL er bygget ind i Linux-udgaven.

Hvilken kommune der bruger hvilket system, og hvordan systemerne svarer, kommer fra [AffaldDK](https://github.com/briis/affalddk) til Home Assistant (MIT). Integrationen bruger de samme åbne adresser og nøgler som kommunernes egne apps og hjemmesider og logger ikke ind nogen steder. Til adressesøgningen i Aarhus, Odder, Skanderborg og Tønder bruger den Klimadatastyrelsens Adressevælger med dens offentlige demonøgle.

## Udvikling

Bygget i Rust på SDK'en `thehome-plugin` fra [homewright-example-integration-rust](https://github.com/magnusdunker/homewrighthq-example-integration-rust).

- `src/api.rs`: de systemer, mange kommuner deler, og opslaget i dem
- `src/sources.rs`: systemerne med én eller få kommuner hver
- `src/pdf.rs`: tømningskalenderen som PDF fra affaldonline.dk
- `src/kommuner.rs`: kommunerne og deres system
- `src/schedule.rs`: fra tømninger til én sensor pr. slags affald
- `src/lib.rs`: forbindelsen til HomewrightHQ
- `tests/affald.rs` og `tests/sources.rs`: tests mod falske servere, og en falsk HomewrightHQ i `tests/host`
- `tests/live.rs`: slår rådhuse og andre adresser op i de rigtige systemer, `cargo test --test live -- --ignored --nocapture`

```
cargo test
cargo build --release
```

En ny version udgives med et tag, f.eks. `git tag v0.2.0 && git push origin v0.2.0`. Workflowet bygger til Windows, Linux (x86_64 og arm64, statisk musl) og macOS og lægger binærerne og `homewright.json` på releasen.

## Licens

MIT
