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

## 2026-11-01 — Fase F2 (eventi e crittografia)

### Librerie
- **Firme e chiavi**: `ed25519-dalek` 3 e `x25519-dalek` 3 (stessa generazione usata da iroh 1.x,
  così la chiave Ed25519 del dispositivo diventa direttamente la `SecretKey` iroh in F4).
- **Sealed box**: `crypto_box` 0.9 (ultima stabile; la 0.10 è pre-release). Per restare su una
  sola generazione `aead` 0.5 usiamo `chacha20poly1305` 0.10, `hkdf` 0.12 e `sha2` 0.10 invece
  delle 0.11/0.13 appena uscite. Da riallineare quando `crypto_box` 0.10 sarà stabile.
- **SQLite**: `rusqlite` 0.41 con `bundled-sqlcipher-vendored-openssl` (OpenSSL compilato dal
  sorgente: nessuna dipendenza di sistema, funziona anche per Android/iOS; prima build ~30 s).
- Le chiavi sono costruite da byte casuali di `getrandom` invece che da un `RngCore`, per non
  dipendere dalle versioni di `rand_core` delle singole librerie.

### Cifratura
- AAD esattamente come §8.2: `org (32) || channel id (32) || epoch (u32 LE) || nome del kind`.
- La chiave sigillata verso un dispositivo contiene `channel id || epoch || chiave`: una chiave
  distribuita per un canale non può essere spacciata per quella di un altro canale o epoch.
- Rotazioni concorrenti: si conservano tutte le chiavi della stessa epoch (ogni payload resta
  leggibile) e per i nuovi payload si usa quella introdotta dall'evento con id minore.
- Chiavi dei canali nel database cifrate di nuovo con una chiave locale derivata (HKDF-SHA256,
  info `org-app/local-wrap/1`) dalla chiave del database, legata a canale, epoch ed evento.
- Vettori di test: RFC 8032 (Ed25519), RFC 7748 (X25519), draft XChaCha20-Poly1305 A.3.1, più
  vettori di regressione nostri per id dei canali e formato AAD (cambiarli renderebbe illeggibili
  i canali esistenti).

### Eventi
- **Tipi in chiaro** (`EventKind::is_public`): `org.*`, `config.published`, `device.*`,
  `channel.*`, `person.data_erasure_requested`, `retention.applied`, `admin.quarantine_resolved`.
  Tutti gli altri devono essere cifrati per un canale: la regola è verificata in `verify()`.
  Gli eventi organizzativi (qualifiche, incarichi, appartenenze) andranno in un canale di tutta
  l'organizzazione (F3): verificabili da ogni dispositivo membro, illeggibili per chi è fuori.
- **Intestazione in chiaro** del payload: tipo di oggetto, id dell'oggetto e tipo configurato
  (es. quale modulo). Mai valori dei campi.
- La genesi ha `org` = 32 byte a zero (il suo id non è ancora noto); `OrgId` = id della genesi.
  Non contiene nomi di persone, solo id e chiavi dei dispositivi degli amministratori radice;
  un `nonce` casuale rende unico ogni `OrgId`.
- **Ordine deterministico**: `(depth, id)`. Poiché `depth = 1 + max(depth dei genitori)`,
  quest'ordine è già topologico; non serve un ordinamento topologico separato.
- postcard codifica le varianti per posizione: i nuovi `EventKind` si aggiungono solo in fondo
  (un test blocca le posizioni).

### Archivio
- Stati a livello di archivio: `pending_parents` finché manca un antenato, poi `valid`
  (strutturale) o `invalid` (profondità dichiarata errata). `pending_key`, `invalid` per
  permessi e `quarantined` li assegna il motore (F3). Eventi con id o firma errati non vengono
  mai salvati.
- `ancestry_index` non implementato: il passato causale si calcola con una CTE ricorsiva,
  sufficiente ai volumi previsti (§1.3). Da rivedere con le misure del simulatore (F4).
- Un archivio è legato a una sola organizzazione: dalla genesi, oppure con `bind_org` quando un
  nuovo dispositivo conosce l'`OrgId` dal ticket di invito prima di ricevere la genesi.
- `SecretStore`: per ora solo l'implementazione in memoria; desktop (`keyring`) in F5, mobile in F7.

## 2026-11-01 — Fase F3 (motore)

### Ricostruzione
- **`rebuild(eventi, portachiavi)` è una funzione pura**: stesso input → proiezione identica byte
  per byte (`state_hash`). Nessuna data viene dall'orologio locale: si usano la data dichiarata
  degli eventi e le date dei contenuti. Ricalcolo completo a ogni modifica (§7.5 lo ammette per
  i volumi della v1); il ricalcolo incrementale si valuterà con le misure del simulatore (F4).
- Tre fasi: (1) eventi organizzativi, ciascuno verificato sullo stato dei soli eventi
  organizzativi del suo passato causale (bitset di antenati, cache per insieme); (2) pre-passo
  dei conflitti; (3) tutti gli altri eventi in ordine `(depth, id)`, verificati sullo stato di
  autorizzazione del loro passato causale e sullo stato attuale degli oggetti.
- **Quarantena** decisa prima della decifratura (dipende solo da autore, tempo e passato causale),
  così tutti i dispositivi concordano. Limite noto: non si applica agli eventi organizzativi,
  che restano validati sul loro passato causale.
- **Configurazione attiva**: l'ultima `config.published` che ha tutte le altre nel passato
  causale; con versioni concorrenti resta attiva l'ultima comune e si genera un avviso.

### Conflitti (§7.6)
- Passaggi concorrenti sullo stesso documento, passi concorrenti dello stesso lotto, documenti
  periodici duplicati: vince la priorità di incarico più alta dell'autore, poi l'id minore; chi è
  concorrente con un vincitore diventa `superseded`. La regola vale per "eventi concorrenti sullo
  stesso oggetto", che nella pratica partono dallo stesso stato.
- Conferimenti concorrenti oltre `holders.max`: si tengono gli id evento minori.

### Permessi
- I passaggi di flusso sono autorizzati dal pubblico `by` del flusso (fonte di verità); i permessi
  degli incarichi (`batch.forward`, ecc.) servono alle azioni fuori dai flussi.
- Gli amministratori radice hanno tutti i permessi e possono eseguire ogni passaggio.

### Canali e cifratura
- Ogni evento non pubblico deve essere cifrato per il canale previsto dalle regole (`rules.rs`,
  condiviso da motore e client): altrimenti è invalido. Gli eventi organizzativi vanno nel canale
  `Organization` (tutti i membri).
- Canali personali (`Subject`): pubblico di lettura **più il soggetto** (§8.2).
- **Bozza compilata per conto**: leggibile dal soggetto e da `fillers_on_behalf` (altrimenti chi
  compila non potrebbe cifrarla). Le bozze compilate dal soggetto restano `subject_only`. Limite:
  chi ha `document.fill_on_behalf` ma non è nel pubblico `fillers_on_behalf` non può creare la
  bozza.
- Lotti: canale del pubblico di lettura dello stato di arrivo, nell'ambito dell'unità e senza
  soggetto (quindi sorveglianza e segretario; dopo la registrazione anche gli anziani).
- Documenti ridistribuiti **uno per evento** quando un passaggio cambia il pubblico (la specifica
  suggerisce un evento per lotto): così ogni lettore ricostruisce il documento dal proprio canale.
- Bozze dei resoconti: nuovo descrittore `PermissionHolders` (chi ha `aggregate.generate`), poi
  distribuzione al pubblico di lettura.
- Programmi: bozze per responsabile e assistenti; all'invio delle assegnazioni l'istantanea va a
  loro e a chi può pubblicare in bacheca; un avviso cifrato per ogni assegnato
  (`Assignment { program_type, person }`: aggiunto il tipo di programma al descrittore).
- Distribuzione e rotazione: il client completa i destinatari mancanti e ruota l'epoch quando un
  membro è uscito o quando non ha la chiave corrente. Il compito automatico di completamento
  (`complete_channels`) gira dopo l'aggiunta di dispositivi; la sincronizzazione (F4) lo
  renderà periodico.
- Gli attributi delle persone viaggiano per ora nel canale `Organization` (servono a tutti per
  verificare i requisiti, come le qualifiche); l'interfaccia mostrerà solo ai pubblici
  `read_audience`. Da rivedere.

### Client
- Ogni azione costruisce i suoi eventi e li verifica con una ricostruzione di prova: il client non
  può produrre eventi che gli altri dispositivi rifiuterebbero.

### Test
- Scenari del pacchetto in YAML (`packages/*/tests`), eseguiti da `cargo xtask test-package`: le
  verifiche di lettura sono **crittografiche** (la vista di un dispositivo con le sole sue chiavi).
  Formato: `given` (persone, gruppi, qualifiche con periodo, incarichi, dispositivi), `when`
  (azioni, `check` intermedi, `concurrent`), `then`. Rispetto all'esempio di §15.4 le verifiche
  "prima di un passo" si scrivono come `check` intermedi.
- Test del motore contro eventi contraffatti (che aggirano il client), quarantena,
  configurazioni e conferimenti concorrenti, e convergenza con proptest: stessi eventi in ordini
  diversi e su dispositivi diversi della stessa persona danno lo stesso `state_hash`.
- Testi del nucleo (non del pacchetto) per promemoria senza testo configurato, es.
  `core.resource_no_log`: verranno dalla UI (F5).
