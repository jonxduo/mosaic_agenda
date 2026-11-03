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

## 2026-11-01 — Fase F4 (sincronizzazione)

### Librerie
- `iroh` 1.4, `iroh-gossip` 0.101 (stessa versione di iroh). Endpoint con la chiave Ed25519 del
  dispositivo; relay n0 in sviluppo, relay propri dalla genesi in produzione; in test solo
  loopback senza relay, con indirizzi insegnati tramite `MemoryLookup`.
- **Allegati senza `iroh-blobs`** nella v1: i blob (già cifrati) viaggiano nel protocollo
  `org-app/sync/1` (`BlobWant`/`Blob`), verificati con l'hash BLAKE3 del contenuto, salvati come
  file con nome = hash. Motivo: API di `iroh-blobs` ampia e una seconda pila di trasferimento per
  file di ~0,5 MB; da riconsiderare se servono trasferimenti riprendibili di file grandi.
- Foto: crate `image` (solo JPEG e PNG): lato lungo 2048 px, JPEG qualità 80 (la ricodifica
  elimina EXIF e GPS), miniatura 256 px qualità 60 (< 20 KB) da mettere nel payload cifrato,
  immagine completa cifrata con la chiave del canale (AAD con kind `blob`).

### Protocollo
- Riepilogo per fasce di 64 livelli di profondità (numero di eventi e XOR degli id); per le fasce
  diverse si scambiano gli id; si inviano gli eventi mancanti dal più vecchio, a lotti di 256 o
  1 MB; i genitori ancora mancanti si chiedono con `Want`. Una sessione di prova senza differenze
  scambia solo i due `Hello`.
- `Session` è una macchina a stati senza I/O: lo stesso codice gira in memoria (test, simulatore)
  e su QUIC. Sulla rete un solo stream bidirezionale, scrittura in un task separato (nessun
  blocco reciproco con lotti grandi), fine sessione quando entrambi sono inattivi e ciascuno ha
  ricevuto tutto ciò che l'altro ha inviato (frame `Idle` con i contatori).
- Ricevute firmate (contesto `org-app/receipt/1`), non eventi; "inviato" con ricevute da almeno 2
  dispositivi di altre persone, "consegnato" quando ogni persona del pubblico ne ha almeno una.
- Un dispositivo accetta sessioni solo da dispositivi noti (`allowed`), salvo il primo contatto.

### Abbinamento
- Ticket `orgapp:` + base32 di postcard; segreto monouso di 32 byte confrontato in tempo costante;
  il dispositivo che si connette deve essere quello delle chiavi presentate; codice a 6 cifre da
  `blake3(segreto || chiavi)`. Dopo la conferma l'app pubblica `device.added` e il compito di
  completamento distribuisce le chiavi: senza la chiave del canale `Organization` il nuovo
  dispositivo non può verificare nulla (verificato nei test).

### Simulatore
- Due fasi: (1) attività reale (eventi firmati, cifrati e validati dal motore) di
  un'organizzazione fittizia generata dal pacchetto con `packages/<nome>/simulation.yaml`
  (profili di popolazione, incarichi, chi ha un desktop d'archivio); (2) propagazione minuto per
  minuto con un modello di disponibilità: desktop d'archivio sempre accesi; telefoni 1–3 sessioni
  al giorno di 2–10 minuti tra le 7 e le 22; risveglio push dopo ogni evento (Android 85% entro
  2 minuti; iOS: notifica visibile aperta nel 60% dei casi dopo in media 45 minuti); due
  dispositivi online si raggiungono via relay e ottengono l'unione degli eventi. La correttezza
  dello scambio è provata in `core-sync`, il simulatore misura i tempi.
- Semplificazione: le azioni non aspettano gli eventi da cui dipenderebbero (es. l'inoltro
  avviene alla data prevista anche se un rapporto arriva dopo). Il generatore crea rapporti,
  lotti, resoconti e programmi; non ancora territori e allegati.
- Risultato (seed 1, dal 1° settembre 2026): 120 persone, 60 giorni, 1 desktop d'archivio →
  100% dei documenti al nuovo pubblico entro 24 ore (mediana 1 min, p95 11 min), ~7,8 MB di
  eventi all'anno per dispositivo, nessun evento invalido. Il volume generato (~220 eventi a
  settimana) è sotto la stima di §1.3 (1.500–2.500): mancano territori, notifiche di lettura e
  correzioni.
- Generare l'attività di 120 persone richiede ~50 s, quasi tutti nella ricostruzione completa a
  ogni azione (prova + vista). Con un anno di dati serve la proiezione incrementale: da fare
  prima di F9 (avvio a freddo < 2 s).

## 2026-11-01 — Fase F5 (desktop)

### Struttura
- `core-api`: facciata `Core` usata da ogni shell. Comandi con DTO (`specta::Type`) già pronti
  per lo schermo: etichette risolte dai testi del pacchetto, date formattate, stati come testo
  (`state.<id>`, `variant.<id>`, `category.<id>`, facoltativi: senza testo l'interfaccia non
  mostra nulla). La UI non conosce il dominio: riceve solo testi e identificatori.
- Le viste si ricalcolano con `rebuild` a ogni lettura dopo un cambiamento (cache della vista
  invalidata a ogni commit). Va bene per F5; la proiezione incrementale resta da fare prima di F9.
- `dispatch(nome, JSON)` espone gli stessi comandi per nome con argomenti camelCase come l'IPC di
  Tauri: lo usa `api-dev-server` (feature `dev-server`, axum, solo 127.0.0.1). In un browser
  `bridge.ts` installa un finto `__TAURI_INTERNALS__.invoke` che fa `POST /api/<comando>`, così
  i binding generati funzionano identici contro un core vero (sviluppo e Playwright).
  `?api_port=8787..8799` sceglie un altro server locale: un solo browser fa più dispositivi.
- Tauri 2 (non 3 alpha): plugin di notifica stabile, tray, keyring di sistema.
  `tauri-specta`/`specta` fissati a `=2.0.0-rc.26` e `specta-typescript` a `=0.0.13`
  (versioni rc incompatibili tra loro). `serde_json::Value` esportato come `unknown`
  (`JsonMap`), i numeri restano `number`.
- `cargo xtask bindings [--check]` rigenera `apps/app/ui/src/lib/bindings.ts`; la CI controlla
  che sia aggiornato.

### Build
- Il pacchetto compilato entra nel binario a build time (`APP_PACKAGE_BUNDLE` → `include_bytes!`).
  Senza pacchetto l'app si rifiuta di partire con un messaggio chiaro.
- `cargo xtask dev|build --package <nome>`: valida e compila il pacchetto, genera le icone dal
  `branding.icon` (`tauri icon`) e una sovrapposizione di `tauri.conf.json` con nome
  dell'app (testo `branding.app_name` nella lingua predefinita), identificatore e icone.
  `--target android|ios` rispondono "fase F7/F8". `--package` accetta anche un percorso
  (serve per la fixture neutra `club` nei test).

### Sicurezza locale
- Segreti nel portachiavi del sistema (`keyring` 4, servizio = identificatore dell'app).
  `FileSecretStore` (JSON in chiaro) esiste solo per `api-dev-server`.
- PIN facoltativo: si salva solo `blake3::derive_key("org-app/pin/1", pin)` nel portachiavi;
  blocco automatico dopo N minuti di inattività (impostazione locale). Il PIN protegge
  l'interfaccia, non cifra il database (la chiave SQLCipher resta nel portachiavi).
- Gli errori arrivano alla UI come `{kind, message}`; la UI mostra un testo generico per tipo
  (`denied`, `not_found`, `invalid`, `locked`, `network`, `internal`) e scrive il dettaglio solo
  in console.

### Comportamenti
- Scadenza di un documento = ultimo giorno del periodo + `due.offset_days` (ottobre, 5 giorni →
  5 novembre).
- L'inoltro di un lotto incompleto è mostrato ma disattivato finché non passa
  `allow_incomplete_after`, con l'indicazione della data.
- La sincronizzazione dopo un commit è raggruppata (300 ms) e annuncia le teste via gossip; in
  background un giro ogni 60 s (desktop) con le notifiche locali nuove (deduplicate su disco).
- Connessione a un peer con timeout di 8 s e sincronizzazioni in parallelo: un peer spento non
  blocca gli altri.

### Test
- `crates/core-api/tests/flow.rs`: tre `Core` con database e endpoint propri: creazione,
  abbinamento con codice, documento → lotto → inoltro → registrazione → resoconto distribuito
  letto da un terzo dispositivo; stesso test sui relay pubblici n0 (`--ignored`, superato).
- Playwright (`apps/app/ui/tests/e2e`): lo stesso flusso dall'interfaccia con tre
  `api-dev-server` locali e la fixture `club` (onboarding, abbinamento con conferma del codice,
  compilazione dalla home, coda di lavoro, generazione e lettura del resoconto). Le operazioni
  di configurazione dell'organizzazione passano dall'API per tenere il test breve.
- Il criterio "tre istanze desktop via relay pubblico" è coperto dal test Rust sui relay n0;
  le tre istanze Tauri reali non sono automatizzate (WebDriver di Tauri non c'è su macOS).

## 2026-11-01 — Proiezione incrementale e PIN che cifra i dati

### Proiezione incrementale (`core-engine::Engine`)
- `Engine` dà sempre lo stesso risultato di `rebuild` sugli stessi eventi (stesso `state_hash`);
  `rebuild` ora è `Engine::build(...).into_projection()`, quindi il codice è uno solo.
- In memoria restano gli eventi già decifrati e decodificati, il passato causale, la cache dei
  permessi e copie dello stato ogni 256 eventi applicati (le 8 più recenti).
- Un evento nuovo che si ordina dopo tutti gli altri si applica sullo stato attuale. Uno che si
  ordina prima (dispositivo rimasto offline) riparte dalla copia più vicina precedente, o dallo
  stato dopo la fase organizzativa. Si ricostruisce tutto solo quando cambia qualcosa che può
  toccare il verdetto di qualunque evento: un evento organizzativo, una risoluzione di
  quarantena, una chiave che apre eventi prima illeggibili, oppure un antenato mancante che
  arriva dopo e cambia il passato causale degli eventi organizzativi.
- Un nuovo evento può far perdere (o vincere) un conflitto a un evento più vecchio: si
  ricalcola l'insieme dei perdenti e si riparte dal primo evento cambiato.
- La prova a secco delle azioni (`Client::finish`) usa `Engine::preview`: aggiunge in modo
  provvisorio, applica su una copia dello stato e poi toglie, senza ricostruire.
- Test: proprietà con ordini di consegna mescolati, a blocchi, rovesciati e in ordine causale
  casuale (ogni evento dopo i genitori) → stesso stato di `rebuild`; un test verifica che il
  percorso di ripresa venga davvero usato.
- Misure (simulatore, 120 persone): 60 giorni generati in 10,8 s (prima ~50 s); un anno =
  9.440 eventi, avvio a freddo 0,09 s (lettura 0,01 s + proiezione 0,08 s).
- Gli eventi riletti dal database locale non si riverificano (firma già controllata
  all'inserimento, database autenticato da SQLCipher): si ricontrolla solo l'id. Era il costo
  principale dell'avvio. Con questi numeri uno snapshot della proiezione su disco non serve
  ora; da rivalutare in F9 con misure su telefono.

### PIN che cifra i dati (§8.6)
- Con il PIN la chiave del database esiste solo cifrata: XChaCha20-Poly1305 con una chiave
  Argon2id (64 MiB, 3 passate) del PIN più un *pepper* casuale di 32 byte nel portachiavi del
  sistema. Copiare la cartella dei dati non basta per provare i PIN. Chi ha anche accesso al
  portachiavi della sessione utente può provarli offline: con un PIN di 4–6 cifre ci vuole
  poco, quindi conviene un PIN più lungo (da dire nell'interfaccia in F9).
- Bloccata = database chiuso, chiave e dati decifrati tolti dalla memoria (vista, motore). Si
  blocca all'avvio, con "Blocca ora" e dopo l'inattività.
- Ad app bloccata la sincronizzazione continua in sola ricezione (`core-sync::local::LocalLog`
  = database aperto o `Inbox`). Gli eventi ricevuti si sigillano (sealed box X25519) con una
  chiave pubblica derivata dalla chiave del database e si scrivono un file ciascuno; allo
  sblocco entrano nel database. Il dispositivo bloccato non invia eventi né ricevute.
- In chiaro restano solo l'indice degli id (hash) e delle profondità già noti, scritto a ogni
  blocco, e lo stesso per gli eventi in attesa: nessun contenuto, autore o tipo.
- Dopo un riavvio da bloccata non si sincronizza finché non si sblocca (non si conoscono i
  dispositivi ammessi senza leggere il database). Ad app bloccata la notifica è solo il testo
  generico del pacchetto (`push.generic`).
- PIN dimenticato: si perde la copia locale; si riabbina il dispositivo e i dati tornano dagli
  altri. La cancellazione dopo N tentativi sbagliati (§8.6) e la biometria arrivano con il
  mobile (F7/F8).

## 2026-11-02 — Fase F6 (server)

### Rete dalla genesi
- `NetworkParams` della genesi ha ora anche `push_url`. L'app usa relay e discovery scritti nella
  genesi della propria organizzazione (`Core::effective_network`); prima della genesi (nuovo
  dispositivo che si abbina) usa quelli della build. `Network::Local` resta per i test.
- La build sceglie il server con `cargo xtask build|dev --server <dominio> [--dns]`:
  `https://relay.<dominio>:8443`, `https://api.<dominio>`, `https://dns.<dominio>/pkarr`
  (variabili `APP_RELAY_URLS`, `APP_PUSH_URL`, `APP_DNS_URL` lette a compilazione). Senza
  `--server`: relay pubblici di n0 (sviluppo).
- Con `dns` il nodo usa solo la discovery propria (pkarr + DNS); senza, i relay propri con la
  discovery di n0.
- `check-domain-leak` ora controlla anche `server/` e ignora i nomi di API esterne che contengono
  per caso un termine di dominio (`PkarrPublisher` di iroh contiene "publisher").

### Servizio push (`server/push-service`, client `core-push`)
- API come da §10.1, firme Ed25519 su `method || path || timestamp (LE) || blake3(body)`, ±5
  minuti; lo stesso codice di firma/verifica in `core-push` per client e server.
- Primo `POST /v1/org/membership` di un'organizzazione: invece di "il primo che registra diventa
  amministratore" (chiunque conosca l'id potrebbe appropriarsene) il chiamante invia l'evento di
  genesi; il servizio verifica firma e id e accetta solo un dispositivo di un amministratore
  radice. Gli amministratori del servizio sono i dispositivi degli amministratori radice (lista
  aggiornata da loro). La genesi non viene salvata.
- `register` è accettato solo per dispositivi già nell'elenco dell'organizzazione; quando un
  dispositivo esce dall'elenco il suo token viene cancellato. Token che FCM/APNs dichiarano non
  validi (404/410) vengono cancellati.
- Limiti: 60 `wake` all'ora per mittente, 10 push all'ora per destinatario, una sola push per
  destinatario in 2 minuti; massimo 500 destinatari per richiesta e 10.000 dispositivi per
  organizzazione.
- Invio: FCM HTTP v1 (JWT RS256 del service account → token OAuth, in cache), APNs con token
  ES256 (`.p8`, rinnovato ogni 50 minuti), HTTP/2. JWT con `jsonwebtoken` in Rust puro;
  TLS con rustls + ring come il resto del progetto.
- Nell'app: dopo un'azione locale si attende 10 s la consegna diretta, poi si chiede il
  risveglio dei dispositivi destinatari senza ricevuta (al massimo una richiesta ogni 2 minuti
  per dispositivo). Un amministratore radice pubblica l'elenco dei dispositivi quando cambia
  (impronta salvata nelle impostazioni locali). La registrazione del token arriva con il mobile
  (F7); `PushProvider` con l'implementazione vuota del desktop è già in `core-push`.
- Backup giornaliero dell'archivio (`VACUUM INTO`, 7 copie a rotazione).

### Deploy (`server/deploy`)
- Relay e DNS: binari ufficiali compilati da crates.io alla versione di iroh dell'app
  (`Dockerfile.iroh`, `IROH_VERSION=1.4.1`); configurazioni come modelli con `envsubst`.
- Stesso IP per tutto: Caddy tiene 80/443 (`api.`, `dns.` e il certificato di `relay.`), il
  relay serve HTTPS sulla 8443 con il certificato di Caddy riletto periodicamente
  (`cert_mode = "Reloading"`), QUIC sulla 7842. La discovery è un profilo facoltativo (`dns`).
- Accesso al relay: aperto a tutti (con limiti di connessione). Un controllo per dispositivo
  (modalità `http` del relay verso il servizio push) bloccherebbe l'abbinamento dei nuovi
  dispositivi, che non sono ancora nell'elenco: da rivedere.
- Verifica: Docker non è disponibile sulla macchina di sviluppo; la CI (job `server`) avvia
  relay e servizio push con `docker compose` e fa girare il flusso a tre dispositivi sul relay
  proprio. In locale lo stesso flusso è passato con il binario ufficiale `iroh-relay --dev`
  (metriche del relay: client collegati).

## 2026-11-02 — Fase F7 (mobile), prima parte

- CI su GitHub solo a mano (`workflow_dispatch`) o sui tag `v*`: i controlli quotidiani girano in
  locale; la CI completa si usa per le release.
- Progetti nativi (`gen/android`, `gen/apple`) generati da `cargo xtask build --target
  android|ios` per il pacchetto scelto (l'identificativo dell'app è nel progetto nativo) e non
  versionati; un file `.package-identifier` dice per quale pacchetto sono stati generati. Le
  parti native nostre stanno in plugin Tauri (`apps/app/src-tauri/plugins/`).
- `SecretStore` mobile: plugin `secret-store`. Android: valori cifrati AES-GCM con una chiave
  del Keystore non esportabile, salvati nelle preferenze private dell'app. iOS: Keychain,
  `AfterFirstUnlockThisDeviceOnly` (leggibile in background dopo il primo sblocco, mai in
  backup o su altri dispositivi). Lo stesso plugin nasconde il contenuto nel selettore delle
  app (Android 13+: `setRecentsScreenshotEnabled(false)`, prima `FLAG_SECURE`; iOS: sfocatura).
- Biometria: plugin ufficiale `biometric` (solo verifica dell'identità). Attivandola, una copia
  della chiave del database va nel `SecretStore` e viene usata dopo una verifica riuscita. Non è
  una chiave legata alla biometria dal sistema (il plugin non lo permette): protegge dall'uso
  dell'app sbloccata da parte di altri, non da chi esegue codice come l'app. Disattivata se si
  toglie il PIN.
- Cancellazione dopo N PIN errati (§8.6): il limite viene dalla genesi (`SecurityParams`) e si
  copia nelle impostazioni locali a ogni sblocco, perché da bloccata la genesi non è leggibile.
  Alla soglia si cancellano database, posta in arrivo, allegati, impostazioni e segreti
  (anche le chiavi del dispositivo); l'app mostra "Dati cancellati" e va riaperta; i dati
  tornano riabbinando il dispositivo. Il limite per una nuova organizzazione si passa con
  `CoreOptions::security`.
- QR: plugin ufficiale `barcode-scanner` per leggere l'invito al primo avvio.
- Versioni dei plugin JS allineate a quelle Rust (il CLI di Tauri rifiuta major/minor diversi).

## 2026-11-02 — Fase F7 (mobile), iOS

- Prima build iOS, provata sul simulatore (iPhone 18 Pro, iOS 27.2): informativa, creazione
  dell'organizzazione, home; contenuto fuori da status bar e indicatore home
  (`env(safe-area-inset-*)` con `viewport-fit=cover`); dati e chiavi conservati dopo la
  chiusura forzata (Keychain).
- `libc` fissata a `=0.2.189` (solo per iOS, nel `Cargo.toml` dell'app): dalla 0.2.190 libc
  non definisce più per iOS le costanti di routing (`RTM_*`, `RTAX_*`) e `in6_ifreq`, e
  `netwatch`/`netdev` (usati da iroh) non compilano. Da togliere quando iroh passa a
  `netdev` 0.47 o libc torna a definirle.
- `SystemConfiguration.framework` collegato all'app iOS (`bundle.iOS.frameworks` in
  `tauri.conf.json`): lo usano `netdev` e `n0-dns-resolver`; le librerie statiche Rust non
  portano con sé i framework.
- Descrizioni d'uso di fotocamera (QR) e Face ID in `apps/app/src-tauri/Info.ios.plist`, che la
  CLI di Tauri unisce al plist generato a ogni build (niente modifiche a `gen/`).
- `cargo xtask build --target ios`:
  - la fase Xcode "Build Rust Code" lancia `node tauri …` da `gen/apple` e non riceve
    l'ambiente di xtask: xtask scrive lì uno script `tauri` che imposta `APP_PACKAGE_BUNDLE`
    (e gli URL del server) e poi avvia la CLI. Senza, l'app partiva senza pacchetto e si
    chiudeva subito;
  - il progetto nativo si rigenera anche quando cambia `tauri.conf.json` (alcune impostazioni,
    come i framework, valgono solo all'`init`);
  - SDK e NDK Android solo per `--target android` (i loro strumenti nel `PATH` coprirebbero il
    `clang` di Apple); per iOS `LANG=en_US.UTF-8` richiesto da CocoaPods.
- Strumenti in più per iOS: CocoaPods (`brew install cocoapods`; senza, la CLI prova a
  installarlo con `sudo gem`).
- Non provati sul simulatore: Face ID, lettura del QR (nessuna fotocamera), push.

## 2026-11-02 — Aspetto: colori del logo, splash, testo formattato

- Colore principale dal logo: `#6A4CE6` (viola-blu al centro del gradiente; contrasto 5,5:1 con
  il bianco), `#A99BFF` nel tema scuro (7,7:1). Il gradiente del logo (`--brand-gradient`) solo
  come accento (nome dell'app nella barra laterale). Il colore del pacchetto
  (`branding.primary_color`) ora vale solo per il tema chiaro: prima sovrascriveva anche il
  tema scuro.
- Grafica facoltativa del pacchetto, per convenzione in `branding/`: `logo.png` (barra
  laterale), `mark.png` (logo monocromatico, sfondo chiarissimo delle schermate vuote),
  `splash.mp4` + `splash.webp` (avvio). `cargo xtask bundle` li copia in
  `apps/app/ui/static/package/` (generata, non versionata); l'interfaccia li usa se ci sono.
- Splash: il video fornito (10 s, 1280×720, 3,3 MB, con audio) diventa un MP4 H.264 quadrato
  480×480 senza audio da 164 KB, più l'ultimo fotogramma in WebP (7 KB). Niente GIF: con 256
  colori i gradienti del logo si rovinano e pesa molto di più. Al primo avvio l'animazione
  si vede intera prima dell'informativa (un tocco la salta); poi solo se l'avvio dura più di
  250 ms. Con "riduci movimento" solo l'immagine fissa; nei test automatici
  (`navigator.webdriver`) niente introduzione.
- Informativa e altri testi del pacchetto in Markdown: convertitore minimo nostro
  (`markdown.ts`: titoli, paragrafi, citazioni, elenchi, grassetto, corsivo, link web) che
  prima fa l'escape di tutto, così nessun HTML del testo arriva alla pagina.

## 2026-11-02 — F8: amministrazione e configurazione

- **Sorgenti nel bundle** (formato compilato 2): `CompiledConfig.sources` porta i file YAML
  del pacchetto (package.yaml, file del manifest, testi, informative). L'editor parte da
  quelli della versione in uso, non da una cartella sul disco; `config.published` porta il
  nuovo bundle con le sue sorgenti, così il desktop successivo può ancora modificarle.
- **Migrazioni** (`core-config::migrate`): un elemento con id (qualifica, tipo di unità,
  incarico, pubblico, modulo, flusso, resoconto, tipo di evento, tipo di programma, slot, tipo
  di risorsa) che sparisce va dichiarato in `migrations` di package.yaml come `rename: {from,
  to}` (il nuovo id deve esistere, il vecchio no) o `retire: {id}`; altrimenti la versione è
  rifiutata. Lo stesso controllo lo fa il motore su `config.published` (una versione che lo
  viola è negata a tutti i dispositivi), e applica i rename alle assegnazioni esistenti
  (qualifiche, incarichi, tipi di unità). Cambiare l'id del pacchetto è vietato.
- **Simulazione**: la nuova versione viene pubblicata su una copia degli eventi e il motore
  ricostruito da zero; si mostrano errori, differenze elemento per elemento (slot come
  `programma.slot`, testi per chiave e lingua), persone entrate/uscite da ogni pubblico (alla
  data di oggi), avvisi nuovi o spariti e documenti aperti. Niente rete, niente scrittura.
- La pubblicazione numera la versione `max + 1` e richiede `config.publish` (o il ruolo
  radice). Se la nuova versione chiede una versione del nucleo più recente, l'editor lo
  segnala: servirà un aggiornamento dell'app.
- **Editor**: due viste sugli stessi file. *YAML*: testo con validazione in tempo reale (600 ms
  dopo l'ultima battuta) ed errori per file. *Guidato* (§12.4): incarichi (ambito, requisiti,
  titolari, permessi, sostituzioni…), pubblici, campi dei moduli (per variante), slot dei
  programmi, promemoria e testi (per lingua, con ricerca). Le etichette mostrano la chiave e il
  testo, modificabili insieme. Condizioni e liste (requisiti, permessi, `select`, campi
  aggiuntivi) si scrivono come una riga di YAML: un editor visuale delle condizioni è un
  miglioramento futuro.
  - Le modifiche passano dalla libreria `yaml` (eemeli/yaml 2.8.3) che conserva commenti,
    ancore (`&`/`*`) e il resto del file; un valore modificato viene riscritto in stile
    inline e un valore che era un alias diventa una copia.
  - Cambiare o eliminare l'id di un elemento che c'era nella versione attiva aggiunge da solo
    la migrazione in package.yaml (`rename`, che segue rinomine precedenti, o `retire`). I
    riferimenti negli altri file **non** vengono aggiornati: la validazione li segnala.
  - Nel confronto tra versioni gli slot rinominati appaiono come "rinominato
    `programma.vecchio → programma.nuovo`".
- L'aggiornamento periodico della UI (20 s) rimontava la vista e cancellava le modifiche non
  salvate: ora non tocca le viste di modifica (`EDITING` in App.svelte) e solo se sono
  arrivati eventi.
- **Dispositivi archivio**: dalla scheda della persona (Persone) si attiva/disattiva
  `device.archive_set` (serve `devices.manage`) e si revoca un dispositivo. Un dispositivo
  archivio chiede ai pari tutti gli allegati citati dagli eventi (gli altri li prendono su
  richiesta) ed è il primo contattato (già in F4).
- **Backup** (`.orgbackup`): `ORGBACKUP1\n` + `PassphraseBox` (Argon2id sulla passphrase,
  almeno 8 caratteri, **senza** il pepe del portachiavi, altrimenti non si aprirebbe su un
  computer nuovo; XChaCha20-Poly1305) con dentro chiavi del dispositivo, tutti gli eventi e
  gli allegati. Il ripristino va solo su un dispositivo vuoto, poi chiede di riaprire l'app
  (stato `restart`). Il dispositivo ripristinato **è lo stesso** (stessa identità): il
  vecchio non va più usato, altrimenti due copie firmerebbero con la stessa chiave. Per un
  computer in più si usa l'invito, non il backup. Nell'app il file si salva con la finestra di
  sistema (`tauri-plugin-dialog`), nel browser di sviluppo si scarica.

## 2026-11-02 — F9: privacy e rifinitura

- **Eventi cancellabili ("gusci")**: l'id di un evento ora copre il contenuto solo tramite il
  suo hash (`id = blake3(postcard(corpo con il contenuto sostituito dal suo hash))`). Un
  evento cancellato tiene `Content::Erased(hash)` al posto del contenuto: stesso id, stessa
  firma, ancora verificabile, quindi resta nel grafo e si sincronizza come gli altri. Chi
  riceve il guscio da un dispositivo e l'evento intero da un altro tiene quello arrivato per
  primo; il motore poi lo cancella comunque se rientra in una cancellazione. **Il cambio di
  formato rende illeggibili gli archivi creati prima** (nessuno in uso: solo dati di
  sviluppo, spostati in `target/dev-data-before-f9`).
- **Cosa si cancella** (`Engine::erasure_targets`, calcolato da ogni dispositivo con quello
  che riesce a leggere, come chiede §14):
  - `person.data_erasure_requested` (solo amministratori radice): gli eventi della persona
    (aggiunta e modifiche: nome e attributi) e tutti gli eventi dei documenti di cui è
    soggetto, con i loro allegati;
  - `retention.applied { before }`: documenti il cui periodo è finito da più della
    `retention` del modulo e programmi (con le assegnazioni) oltre la `retention` del tipo di
    programma. Lotti, resoconti e storico delle risorse restano;
  - ogni evento successivo su un oggetto già cancellato (per esempio una modifica arrivata in
    ritardo da un dispositivo rimasto spento).
- La persona cancellata **resta** nell'organizzazione senza nome né attributi (l'intestazione
  in chiaro degli eventi persona ora porta l'id della persona): qualifiche, incarichi,
  assegnazioni passate e lotti restano validi e nessun evento diventa invalido. Il nome
  mostrato è "—". Se la persona deve anche uscire, l'amministratore chiude incarichi e
  dispositivi come per chiunque.
- Nell'archivio: l'evento viene riscritto come guscio, `PRAGMA secure_delete = ON` perché il
  contenuto non resti nelle pagine libere del database, allegati eliminati dalla cartella dei
  blob; un dispositivo archivio non li richiede più.
- **Conservazione automatica**: il dispositivo di un amministratore radice pubblica
  `retention.applied` con la data di oggi al massimo una volta alla settimana, durante la
  sincronizzazione, se il pacchetto ha almeno una `retention`.
- **Esportazione** (`export_person`): tutto ciò che il dispositivo vede in chiaro della
  persona (attributi, qualifiche, incarichi, documenti con i valori e la storia,
  assegnazioni, risorse). La può chiedere la persona stessa (Impostazioni → Dati personali)
  o chi gestisce le persone (Persone → scheda). Formato JSON (file salvato con la finestra di
  sistema) o pagina stampabile: il "PDF" è la stampa di sistema ("Salva come PDF"), senza
  librerie in più.
- L'informativa resta consultabile in Impostazioni → Dati personali.
- **Accessibilità**: test automatico (axe, regole WCAG 2.1 A/AA) delle viste principali in
  tema chiaro e scuro dentro i test Playwright. Corretto il contrasto del pulsante principale
  e dei messaggi d'errore nel tema scuro (`--on-primary`, testo scuro su colori chiari).
- Un test verifica che ogni comando della tabella di `dispatch` abbia il suo gestore (il
  server di sviluppo e il ponte del browser usano quella tabella).
- **Prestazioni** (`cargo xtask simulate --package congregazione --devices 120 --days 365`,
  build release, Mac Apple Silicon): 9.440 eventi in un anno (658 di impianto, 8.782 di
  attività), ~4,6 MB per dispositivo. Avvio a freddo del nucleo dell'amministratore: lettura
  degli eventi 0,01 s, proiezione da zero 0,07 s. Con un margine di 5–10 volte per un
  telefono di fascia media il nucleo resta sotto 1 s; il resto dell'avvio è la WebView. La
  misura completa (dall'icona alla home) su un telefono reale è la prova 23 di
  `docs/MOBILE_CHECKLIST.md`: non c'è un telefono a disposizione, quindi il criterio "< 2 s
  su telefono di fascia media" resta da confermare lì.

## 2026-11-02 — Calendario e turni

- **Vista "Calendario e turni"** (core, per chi può pubblicare la configurazione): giorni e ora
  dei tipi di evento con ricorrenza settimanale (`default_recurrence`, `default_time` in
  calendar.yaml) e, per i tipi di programma che coprono più tipi di evento, la rotazione e gli
  slot da assegnare. Modifica calendar.yaml/programs.yaml con lo stesso editor YAML che
  conserva i commenti e pubblica una nuova versione: niente evento nuovo, la configurazione
  resta l'unica sorgente.
- **`ProgramTypeDef.rotation`**: `occurrence` (predefinito) | `week` | `month`. Il motore non
  cambia: l'API espande una scelta a tutte le occorrenze del gruppo (`assign_slot`, svuotare
  con persona vuota) e una risposta a tutte le occorrenze del gruppo tenute dalla stessa
  persona (`respond`, `Client::respond_all`). Home, bacheca ed editor mostrano un elemento per
  gruppo. `rotation: month` richiede `period: month` (validazione).
- **`SlotDef.enabled`** (predefinito `true`): il pacchetto offre un catalogo di slot,
  l'organizzazione sceglie quali assegnare. Disattivare non è una rimozione: niente migrazione.
- **Date proposte** (`program_dates`): dai giorni `BYDAY` delle ricorrenze settimanali dei tipi
  di evento del programma, nel periodo dopo l'ultimo programma del tipo. L'utente le può
  togliere; le eccezioni di calendario restano manuali.
- **Formato del bundle 3**: intestazione con le sorgenti YAML *prima* del pacchetto compilato.
  postcard non è auto-descrittivo, quindi un campo aggiunto al modello rendeva illeggibili le
  versioni già pubblicate; ora, se la parte compilata non si legge, il nucleo ricompila il
  pacchetto dalle sorgenti. I dati di sviluppo in formato 2 non si leggono più
  (spostati in `target/dev-data-before-schedule`).
- **Riga delle persone** (`ui.people` in ui.yaml): il pacchetto sceglie gli attributi
  (`attributes`), le qualifiche mostrate con la data da cui valgono (`dated_qualifications`) e
  quelle mostrate come contrassegni (`qualifications`). Il nucleo restituisce `facts`
  (date prima, poi le scelte senza etichetta) e `marks`; `values` riempie il modulo di modifica
  con i valori attuali. Congregazione: nascita, battesimo, sesso, speranza
  (`birth_date`, `hope`, nuovi attributi per anziani e amministratori), nomine e servizi.
- **`SlotDef.each_event`**: uno slot può cambiare persona a ogni evento anche quando il
  programma ruota per settimana o mese; l'editor lo mostra sotto ogni data del gruppo
  (`SlotCell.per_event`, `ProgramGroup.days`). In "Calendario e turni" gli incarichi si
  aggiungono (id dal nome, testo `s.<id>` nella lingua predefinita, requisiti del primo slot del
  programma), si rinominano (solo il testo) e si eliminano (`retire` se l'id sparisce dal
  pacchetto).
- **Conferme nell'app** (`ask.svelte.ts`): `window.confirm` non viene mostrato dalla webview e
  risponde "no" in silenzio; ogni conferma è ora un dialogo dell'app. "Calendario e turni"
  controlla prima di pubblicare che la versione non sia cambiata nel frattempo ed elenca gli
  errori di `config_check` invece di un generico "Dati non validi".
- **Vista `qualification_matrix`** (core, generica): righe = persone, colonne = le qualifiche
  elencate nel parametro `qualifications` del menu; una spunta concede la qualifica, toglierla
  la chiude (oggi). Le celle di chi non soddisfa `requires` o tiene una qualifica esclusa sono
  disattivate. Le modifiche restano in pagina fino a "Salva" (`set_qualifications`, un evento
  per cella). Congregazione: "Incarichi" mostra le abilitazioni `can_*` (usciere, audio/video,
  microfonista, presidenti, discorso, tesori, gemme, lettura biblica, dimostrazione, vita
  cristiana, studio biblico, lettori, preghiera); gli slot dei programmi le richiedono, così
  l'editor propone solo chi è abilitato. Aggiunto lo slot `chairman` all'adunanza pubblica.

## 2026-11-03 — Schermate secondo i permessi; avvisi alla pubblicazione

- **Una schermata di gestione si vede solo se si può agire** (core, `access.rs`): ogni vista ha
  i permessi che la rendono utile (`qualification_matrix` → `qualifications.manage`; `people` →
  `people.manage`, `qualifications.manage` o `roles.grant`; `units` → `units.manage`; `roles` →
  `roles.grant`; configurazione → `config.publish`; `resource_list` → gestire o assegnare quel
  tipo di risorsa). Il menu nasconde le voci senza permesso, oltre a `visible_to`, e le viste
  `people`, `units`, `qualification_matrix` rispondono `denied` se chieste comunque (link vecchi,
  altri client). L'esportazione dei propri dati resta possibile a tutti.
- **Persone divisa per permesso**: dati, gruppo, dispositivi e privacy con `people.manage`;
  nomine/qualifiche con `qualifications.manage` (`can_qualify`); ruoli con `roles.grant`
  (`can_grant_roles`).
- Congregazione: la matrice "Incarichi" e le qualifiche le gestiscono solo il **segretario** e
  il nuovo ruolo **assistente del segretario** (anziano o servitore, `assistant_of: secretary`);
  `app_admin` non ha più `qualifications.manage`. Tolti i `visible_to` ridondanti dal menu.
- **`notify: publish`** (opzione del tipo di programma, core; predefinito `send` = comportamento
  di prima): chi prepara il programma vede solo "Salva"; salvare consegna la versione a chi
  pubblica (evento `Team` + nuovo `Roster`: chi tiene cosa, senza dare accesso alla bozza) e il
  programma è "Da pubblicare", con una voce in Oggi per chi può pubblicarlo. Gli incaricati non
  ricevono nulla finché il responsabile della bacheca non pubblica: in quel momento chi pubblica
  scrive gli avvisi (`Notice`) dal `Roster`. Per questo chi ha `program.publish` legge i canali
  degli incarichi dei tipi `notify: publish`, e il motore accetta da lui gli avvisi che
  corrispondono al `Roster`. Alla ripubblicazione ricevono un avviso solo le persone i cui turni
  sono cambiati; chi non ha più turni riceve un avviso vuoto che cancella il vecchio.
  Congregazione: tutti i programmi usano `notify: publish`.
- Le organizzazioni esistenti ricevono queste opzioni solo pubblicando la nuova configurazione
  (la configurazione vive nello storico dell'organizzazione).

## 2026-11-03 — Programmi mensili delle adunanze, precompilazione, bundle compresso

- **Dettagli degli slot** (core): i campi extra di uno slot (titolo, minuti, brano…) vivono
  nel programma anche senza persona assegnata (`Program.details`, evento nuovo
  `program.details_set`, in coda all'elenco dei tipi). Nella versione inviata e pubblicata i
  dettagli si fondono con gli extra dell'assegnazione. L'editor li mostra sotto ogni incarico,
  modificabili; "Salva" li invia insieme alle scelte (`assign_slots(program, changes, details)`),
  un testo vuoto cancella il campo.
- **`prefill`** (opzione del tipo di programma, core generico): un indirizzo per evento
  (`{year}`/`{week}` = anno e settimana ISO, `{date}`, `{month}`, `{day}`), un `follow` facoltativo
  (regex sul testo del primo link da aprire) e regole provate in ordine su ogni titolo della
  pagina: `section`/`heading`/`text` sono regex sul titolo superiore, sul titolo e sul testo
  sotto; `set` compone i campi con i gruppi nominati. La prima regola che combacia prende il
  titolo (anche senza `slot`, per ignorarlo); gli slot ripetuti si riempiono in ordine. La
  pagina è ridotta a "schema" (titoli h1–h6 con il testo sotto e i link) da un lettore HTML
  minimo, senza dipendenze nuove. Si riempiono solo i campi vuoti; l'editor legge le pagine da
  solo alla prima apertura (nessun dettaglio ancora) e ha il pulsante "Precompila". Una pagina
  che non si legge lascia l'evento com'è.
- Congregazione: adunanza infrasettimanale e del fine settimana passano a **programmi mensili**
  (una sezione per adunanza). L'infrasettimanale legge la Guida della settimana da
  wol.jw.org (discorso dei Tesori, brano della lettura biblica, parti degli studenti con minuti,
  parti di Vita cristiana con minuti, materiale dello studio biblico); il fine settimana legge
  il titolo dello Studio Torre di Guardia (campo "Articolo" del lettore). Verificato sulle
  settimane 45 e 47 del 2026 con il test `live_pages` (ignorato di default: rete).
- **Bundle formato 4**: sorgenti e pacchetto compilato sono compressi (deflate, `miniz_oxide`)
  dopo un'intestazione con formato e versione del core. Il pacchetto aveva superato i 64 KB di
  un evento (67 KB → 16 KB). Il formato 3 resta leggibile: le organizzazioni esistenti lo hanno
  nella loro storia.
- Interfaccia: su desktop i programmi senza dettagli (audio/video, uscieri) mettono i riquadri
  degli eventi due per riga. Su telefono "Sincro" è l'ultima voce del menu; il pulsante Menu
  mostra un pallino arancione se ci sono eventi da inviare.

## 2026-11-03 — L'amministratore legge ogni canale

- Gli amministratori (root) avevano già ogni permesso (`is_allowed`), ma non le chiavi dei
  canali dei gruppi di lavoro: per esempio non potevano preparare l'adunanza infrasettimanale
  senza il ruolo di sorvegliante ("this device is not a member of the channel"). Ora
  `channels::members` aggiunge sempre i root a ogni canale: possono fare tutto a prescindere da
  ruoli e qualifiche. Le chiavi dei canali già esistenti arrivano da `complete_channels`, come
  per ogni nuovo membro. Conseguenza voluta: i root leggono anche i dati personali (rapporti).
- Scenario 10: l'amministratore senza ruoli legge la bozza e prepara un programma.

## 2026-11-03 — Parti degli studenti: sinossi, lezione, interlocutore

- Congregazione: `student_part` ha due campi in più, `synopsis` (testo lungo) e `lesson`. La
  regola della Guida legge "(N min) sinossi (lezione)": la parentesi finale è la lezione, il
  resto la sinossi. Nei modelli di `set` un gruppo facoltativo che non ha preso nulla diventa
  vuoto (prima restava `{nome}`).
- Editor (core + UI): ogni cella dice `partner` (lo slot di `pair_with`) e `paired` (è il
  partner di un'altra). La scelta del partner (l'interlocutore) compare dentro la parte, subito
  sotto la sua scelta, non più in fondo alla sezione. I campi `long_text` sono aree di testo a
  tutta larghezza.

## 2026-11-03 — Programma in righe, sezioni, classe supplementare

- **Opzioni del tipo di programma** (core): `options: [{ id, label, enabled }]`; uno slot con
  `option` è attivo solo se l'opzione è accesa (`ProgramTypeDef::slot_active`). Gli
  amministratori le accendono in "Calendario e turni" ("Opzioni dei programmi").
- **Disposizione** (core): `sections: [{ id, label, color }]` e `SlotDef.section` mettono una
  fascia colorata dove la sezione cambia; `beside: <slot>` mostra lo slot nella riga di un altro,
  nella colonna della sua opzione (una seconda sala); `with: <slot>` nella stessa riga, sotto
  la persona dell'altro (es. il lettore); il partner di `pair_with` va nella riga della sua parte.
  `assignable: false` = slot senza persona, solo dettagli (es. il numero del cantico); il motore
  rifiuta assegnazioni su questi slot. L'editor riceve per ogni cella `row`, `column`, `section`,
  `assignable` e le colonne extra in uso.
- Editor: i programmi con dettagli mostrano una riga per parte, a sinistra la parte e i suoi
  campi, a destra chi la svolge (4/5 + 1/5; con la classe supplementare 4/6 + 1/6 + 1/6); la pagina
  usa fino a 1180 px. Su telefono le colonne si impilano con la loro etichetta. Audio/video e
  uscieri restano a riquadri, due per riga.
- Congregazione, infrasettimanale: presidente, consigliere ausiliario (classe supplementare),
  cantico e preghiera; fascia grigia Tesori (discorso, gemme, lettura biblica + classe
  supplementare); fascia gialla Efficaci nel ministero (parti degli studenti con interlocutore,
  + classe supplementare); fascia rossa Vita cristiana (cantico, parti, studio biblico con
  lettore, cantico, preghiera conclusiva). I numeri dei tre cantici si leggono dalla Guida.
  La classe supplementare è spenta nel pacchetto.

## 2026-11-03 — Assegnazione con trascinamento

- L'editor a righe ha accanto un elenco fisso (con scorrimento proprio) di tutte le persone,
  ordinate per settimane dall'ultimo incarico in un programma dello stesso tipo prima di questo
  (`ProgramEditor.people`, `people_load`): prima chi non ne ha mai avuti, poi chi aspetta da più
  tempo, a parità per nome. Chi ha già un posto nel programma aperto scende in fondo ("ora").
- Le scelte sono riquadri tratteggiati con l'etichetta al centro, tutti alti uguale (64 px):
  vi si trascina un nome dall'elenco. Si accetta solo chi è tra i candidati dello slot (stesse
  regole del motore e della tabella Incarichi), altrimenti un avviso. Un clic sul riquadro apre
  l'elenco dei nomi (un `select` trasparente sopra): vale per tastiera e telefono, dove
  l'elenco laterale è nascosto (sotto i 1000 px). La × toglie la persona.

## 2026-11-03 — Un solo editor per tutti i programmi; elenco per settimana

- Tutti i programmi (anche audio/video e uscieri) usano l'editor a righe con l'elenco delle
  persone e il trascinamento; tolto il modo a riquadri. Gli slot che cambiano a ogni evento
  dentro un gruppo hanno righe proprie sotto il giorno.
- L'elenco segue la settimana (gruppo di eventi) sullo schermo: mostra solo chi è candidato
  per almeno uno slot di quella settimana e conta le settimane dall'ultimo slot prima della sua
  data, guardando gli altri programmi del tipo (`PersonLoad.last`) e le settimane precedenti di
  questo (scelte non ancora salvate comprese). Chi ha già un posto quella settimana è "ora", in
  fondo. `ProgramEditor.dates` dà le date degli eventi.
- Congregazione, fine settimana: cantico, preghiera iniziale, presidente, discorso pubblico,
  cantico, Studio Torre di Guardia (tema a sinistra; conduttore e lettore nella stessa riga),
  cantico, preghiera finale. Nuova abilitazione `can_wt_conductor` (anziani), anche nella
  tabella Incarichi; il tema dello studio si legge dalla pagina della settimana.

## 2026-11-03 — Modulo della persona

- Il dettaglio di una persona è un modulo solo, in sezioni a griglia: Dati (attributi e le
  qualifiche "con data" del pacchetto, es. "Battezzato il": la data dà la qualifica, vuota la
  toglie), Nomine e servizi (ogni qualifica di `ui.people.qualifications` è una casella con la
  data di inizio; spuntata senza data prende oggi, tolta finisce oggi), Gruppo e ruoli. Un solo
  "Salva"; sotto restano gli attributi `ui.people.with_devices` (es. "Usa l'app", salvati
  subito) e i dispositivi. Esportazione e cancellazione dei dati in alto a destra.
- Cambiare o cancellare una data è una correzione: la concessione precedente si chiude alla sua
  data d'inizio (non è mai valsa) e se ne crea una nuova. Al salvataggio prima le chiusure, poi
  le concessioni (così una nomina può sostituirne una che esclude).
- Ruoli: caselle a scelta multipla, senza date. Un ruolo di unità vale per l'unità della persona
  di quel tipo; solo un ruolo che `supervises` chiede l'unità ("Gruppo supervisionato").
  Congregazione: aggiunto il pioniere ausiliare alle caselle e alle etichette della riga.
- Prestazioni: il JSON Schema del pacchetto e i suoi validatori si costruiscono una volta per
  processo, e un bundle ricompilato dai sorgenti (modello cambiato) si ricorda per contenuto.
  Prima ogni modifica ricompilava tutte le versioni della configurazione dello storico (minuti
  in debug con 9 versioni).

## 2026-11-03 — Nomine che si sostituiscono

- `grant_qualification` da una data D chiude in D le qualifiche escluse (nei due sensi) che la
  persona aveva in D, anche se erano già state chiuse dopo (es. servitore chiuso oggi e anziano
  retrodatato in un salvataggio successivo): la nuova sostituisce la vecchia da quel giorno e
  non si sovrappongono. Una qualifica esclusa che comincia dopo D resta un errore.

## 2026-11-03 — Altri incarichi nella stessa settimana

- Nell'elenco delle persone dell'editor, sotto il nome, una riga piccola elenca gli incarichi
  della persona nella stessa settimana (lunedì–domenica della settimana sullo schermo) negli
  altri programmi consegnati a chi pubblica, pubblicati o no (`ProgramEditor.others`). La fonte
  è l'ultimo `Roster` di ogni programma, quindi vale per i tipi `notify: publish` e per ciò che
  il dispositivo può leggere (gli amministratori leggono tutto).
- Tipo di programma, `short` (sigla, es. "VM", "AP"): nella riga degli altri incarichi si
  aggiunge allo slot che ha lo stesso id in un altro tipo (presidente, preghiere), dove il nome
  da solo sarebbe ambiguo. Un tocco su un riquadro, al telefono, apre lo stesso elenco del
  computer per la settimana di quello slot, limitato a chi può svolgerlo.
