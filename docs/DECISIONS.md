# Registro delle decisioni

Formato: data · contesto · decisione · alternative scartate. Le voci marcate **da confermare**
sono valori predefiniti di questioni aperte (PROGETTO.md §17), configurabili nel pacchetto.

## 2026-11-01 — Fasi F0 e F1

### Toolchain e librerie
- **Rust stable, edition 2024**, resolver 3. `rust-toolchain.toml` segue `stable` (non una
  versione fissa) per non scaricare toolchain extra; CI usa `dtolnay/rust-toolchain@stable`.
- **YAML con `serde_norway`** (fork mantenuto di `serde_yaml`, deprecato). Ancore e chiavi di
  merge (`<<`) espanse prima della validazione; le chiavi di primo livello `x-*` sono scartate.
- **JSON Schema con `schemars` 1.x, validazione con `jsonschema`** (senza feature di rete). Lo
  schema è generato a runtime dai tipi di `core-model` (quindi sempre allineato);
  `schemas/package.schema.json` è la copia committata, controllata in CI con `xtask schema --check`.
- **Ordine dei controlli**: prima la deserializzazione tipizzata (`serde_path_to_error`, messaggi
  più precisi: "unknown field `priorty`" con percorso), poi lo schema, poi la validazione
  semantica. Gli errori di schema compaiono solo se serde accetta il valore (oggi coincidono).
- Messaggi di errore del nucleo in **inglese** (il nucleo è neutro); traduzione con la UI in F5.

### Formato binario
- Il bundle compilato è `postcard`, che non è auto-descrittivo. Regola per tutti i tipi di
  `core-model`: niente enum `untagged`/internally tagged, niente `flatten`, niente
  `skip_serializing_if`. Le forme abbreviate YAML (`event_type: x` o `[x, y]`, opzioni `m` o
  `{id, label}`, valori dinamici `Scalar`, `Id` in base32) hanno una doppia rappresentazione
  scelta con `is_human_readable()`.
- **`CompiledConfig` mantiene gli slug** come chiavi invece di indici numerici: il bundle è
  piccolo (~20 KB per Congregazione), gli slug restano stabili tra versioni e migrazioni.
  Le tabelle derivate (`ConfigIndex`: chiusura delle implicazioni, membri degli insiemi,
  pubblici, incarichi) sono precalcolate. Scartato: indici interni (fragili con le migrazioni).
- `CORE_VERSION = "1.0.0"` è la versione del modello di configurazione, separata dalle versioni
  dei crate; `core_compat` dei pacchetti si confronta con essa.

### Modello
- **Periodi semiaperti `[inizio, fine)`**: una qualifica di ottobre è `2026-10-01..2026-11-01`.
- **`except: [base, tolti...]`** = base meno l'unione degli altri (la specifica non ne fissava la forma).
- **`supervisors_of`** include i titolari di un incarico con `supervises: X` nell'unità solo
  mentre X è effettivamente coperto in quell'unità.
- `holds_role` con `where: subject_unit/own_unit/unit:<id>` non corrisponde mai a conferimenti
  di organizzazione.
- **Pubblici di sistema**: `subject_only`, `subject_or_on_behalf`, `subject_missing` (dalla
  specifica) più `assignee`, `resource_holder`, `object_team` (responsabile + assistenti),
  `root_admins`, necessari per la tabella delle notifiche §13.12. Non ridefinibili.
- **Etichette predefinite** quando `label` manca: campi `fld.<id>`, opzioni `opt.<id>`, misure
  `msr.<id>`, passaggi `step.<id>`. La validazione esige il testo anche per le chiavi predefinite.
- Trigger aggiunto **`document.opened`** (serve per "apertura del rapporto" in §13.12).
- Promemoria: tipo `ReminderRuleDef { kind, days, filter }`; aggiunto `batch.missing_after`
  (presente nella tabella §13.12, assente dall'elenco §5.11).
- `retention` (`{ years | months | days: n }` o `{ forever: true }`) su moduli, aggregazioni,
  tipi di programma e di risorsa (§14).
- **Soddisfacibilità statica** delle condizioni (regola "incarico che richiede qualifiche mai
  ottenibili"): falsa se compaiono `always: false`, qualifiche mutuamente esclusive (anche
  tramite implicazioni e requisiti delle qualifiche) o uguaglianze in conflitto sullo stesso
  attributo. Ogni ramo di `any` è valutato a sé. È un'approssimazione prudente: può non vedere
  combinazioni impossibili più sottili, non segnala mai come impossibile ciò che è possibile.
- `unit:<id>` nei selettori si riferisce a un'istanza di unità (id base32); la validazione
  non può verificarlo perché le istanze nascono a runtime.

### Privacy (da §7.1)
- I dispositivi conoscono la mappa qualifiche/incarichi di tutti (serve a verificare i
  permessi); l'interfaccia la mostra solo al pubblico `visibility` di ciascuna qualifica.
  Compromesso accettato, da riportare nell'informativa.

### Pacchetto Congregazione — scelte aggiunte
- Pubblico `group_leaders` (chi sorveglia un qualsiasi gruppo, senza soggetto): serve per la
  visibilità dei menu, dove `group_oversight` (relativo al gruppo del soggetto) è vuoto.
- Pubblico `app_admins` per le notifiche amministrative.
- `board_manager` riceve anche `program.publish` (pubblica i programmi in bacheca, §13.10).
- `lmm_overseer` e `public_meeting_coordinator` ricevono `assignment.respond_on_behalf` sul
  proprio programma (conferma degli oratori esterni, §13.9).
- I promemoria sui rapporti (5/8/15 giorni) stanno solo nei timer del flusso, non anche in
  `notifications.yaml`, per non generarli due volte.
- Colore primario `#2952CC` e menu presi dal mockup di riferimento (canvas "Mockup app
  Congregazione"); l'icona in `branding/icon.png` è un segnaposto.
- `privacy/it.md` è una bozza con parti da compilare a cura del titolare.

### Ambiente di sviluppo locale
- macOS con SDK 27 e linker `ld-1267`: il link fallisce (`unknown architecture arm64e.x1-macos`).
  Soluzione: `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk`. Non è nel
  repository perché dipende dalla macchina.

### Da confermare (valori predefiniti, §17)
- Accettazione attiva per tutti i programmi.
- Bacheca leggibile da tutti i proclamatori (`board_public` = `congregation`).
- Responsabile della bacheca e organizzatore del servizio di campo: anziano o servitore, più titolari.
- Struttura di Vita e ministero come §13.9; presidente anziano.
- Rapporti: invio entro il 5, inoltro con mancanti dall'8, promemoria al segretario il 15.
- Ore extra: durata libera, solo `min: 0`. Ore: massimo 31000 minuti.
- Gruppo del rapporto = gruppo alla fine del periodo (si applica nel motore, F3).
- Visibilità delle qualifiche come §17; contatti leggibili da anziani, sorveglianza del gruppo e persona.
- Conservazione: rapporti e resoconti 5 anni, programmi 2 anni, territori senza scadenza.
- Territori: massimo 3 per detentore, promemoria dopo 120 giorni senza registrazione, avviso dopo 365.
- Preavviso di fine servizio di pioniere ausiliare: 7 giorni.
- Almeno 2 amministratori dell'app (anziani); giorni e orari delle adunanze come §13.8.
- Indicazioni della congregazione sugli strumenti digitali: da verificare prima del pilota.
