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
- **Editor**: per ora solo la vista testuale YAML con validazione in tempo reale (600 ms dopo
  l'ultima battuta), errori per file, simulazione e pubblicazione. Gli editor guidati di §12.4
  (incarichi, pubblici, campi, slot, testi, promemoria) **restano da fare**: producono gli
  stessi file, quindi si aggiungono sopra senza cambiare il resto.
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
