# Progetto: app P2P per attività di organizzazioni — nucleo astratto + pacchetto Congregazione

> **Questo documento è autosufficiente.** Contiene tutto ciò che serve per realizzare il progetto da zero: requisiti, architettura, struttura del repository, modello dati, formato dei pacchetti di configurazione, protocolli, sicurezza, server, interfaccia, test, fasi di lavoro e criteri di accettazione. Un agente AI deve poterlo seguire senza altre fonti, salvo la documentazione ufficiale delle librerie citate.

---

## 0. Istruzioni per l'agente che realizza il progetto

1. **Leggi tutto il documento prima di scrivere codice.** Le sezioni si richiamano a vicenda; il modello dati (§5), il formato eventi (§7) e la cifratura (§8) vincolano tutto il resto.
2. **Lavora per fasi (§16), nell'ordine indicato.** Non iniziare una fase finché i criteri di accettazione della precedente non passano.
3. **Il nucleo non contiene concetti di dominio.** Nessuna stringa, tipo, enum o condizione nel codice dei crate `core-*` o nell'interfaccia può riferirsi a "anziano", "rapporto", "territorio", "congregazione" o simili. Tutto il dominio sta in `packages/congregazione/`. Un test automatico (§15.6) deve verificarlo.
4. **Identificatori nel codice in inglese, contenuti del pacchetto in italiano.** Il glossario (§18) collega i termini.
5. **Verifica le versioni delle librerie all'inizio.** Le versioni indicate sono riferimenti (novembre 2026). Usa l'ultima versione stabile compatibile e consulta la documentazione ufficiale per le API esatte (in particolare iroh, iroh-gossip, iroh-blobs, Tauri 2 e i suoi plugin). Se un'API differisce da quanto descritto qui, adatta l'implementazione mantenendo il comportamento richiesto.
6. **Non inventare requisiti.** Dove questo documento lascia una scelta aperta, applica il valore predefinito indicato in §17 e rendilo configurabile.
7. **Ogni modulo ha test.** Nessuna fase è completa senza i test descritti in §15.
8. **Nessun `unwrap()`/`expect()` nel codice di produzione**, salvo invarianti documentati. Errori tipizzati con `thiserror` nei crate di libreria, `anyhow` solo nei binari.
9. **Mantieni aggiornato `docs/DECISIONS.md`** con ogni scelta tecnica non banale presa durante la realizzazione (data, contesto, decisione, alternative scartate).

---

## 1. Visione e requisiti

### 1.1 Cosa fa l'app

Un'unica applicazione, scritta una volta, che gira su **Windows, macOS, Linux, iOS e Android** e permette ai membri di un'organizzazione di:

- registrare documenti periodici (es. rapporti mensili) che seguono flussi di raccolta, approvazione, registrazione, aggregazione e distribuzione;
- ricevere assegnazioni individuali (parti di un programma, turni) e consultarle;
- vedere programmi pubblicati in una bacheca;
- ricevere in consegna risorse (es. territori), registrarne l'uso e restituirle;
- gestire persone, qualifiche, unità (gruppi), incarichi e la configurazione stessa.

### 1.2 Vincoli fondamentali

| Vincolo | Conseguenza |
| --- | --- |
| Un solo codice per tutte le piattaforme | Tauri 2 con core Rust e frontend web |
| Comunicazione peer-to-peer via internet | iroh (QUIC, hole punching, relay di riserva) |
| Nessun dispositivo sempre acceso con i dati | I dati stanno solo sui dispositivi; il server non conserva eventi né allegati |
| Un piccolo server è ammesso | Solo relay iroh, DNS di discovery, servizio di notifiche push |
| Ruoli, assegnazioni e flussi modificabili | Tutto il dominio è configurazione versionata, non codice |
| Base astratta e riusabile | Nucleo generico + pacchetti di configurazione, uniti in fase di build |
| Dati sensibili (appartenenza religiosa, art. 9 GDPR) | Cifratura end-to-end per pubblico di lettura, database locale cifrato, blocco app |

### 1.3 Dimensioni di riferimento (primo pacchetto)

- 100–150 persone, 6 o più gruppi, 1–2 dispositivi per persona.
- ~1.500–2.500 eventi a settimana; 40–130 MB di eventi all'anno.
- Foto allegate: 2–12 GB all'anno complessivi; sui telefoni solo una parte (§9.4).

---

## 2. Principi architetturali

1. **Nessun concetto di dominio nel codice.** Il dominio vive nei pacchetti.
2. **Primitive componibili**: qualifiche, unità, incarichi, pubblici, moduli, flussi, eventi di calendario, programmi, risorse, bacheca, notifiche.
3. **Interfaccia generata dalla configurazione**: moduli da compilare, code di lavoro, programmi, elenchi di risorse e menu derivano dal pacchetto.
4. **Tutto è un evento firmato e immutabile.** Lo stato si ricostruisce rileggendo gli eventi (event sourcing). Ogni dispositivo verifica da solo validità e permessi.
5. **Riservatezza per pubblico di lettura.** Ogni contenuto è cifrato per il pubblico che il suo tipo dichiara.
6. **Replica totale degli eventi, selettiva degli allegati.** Tutti conservano e inoltrano tutti gli eventi (anche quelli che non possono leggere); gli allegati pesanti vanno solo a chi deve vederli e ai dispositivi d'archivio.
7. **Estendibilità controllata.** Ciò che le primitive non esprimono diventa una nuova primitiva generica del nucleo, mai un'eccezione per un pacchetto.
8. **Determinismo.** Dato lo stesso insieme di eventi, ogni dispositivo deve calcolare esattamente lo stesso stato (convergenza). Nessuna decisione dipende dall'orologio locale o dall'ordine di arrivo.

---

## 3. Stack tecnologico

| Ambito | Scelta | Note |
| --- | --- | --- |
| Linguaggio core | Rust stable (edition 2024 se disponibile, altrimenti 2021) | Workspace Cargo |
| Shell applicativa | Tauri 2.x | Desktop + iOS + Android |
| Frontend | SvelteKit (adapter static) + TypeScript + Vite | SPA, nessun SSR |
| Componenti UI | Libreria headless leggera (es. bits-ui) + CSS proprio | Tema chiaro/scuro, accessibile |
| Binding TS ↔ Rust | `specta` + `tauri-specta` | Tipi TypeScript generati dai DTO Rust |
| Rete P2P | `iroh` 1.x, `iroh-gossip`, `iroh-blobs` | Relay e DNS propri in produzione |
| Serializzazione su rete e su disco | `postcard` (eventi), `serde_json`/`serde_yaml` (configurazione) | |
| Hash | `blake3` | ID degli eventi e dei blob |
| Firme | Ed25519 (`ed25519-dalek`) — la chiave del dispositivo coincide con la chiave iroh | |
| Scambio chiavi | X25519 (`x25519-dalek`) + `crypto_box` (sealed box) | Chiave X25519 separata per dispositivo |
| Cifratura simmetrica | XChaCha20-Poly1305 (`chacha20poly1305`) | Chiavi dei canali |
| Derivazione chiavi | `hkdf` (SHA-256) | |
| Database locale | SQLite via `rusqlite` con SQLCipher (feature `bundled-sqlcipher`) | Cifrato a riposo |
| Segreti del dispositivo | Desktop: crate `keyring`; mobile: plugin Tauri proprio (Android Keystore, iOS Keychain) | Astrazione `SecretStore` |
| Schema configurazione | `schemars` (JSON Schema generato dai tipi Rust) | Validazione pacchetti |
| Log | `tracing` + `tracing-subscriber` | Niente dati personali nei log |
| Errori | `thiserror` (librerie), `anyhow` (binari) | |
| Async | `tokio` | |
| Push mobile | Plugin Tauri per notifiche push (es. `tauri-plugin-notifications` con feature `push-notifications`), isolato dietro un'interfaccia | Vedi §11.3 |
| Altri plugin Tauri | `barcode-scanner` (QR), `biometric`, `notification`, `deep-link`, `fs`, `dialog` | Verifica supporto mobile di ciascuno |
| Server push | Rust, `axum`, `sqlx` (SQLite) | |
| Relay e DNS | Binari ufficiali `iroh-relay` e `iroh-dns-server` | |
| Reverse proxy | Caddy | TLS automatico per le API |
| Orchestrazione server | Docker Compose | |
| Package manager JS | pnpm | |
| CI | GitHub Actions (o Gitea Actions equivalenti) | Runner macOS per iOS |

---

## 4. Struttura del repository

Un solo repository. Il **nucleo** (`crates/`, `apps/app/`) e i **pacchetti** (`packages/`) sono separati e si uniscono solo in fase di build tramite `xtask`.

```
/
├─ Cargo.toml                    # workspace Rust
├─ rust-toolchain.toml
├─ package.json                  # workspace pnpm
├─ pnpm-workspace.yaml
├─ README.md
├─ PROGETTO.md                   # questo documento
├─ docs/
│  ├─ DECISIONS.md               # registro delle decisioni
│  ├─ PACKAGE_FORMAT.md          # guida al formato dei pacchetti (generata in parte da xtask)
│  └─ DEPLOY.md                  # messa in produzione del server
├─ schemas/
│  └─ package.schema.json        # JSON Schema generato da core-model (non modificare a mano)
├─ crates/
│  ├─ core-model/                # tipi della configurazione e dello stato, espressioni, pubblici
│  ├─ core-config/               # caricamento, validazione e compilazione dei pacchetti
│  ├─ core-crypto/               # chiavi del dispositivo, canali, distribuzione e rotazione chiavi
│  ├─ core-events/               # busta degli eventi, firma, grafo causale, archivio SQLite
│  ├─ core-engine/               # proiezioni: organizzazione, permessi, flussi, moduli,
│  │                             # programmi, risorse, bacheca, notifiche, promemoria
│  ├─ core-sync/                 # endpoint iroh, gossip, riconciliazione, blob, ricevute
│  ├─ core-push/                 # client del servizio push (registrazione, richieste di risveglio)
│  ├─ core-api/                  # facciata per l'app: comandi, DTO, eventi verso l'interfaccia
│  ├─ core-testkit/              # simulatore multi-dispositivo e scenari di test
│  └─ xtask/                     # build: validazione pacchetti, bundle, codegen, branding
├─ apps/
│  └─ app/
│     ├─ src-tauri/              # binario Tauri, plugin, configurazione Tauri di base
│     │  ├─ Cargo.toml
│     │  ├─ tauri.conf.json      # configurazione neutra (nessun nome di dominio)
│     │  ├─ capabilities/
│     │  ├─ src/
│     │  └─ plugins/secret-store/ # plugin mobile per Keystore/Keychain
│     └─ ui/                     # SvelteKit: renderer generico guidato dal pacchetto
│        ├─ src/lib/renderers/   # moduli, code di lavoro, programmi, risorse, bacheca
│        ├─ src/lib/i18n/        # testi forniti dal pacchetto
│        └─ src/routes/
├─ packages/
│  └─ congregazione/             # primo pacchetto di configurazione
│     ├─ package.yaml            # manifest
│     ├─ qualifications.yaml
│     ├─ units.yaml
│     ├─ roles.yaml
│     ├─ audiences.yaml
│     ├─ forms.yaml
│     ├─ workflows.yaml
│     ├─ aggregates.yaml
│     ├─ calendar.yaml
│     ├─ programs.yaml
│     ├─ resources.yaml
│     ├─ board.yaml
│     ├─ notifications.yaml
│     ├─ ui.yaml                 # menu e home generati
│     ├─ texts/it.yaml           # etichette e testi
│     ├─ privacy/it.md           # informativa privacy
│     ├─ branding/               # icone, colori, nome app
│     └─ tests/                  # scenari di test del pacchetto (§15.4)
├─ server/
│  ├─ push-service/              # crate Rust del servizio push
│  └─ deploy/
│     ├─ docker-compose.yml
│     ├─ Caddyfile
│     ├─ iroh-relay.toml
│     ├─ iroh-dns-server.toml
│     └─ .env.example
└─ .github/workflows/
   ├─ ci.yml                     # fmt, clippy, test, validazione pacchetti, controllo dominio
   └─ release.yml                # build per piattaforma con pacchetto scelto
```

### 4.1 Come nucleo e pacchetto si uniscono in build

Il comando unico è:

```
cargo xtask build --package congregazione --target <desktop|android|ios> [--release]
```

`xtask` esegue in ordine:

1. **Valida** il pacchetto: parsing YAML, validazione contro `schemas/package.schema.json`, poi validazione semantica di `core-config` (riferimenti esistenti, espressioni ben formate, flussi raggiungibili, nessun ciclo nei requisiti, pubblici risolvibili, testi presenti per tutte le chiavi usate).
2. **Esegue i test di scenario** del pacchetto (`packages/<nome>/tests/`).
3. **Compila** il pacchetto in un *bundle* binario (`postcard`) con hash BLAKE3: `target/package/<nome>/bundle.bin` + `bundle.hash`.
4. **Genera il branding**: nome app, identificatore (es. `it.example.congregazione`), icone e colori da `branding/`; produce `target/package/<nome>/tauri.conf.overlay.json`.
5. **Copia testi e informativa** in `apps/app/ui/static/package/` (file generati, ignorati da git).
6. **Lancia la build Tauri** passando l'overlay (`tauri build --config <overlay>`; per mobile `tauri android build` / `tauri ios build`) e la variabile d'ambiente `APP_PACKAGE_BUNDLE=<percorso bundle.bin>`.

Il crate `apps/app/src-tauri` include il bundle con `include_bytes!(env!("APP_PACKAGE_BUNDLE"))` in un modulo generato da `build.rs`. Il bundle incorporato è la **configurazione iniziale**: alla creazione di una nuova organizzazione diventa la versione 1 pubblicata nell'evento di genesi. Le versioni successive viaggiano come eventi firmati (§6.6), quindi una congregazione può modificare la propria configurazione senza nuove build.

Altri comandi `xtask`:

| Comando | Effetto |
| --- | --- |
| `cargo xtask schema` | Rigenera `schemas/package.schema.json` da `core-model` |
| `cargo xtask validate --package <nome>` | Solo validazione |
| `cargo xtask test-package --package <nome>` | Solo test di scenario |
| `cargo xtask dev --package <nome>` | Avvio desktop in sviluppo con hot reload della UI |
| `cargo xtask check-domain-leak` | Fallisce se nel nucleo compaiono termini del dominio (§15.6) |
| `cargo xtask bindings` | Rigenera i tipi TypeScript da `core-api` |
| `cargo xtask simulate --package <nome> --devices 20 --days 60` | Simulazione con dati fittizi (§15.3) |

---

## 5. Modello del nucleo

Tutti i tipi stanno in `core-model`, derivano `Serialize`, `Deserialize`, `JsonSchema`, `Clone`, `Debug`. Gli identificatori di configurazione sono **slug** (`[a-z][a-z0-9_]*`), gli identificatori di istanza sono `Id` = 16 byte casuali (UUID v4 o equivalente) codificati in base32 minuscolo.

### 5.1 Entità

| Entità (codice) | Italiano | Tipo | Descrizione |
| --- | --- | --- | --- |
| `Person` | Persona | Istanza | Membro dell'organizzazione; attributi configurabili (§5.2); uno o più dispositivi |
| `ExternalPerson` | Persona esterna | Istanza | Contatto senza dispositivo, assegnabile (es. ospite) |
| `Device` | Dispositivo | Istanza | Chiave pubblica Ed25519 (= iroh `NodeId`), chiave X25519, nome, persona, stato |
| `QualificationDef` | Qualifica | Config | Nome, tipo di durata, esclusioni, implicazioni, requisiti |
| `QualificationGrant` | Qualifica concessa | Istanza | Persona + qualifica + periodo |
| `UnitTypeDef` | Tipo di unità | Config | Es. "gruppo"; appartenenza esclusiva o multipla |
| `Unit` | Unità | Istanza | Unità concreta di un tipo |
| `Membership` | Appartenenza | Istanza | Persona + unità + periodo |
| `RoleDef` | Incarico | Config | Ambito, requisiti, titolari min/max, permessi, sostituto, supervisione, gerarchia, priorità |
| `RoleGrant` | Conferimento | Istanza | Persona + incarico + ambito (organizzazione o unità) + periodo |
| `AudienceDef` | Pubblico | Config | Espressione di selettori (§5.4) |
| `Permission` | Permesso | Codice | Enum chiuso (§5.5) |
| `FormDef` | Tipo di modulo | Config | Campi, varianti, periodicità, scadenze, flusso, pubblici per stato |
| `Document` | Documento | Istanza | Modulo compilato; soggetto; periodo; stato; valori |
| `Batch` | Lotto | Istanza | Insieme di documenti di un ambito e periodo |
| `WorkflowDef` | Flusso | Config | Stati e passaggi (§5.7) |
| `AggregateDef` | Aggregazione | Config | Calcolo di un documento di sintesi |
| `CalendarEventTypeDef` | Tipo di evento | Config | Es. "adunanza infrasettimanale"; ricorrenza predefinita |
| `CalendarEvent` | Evento | Istanza | Data/ora, luogo, eccezioni |
| `ProgramTypeDef` | Tipo di programma | Config | Parti/turni, requisiti, responsabili, periodo |
| `Program` | Programma | Istanza | Per un periodo; stato; versione |
| `Assignment` | Assegnazione | Istanza | Parte/turno + persona o persona esterna + risposta |
| `ResourceTypeDef` | Tipo di risorsa | Config | Attributi, ciclo di vita, registrazioni, richieste, regole |
| `Resource` | Risorsa | Istanza | Istanza con attributi e allegati |
| `ResourceLog` | Registrazione | Istanza | Voce del detentore |
| `ResourceRequest` | Richiesta | Istanza | Richiesta al detentore con scadenza |
| `BoardPost` | Elemento di bacheca | Istanza | Programma pubblicato o avviso |
| `NotificationRuleDef` | Regola di notifica | Config | Quando, chi, testo |
| `ReminderRuleDef` | Promemoria | Config | Condizione temporale, destinatari, testo |

### 5.2 Persone e attributi

`Person` ha campi fissi (`id`, `display_name`, `created_at_event`) e **attributi configurabili** dichiarati nel pacchetto (`person_attributes`), con gli stessi tipi dei campi dei moduli (§5.6). Ogni attributo dichiara il proprio pubblico di lettura (es. telefono leggibile solo da certi incarichi). Gli attributi sono cifrati per quel pubblico.

### 5.3 Qualifiche, unità, incarichi

**`QualificationDef`**

```yaml
id: slug
label: chiave_testo
duration: permanent | free_period | fixed_period
fixed_period_unit: month | week | year   # solo per fixed_period
implies: [slug, ...]                    # avere questa implica avere anche queste
excludes: [slug, ...]                   # non compatibile con queste nello stesso periodo
requires: Cond                          # condizione sulla persona (§5.4.1)
sets: [slug, ...]                       # insiemi nominati di cui fa parte (es. full_time)
visibility: audience_id                 # chi vede che una persona ha questa qualifica
```

La verifica è **per data**: il motore risponde a `has_qualification(person, q, date)` usando lo storico delle concessioni.

**`UnitTypeDef`**

```yaml
id: slug
label: chiave_testo
membership: exclusive | multiple
member_requires: Cond                   # chi può essere membro
```

**`RoleDef`**

```yaml
id: slug
label: chiave_testo
scope: organization | unit:<unit_type_id>
requires: Cond                          # requisiti sulla persona
holders: { min: 0, max: 1 }             # per ambito; max null = illimitato
permissions:                            # permessi conferiti
  - permission: document.collect
    on: form:<form_id>                  # tipo a cui si applica (opzionale)
    target: subject_unit | own_scope | organization   # su cosa si esercita
priority: 100                           # usata nella risoluzione dei conflitti
substitute_for: role_id                 # questo incarico sostituisce role_id quando scoperto
exclusive_with: [role_id]               # non può coesistere nello stesso ambito
supervises: role_id                     # incarico di supervisione su un'unità in cui c'è role_id
assistant_of: role_id                   # gerarchia responsabile/assistenti
mandatory: Cond                         # quando l'ambito deve avere un titolare (es. sempre)
```

Regole del motore:

- un `RoleGrant` è **valido** solo se `requires` è soddisfatto per tutto il periodo; se una qualifica decade, il motore produce un avviso "conferimento non più valido" agli amministratori, **non** revoca automaticamente;
- `holders.max` e `exclusive_with` sono verificati al momento del conferimento (evento rifiutato se violati);
- un ambito con `mandatory` vero e nessun titolare (né sostituto) genera un avviso "incarico scoperto".

### 5.4 Espressioni e pubblici

#### 5.4.1 Condizioni (`Cond`)

Linguaggio dichiarativo, serializzato in YAML, **senza scripting**. Valutato rispetto a un contesto (persona valutata, soggetto del documento, data di riferimento, unità del soggetto, valori dei campi).

```yaml
# combinatori
all: [Cond, ...]
any: [Cond, ...]
not: Cond
# sulla persona valutata
has_qualification: slug                 # alla data di riferimento
in_set: slug                            # qualifica appartenente a un insieme (sets)
holds_role: { role: slug, where: anywhere | subject_unit | own_unit | unit:<id> }
member_of: subject_unit | unit:<id> | unit_type:<slug>
attribute: { name: slug, op: eq|ne|in|gt|ge|lt|le|set|unset, value: any }
is_subject: true                        # la persona è il soggetto
# sui campi del documento (solo nei contesti di modulo)
field: { name: slug, op: ..., value: any }
# costanti
always: true
```

#### 5.4.2 Pubblici (`AudienceDef`)

Un pubblico è un insieme di persone calcolato da selettori, combinabili con `union`, `intersect`, `except`.

```yaml
id: slug
label: chiave_testo
select:
  union:
    - everyone: true
    - with_qualification: slug
    - with_set: slug
    - role_holders: { role: slug, where: anywhere | subject_unit | unit:<id> }
    - supervisors_of: subject_unit
    - unit_members: subject_unit | unit:<id>
    - subject: true
    - assigned: true                    # persone assegnate (programmi, risorse)
    - explicit: true                    # elenco scelto al momento dell'azione
    - audience: other_audience_id       # riuso
```

Il motore espone `resolve(audience, context, date) -> BTreeSet<PersonId>` (ordinato, deterministico).

### 5.5 Permessi elementari (enum chiuso nel codice)

| Area | Permessi |
| --- | --- |
| Documenti | `document.fill` (compilare come soggetto), `document.fill_on_behalf`, `document.read`, `document.transition` |
| Lotti | `batch.collect`, `batch.forward`, `batch.register` |
| Aggregazioni | `aggregate.generate`, `document.distribute` |
| Programmi | `program.draft`, `program.assign`, `program.publish` (in bacheca), `assignment.respond_on_behalf` |
| Calendario | `calendar.manage` |
| Risorse | `resource.manage` (creare/modificare), `resource.assign`, `resource.log` (detentore), `resource.request` |
| Bacheca | `board.publish`, `board.withdraw` |
| Organizzazione | `people.manage`, `qualifications.manage`, `units.manage`, `roles.grant`, `devices.manage` |
| Configurazione | `config.publish` |
| Sistema | `archive.serve` (dispositivo d'archivio) |

Ogni permesso è concesso **con un oggetto** (`on`: tipo di modulo/programma/risorsa, opzionale) e **un bersaglio** (`target`). La funzione centrale del motore è:

```rust
fn is_allowed(state: &AuthState, actor: PersonId, perm: Permission,
              object: ObjectRef, context: &EvalContext) -> Result<(), Denied>
```

Gli amministratori radice (dichiarati nell'evento di genesi e modificabili solo da altri amministratori radice) hanno tutti i permessi.

### 5.6 Moduli e documenti

**Tipi di campo**

| Tipo | Opzioni |
| --- | --- |
| `integer`, `decimal` | `min`, `max`, `step`, `unit` |
| `duration` | in minuti; mostrato come ore e minuti |
| `boolean` | etichetta sì/no |
| `text`, `long_text` | `max_length` |
| `date`, `period` | |
| `choice`, `multi_choice` | `options: [{id, label}]` |
| `person`, `resource` | `filter: Cond` |
| `photos` | `max_count`, compressione (§9.4) |
| `computed` | `expr` aritmetica limitata su altri campi: `sum`, `sub`, `mul`, `div`, `if` |

**`FormDef`**

```yaml
id: slug
label: chiave_testo
subject: Cond                           # chi deve/ può avere questo documento
periodicity: none | weekly | monthly    # uno per soggetto per periodo
open_at: { offset_days: 0 }             # creazione automatica dal primo giorno del periodo successivo
due: { offset_days: 10 }                # scadenza relativa all'apertura
variants:
  - id: slug
    when: Cond                          # valutata sul soggetto alla data del periodo
    fields: [FieldDef, ...]
workflow: workflow_id
batching: { unit_type: slug }           # i documenti si raggruppano in lotti per unità
read_audience_by_state:                 # chi legge il documento in ciascuno stato
  submitted: audience_id
  registered: audience_id
fillers_on_behalf: audience_id          # chi può compilare al posto del soggetto
```

Le varianti sono valutate **in ordine**; vince la prima con `when` vero. Il documento memorizza `form_version` e `variant_id` al momento della creazione.

### 5.7 Flussi

```yaml
id: slug
states: [draft, submitted, collected, forwarded, registered, ...]
initial: draft
steps:
  - id: slug
    kind: transition | collect | forward | register | aggregate | distribute | request
    from: [state, ...]
    to: state
    by: audience_id                     # chi può eseguirlo
    rule: any | all                     # basta uno o servono tutti i membri del pubblico
    when: Cond                          # condizioni aggiuntive
    allow_incomplete_after: { offset_days: 8 }    # per forward con mancanti
    effects:
      - set_read_audience: audience_id
      - notify: { audience: audience_id, text: chiave_testo }
      - create_aggregate: aggregate_id
    note: optional | required | none    # nota testuale (es. per "rimanda")
timers:
  - at: { offset_days: 5, from: period_end }
    do: { remind: { audience: audience_id, text: chiave_testo } }
conflict: priority_then_hash            # unica politica nella v1 (§7.6)
```

Semantica dei tipi di passaggio:

| `kind` | Semantica |
| --- | --- |
| `transition` | Cambia lo stato di un documento |
| `collect` | Automatico: quando un documento entra nello stato di raccolta viene aggiunto al lotto `(form, unità, periodo)`; il lotto espone presenti e mancanti |
| `forward` | Chiude il lotto e porta tutti i suoi documenti nello stato `to`; documenti arrivati dopo diventano *integrazioni* e seguono individualmente |
| `register` | Rende definitivi i documenti di un lotto; da qui il contenuto non è più modificabile |
| `aggregate` | Calcola un documento di sintesi (§5.8) a partire dai documenti registrati del periodo |
| `distribute` | Ricifra il documento (o l'aggregato) per un nuovo pubblico (§8.5) |
| `request` | Usato dalle risorse: chiede un'azione al detentore con risposte previste |

### 5.8 Aggregazioni

```yaml
id: slug
source: { form: form_id, state: registered, period: same }
group_by: [variant | qualification_set:<slug> | unit:<unit_type>]
measures:
  - id: slug
    op: count | count_true | sum | avg | min | max
    field: field_id                     # non serve per count
    filter: Cond
include_individual_documents: true      # allega i documenti individuali (il "rapporto completo")
list_missing: true                      # elenca i soggetti senza documento registrato
read_audience: audience_id
```

L'aggregato è un documento del nucleo, versionato: un'integrazione successiva produce una nuova versione.

### 5.9 Calendario e programmi

**`CalendarEventTypeDef`**: `id`, `label`, `default_recurrence` (RRULE RFC 5545), `default_time`, `default_duration_min`, `location_required`, `scope` (organizzazione o tipo di unità).

**`ProgramTypeDef`**

```yaml
id: slug
label: chiave_testo
event_type: calendar_event_type_id | [ ... ]
period: week | month
owner_role: role_id                     # responsabile
assistant_roles: [role_id]
acceptance: true                        # l'assegnato conferma o declina
slots:
  - id: slug
    label: chiave_testo
    count: 1                            # persone per slot
    requires: Cond                      # requisiti sull'assegnato
    allow_external: false
    extra_fields: [FieldDef, ...]       # es. tema, durata, titolo
    repeat: { field: slug }             # opzionale: slot ripetuto N volte
    pair_with: slot_id                  # opzionale: slot abbinato (es. studente + interlocutore)
publish_audience: audience_id
remind_before: { days: 2 }
```

Ciclo di vita di un `Program`: `draft` → `assigned` (notifiche individuali) → `published` (in bacheca) → `revised` (nuova versione; avvisi solo alle persone toccate). Verifiche del motore in bozza: requisiti degli slot, doppie assegnazioni della stessa persona nello stesso evento (avviso, non blocco), slot vuoti.

Ogni `Assignment` ha `response: pending | accepted | declined{note}`. Un rifiuto notifica il responsabile e gli assistenti.

### 5.10 Risorse

```yaml
id: slug
label: chiave_testo
attributes: [FieldDef, ...]             # incluso photos per mappe
lifecycle: workflow_id                  # predefinito: available -> assigned -> returned -> available
holder_requires: Cond
manager_role: role_id
assistant_roles: [role_id]
log_fields: [FieldDef, ...]             # campi delle registrazioni del detentore
requests:
  - id: slug
    label: chiave_testo
    answers: [answer_id, ...]           # es. logged, returned, not_yet
    due: { days: 14 }
rules:
  max_per_holder: 3
  remind_holder_after_no_log: { days: 120 }
  alert_manager_not_worked: { days: 365 }
read_audience:
  list: audience_id
  history: audience_id
  attachments_holder: true              # il detentore vede gli allegati durante la detenzione
```

### 5.11 Bacheca, notifiche, promemoria

- **Bacheca** (`board.yaml`): pubblico predefinito, durata di visibilità predefinita, tipi di avviso liberi ammessi.
- **Notifiche** (`notifications.yaml`): legano un *trigger* del nucleo (`document.state_changed`, `batch.forwarded`, `aggregate.distributed`, `assignment.created`, `assignment.declined`, `program.published`, `resource.assigned`, `resource.request_opened`, `qualification.expiring`, `role.uncovered`, `grant.invalid`) a un pubblico e a un testo.
- **Promemoria** (`reminders` in `notifications.yaml`): condizioni temporali (`document.missing_after`, `batch.incomplete_after`, `event.assignment_before`, `resource.no_log_after`, `request.due_after`).

Notifiche e promemoria si **calcolano sul dispositivo** dalla proiezione locale: funzionano offline e non richiedono server. La push serve solo a risvegliare l'app (§11).

---

## 6. Formato dei pacchetti di configurazione

### 6.1 Manifest `package.yaml`

```yaml
id: congregazione
version: 1.0.0                          # semver del pacchetto sorgente
core_compat: ">=1.0, <2.0"              # versioni del nucleo compatibili
default_locale: it
locales: [it]
branding:
  app_name: chiave_testo
  identifier: it.example.congregazione  # da personalizzare prima della pubblicazione sugli store
  icon: branding/icon.png               # 1024x1024
  primary_color: "#3B5BDB"
files:                                  # tutti obbligatori, anche se vuoti
  - qualifications.yaml
  - units.yaml
  - roles.yaml
  - audiences.yaml
  - forms.yaml
  - workflows.yaml
  - aggregates.yaml
  - calendar.yaml
  - programs.yaml
  - resources.yaml
  - board.yaml
  - notifications.yaml
  - ui.yaml
person_attributes:
  - { id: phone, type: text, read_audience: contacts_readers }
privacy_notice: privacy/it.md
domain_terms: [anzian, elder, servitor, servant, pionier, pioneer, proclamator, publisher,
               congregaz, congregation, territor, uscier, attendant, rapporto, service_report,
               adunanz, meeting, sorvegliant, overseer, segretari, secretary,
               "torre di guardia", watchtower]   # usati da check-domain-leak (§15.6)
```

### 6.2 Testi

Tutte le etichette sono chiavi risolte in `texts/<locale>.yaml`. Testi con segnaposto in stile ICU MessageFormat (`{count, plural, one {...} other {...}}`). La validazione fallisce se una chiave usata manca.

### 6.3 Interfaccia (`ui.yaml`)

Descrive menu e home componendo **viste generiche del nucleo** (§12.2):

```yaml
home:
  - view: my_documents_due
  - view: my_assignments
  - view: my_resources
  - view: board
menu:
  - { label: menu.reports, view: work_queue, params: { workflow: service_report } , visible_to: audience_id }
  - { label: menu.programs, view: program_list, params: { types: [midweek, weekend] } }
```

### 6.4 Validazione semantica (in `core-config`)

Il pacchetto è rifiutato se:

- un riferimento (id di qualifica, incarico, pubblico, modulo, flusso, testo) non esiste;
- un flusso ha stati irraggiungibili o nessun cammino dallo stato iniziale a uno stato finale;
- le implicazioni/esclusioni di qualifiche formano contraddizioni (A implica B e A esclude B) o cicli;
- un incarico richiede qualifiche mai ottenibili;
- un pubblico è definito ricorsivamente;
- un campo `computed` ha riferimenti circolari;
- un'aggregazione usa campi non presenti in nessuna variante;
- un passaggio di flusso ha `by` vuoto o non risolvibile;
- mancano testi per `default_locale`.

L'errore deve indicare file, percorso YAML e messaggio comprensibile.

### 6.5 Compilazione

`core-config` produce `CompiledConfig`: tutti i riferimenti risolti in indici interni, espressioni pre-analizzate, testi indicizzati. Serializzato in `postcard`, hash BLAKE3. È ciò che il motore usa a runtime.

### 6.6 Versioni a runtime e migrazioni

- La configurazione attiva di un'organizzazione è l'ultima versione pubblicata con l'evento `config.published` (contiene il `CompiledConfig` completo, firmato da chi ha `config.publish`).
- **Documenti, lotti, programmi e risorse ricordano la versione** con cui sono nati e continuano a usarla fino alla chiusura.
- Una nuova versione che **rinomina o elimina** un elemento in uso deve includere una sezione `migrations` (`rename: {from, to}`, `retire: {id, until_closed: true}`); altrimenti `config.published` è rifiutato in validazione.
- Un aggiornamento dell'app con un bundle più recente **non** sovrascrive la configurazione di un'organizzazione esistente: propone agli amministratori di pubblicarlo come nuova versione, mostrando le differenze.
- L'editor di configurazione su desktop (§12.4) produce gli stessi file YAML e passa per la stessa validazione.

---

## 7. Eventi firmati

### 7.1 Busta dell'evento

```rust
struct SignedEvent {
    id: EventHash,              // blake3(postcard(EventBody))
    body: EventBody,
    signature: Ed25519Signature // firma di id con la chiave del dispositivo autore
}

struct EventBody {
    org: OrgId,                 // identificatore dell'organizzazione (hash della genesi)
    author: DevicePublicKey,    // = iroh NodeId
    parents: Vec<EventHash>,    // teste del grafo note all'autore (1..=16), ordinate
    depth: u64,                 // 1 + max(depth dei genitori); genesi = 0
    declared_time: i64,         // ms Unix, solo informativo
    kind: EventKind,            // tipo (enum del nucleo, §7.3)
    channel: ChannelRef,        // canale di cifratura del payload (o Public per eventi in chiaro)
    payload: Vec<u8>,           // postcard(payload) cifrato con la chiave del canale
    attachments: Vec<BlobRef>,  // hash dei blob cifrati (§9.4)
    config_version: u32,        // versione di configurazione usata dall'autore
}
```

- **Eventi in chiaro** (`channel = Public`): solo metadati organizzativi indispensabili alla verifica dei permessi e alla sincronizzazione (dispositivi, distribuzione di chiavi, configurazione). Tutto ciò che riguarda persone e contenuti è cifrato.
- I dati necessari a verificare i permessi **senza poter leggere il contenuto** stanno in campi in chiaro minimi del payload esterno (es. tipo di oggetto e id dell'oggetto, mai valori dei campi). Regola: un dispositivo che non appartiene al pubblico deve poter **inoltrare** l'evento, non necessariamente **validarlo**; la validazione completa la fanno i dispositivi che possono decifrarlo. Gli eventi organizzativi (incarichi, qualifiche) sono invece leggibili e verificabili da tutti, perché servono a calcolare i permessi; le qualifiche hanno comunque un pubblico di visibilità per l'interfaccia (§5.3), ma il loro evento è verificabile da tutti.

> Nota di progetto: le qualifiche sensibili (es. stati spirituali) sono necessarie a tutti i dispositivi per verificare i permessi. Il compromesso accettato è che i **dispositivi** conoscono la mappa qualifiche/incarichi; l'interfaccia la mostra solo al pubblico configurato. Registralo in `docs/DECISIONS.md`.

### 7.2 Genesi

Il primo evento (`org.genesis`) contiene: nome dell'organizzazione, chiavi dei dispositivi degli **amministratori radice** (almeno 2 consigliati; ammesso 1 con avviso), `CompiledConfig` versione 1, URL del relay e del DNS, parametri di sicurezza. `OrgId = blake3(genesis)`. Non ha genitori.

### 7.3 Tipi di evento (enum `EventKind` del nucleo)

| Gruppo | Tipi |
| --- | --- |
| Organizzazione | `org.genesis`, `org.settings_changed`, `org.backup_node_set`, `config.published` |
| Dispositivi | `device.invited`, `device.added`, `device.revoked`, `device.archive_set` |
| Persone | `person.added`, `person.updated`, `person.removed`, `external_person.upserted` |
| Qualifiche | `qualification.granted`, `qualification.ended` |
| Unità | `unit.created`, `unit.updated`, `unit.closed`, `membership.started`, `membership.ended` |
| Incarichi | `role.granted`, `role.ended` |
| Chiavi | `channel.keys_distributed`, `channel.rotated` |
| Documenti | `document.created`, `document.updated`, `document.step` (passaggio di flusso, con nota), `document.distributed` |
| Lotti | `batch.forwarded`, `batch.registered` (la raccolta è derivata, non un evento) |
| Aggregati | `aggregate.generated` |
| Calendario | `calendar.event_upserted`, `calendar.event_cancelled` |
| Programmi | `program.drafted`, `program.updated`, `program.assignments_sent`, `assignment.responded`, `program.published`, `program.revised` |
| Risorse | `resource.upserted`, `resource.assigned`, `resource.logged`, `resource.request_opened`, `resource.request_answered`, `resource.returned`, `resource.retired` |
| Bacheca | `board.posted`, `board.withdrawn` |
| Privacy | `person.data_erasure_requested`, `retention.applied` |

### 7.4 Archivio locale

SQLite cifrato (SQLCipher). Tabelle minime:

| Tabella | Contenuto |
| --- | --- |
| `events` | `id` PK, `org`, `author`, `depth`, `kind`, `channel`, `raw` (blob firmato), `received_at`, `status` (`valid`, `pending_key`, `pending_parents`, `invalid`, `quarantined`) |
| `event_parents` | `child`, `parent` |
| `heads` | teste correnti del grafo |
| `ancestry_index` | indice di raggiungibilità per verifiche causali (es. intervalli o bitmap per profondità; ottimizzazione libera) |
| `channel_keys` | `channel`, `epoch`, chiave cifrata con la chiave locale |
| `blobs` | `hash`, `size`, `state` (`present`, `wanted`, `evicted`), `pinned_reason` |
| `receipts` | `event_id`, `device`, `received_at` (ricevute di consegna, non eventi) |
| `projection_*` | tabelle derivate per le viste (ricostruibili) |
| `outbox` | eventi locali non ancora confermati da 2 dispositivi |

Le proiezioni sono **ricostruibili** da zero dagli eventi: `core-engine` deve offrire `rebuild_all()` ed è usato nei test di convergenza.

### 7.5 Ordine deterministico e proiezioni

- Ordine totale per l'applicazione degli eventi: **ordinamento topologico** del grafo, a parità per `depth` crescente e poi per `id` (byte) crescente.
- L'arrivo di un evento "vecchio" (profondità bassa) fa ricalcolare le proiezioni dal punto di inserimento. Implementazione accettabile nella v1: ricalcolo incrementale per oggetto interessato; fallback a ricalcolo completo (volumi piccoli, §1.3).
- Eventi con genitori mancanti restano in `pending_parents` e si richiedono ai peer.

### 7.6 Validità e conflitti

**Validità di un evento E** (verificata da ogni dispositivo che può leggerlo):

1. firma valida e `author` è un dispositivo **aggiunto e non revocato** nel passato causale di E;
2. il dispositivo appartiene a una persona che, secondo lo stato di autorizzazione calcolato **sul passato causale di E**, ha il permesso richiesto per il tipo di evento e l'oggetto;
3. per i passaggi di flusso: lo stato di partenza è corretto secondo la versione di flusso dell'oggetto;
4. vincoli di configurazione (titolari massimi, requisiti, esclusioni) rispettati.

Eventi non validi: conservati con `status = invalid`, ignorati dalle proiezioni, segnalati agli amministratori (senza contenuto).

**Stato di autorizzazione sul passato causale**: gli eventi che lo modificano (`device.*`, `person.*`, `qualification.*`, `membership.*`, `role.*`, `config.published`, `org.*`) sono pochi; il motore ricostruisce lo stato considerando solo quelli antenati di E, con cache per insieme di teste.

**Conflitti**

| Caso | Regola |
| --- | --- |
| Due passaggi concorrenti sullo stesso oggetto dallo stesso stato | Vince quello dell'autore con `priority` di incarico più alta; a parità, `id` minore. L'altro diventa `superseded` e il suo autore riceve un avviso |
| Azione concorrente a una revoca (dispositivo, incarico, qualifica) | Valida se la revoca non è nel suo passato causale. **Ma**: se un evento arriva da un autore già revocato localmente, con passato causale che non include la revoca e `declared_time` successivo alla revoca, va in `quarantined` e richiede conferma di un amministratore (`admin.quarantine_resolved`) |
| Due `config.published` concorrenti | Nessuno attivo finché un amministratore radice non pubblica una versione che li ha entrambi come antenati; l'interfaccia mostra il conflitto |
| Due conferimenti concorrenti che violano `holders.max` | Vince l'evento con `id` minore; l'altro `superseded`, avviso agli amministratori |
| Modifiche concorrenti dello stesso documento in bozza (stessa persona, due dispositivi) | Ultimo per ordine deterministico, campo per campo |

### 7.7 Dimensioni

Eventi tipici 0,5–1 KB. Limite rigido: 64 KB per evento (escluse le miniature, max 20 KB ciascuna, incluse nel payload). Allegati sempre come blob.

---

## 8. Cifratura e chiavi

### 8.1 Chiavi del dispositivo

- **Ed25519** per firma e identità iroh (`SecretKey` di iroh).
- **X25519** separata per ricevere chiavi dei canali; la chiave pubblica X25519 è pubblicata in `device.added`, firmata.
- Entrambe nel `SecretStore`. Il database SQLCipher usa una chiave casuale a 256 bit anch'essa nel `SecretStore`.

### 8.2 Canali

Un **canale** cifra i payload destinati a un pubblico.

```rust
struct ChannelRef { id: ChannelId, epoch: u32 }
// ChannelId = blake3("channel" || org || descrittore canonico)
```

Descrittori:

| Descrittore | Uso |
| --- | --- |
| `Audience { audience_id, scope }` | Pubblico di configurazione in un ambito (es. organizzazione, unità X) |
| `Subject { audience_id, subject: PersonId, scope }` | Documenti personali: pubblico di lettura + soggetto, separato per persona |
| `Program { program_type, stage }` | Bozze di programma: responsabile e assistenti |
| `Assignment { person }` | Assegnazione individuale notificata: responsabili + assegnato |
| `Resource { resource_type, holder? }` | Risorse e allegati |
| `Admin` | Amministratori |

Chiave simmetrica XChaCha20-Poly1305 per ogni `(ChannelId, epoch)`, generata casualmente. Nonce casuale a 24 byte per ogni payload. AAD = `org || channel || epoch || kind`.

### 8.3 Distribuzione delle chiavi

- Chi crea un canale (o deve aggiungervi membri) pubblica `channel.keys_distributed` con, per ogni **dispositivo** dei membri, la chiave del canale cifrata con `crypto_box` sealed box verso la sua chiave X25519.
- Può distribuire chiavi solo chi è membro del canale **e** ha un permesso che giustifica l'accesso (es. `roles.grant`, il responsabile di un programma, il soggetto per il proprio canale personale).
- Quando un nuovo dispositivo viene aggiunto a una persona, **qualsiasi** dispositivo che possiede la chiave e vede la persona nel pubblico la ridistribuisce (compito automatico, idempotente: chi la vede già distribuita non ripete).
- Il calcolo dei membri attesi di un canale è deterministico (pubblico risolto alla data corrente). Ogni dispositivo confronta membri attesi e destinatari effettivi e completa le mancanze.

### 8.4 Rotazione

Si crea una nuova `epoch` quando un membro **esce** dal pubblico di un canale (fine incarico, fine qualifica, cambio unità, revoca dispositivo). La rotazione la esegue il primo dispositivo autorizzato che rileva l'uscita; rotazioni concorrenti si risolvono tenendo l'epoch con `id` evento minore (l'altra viene ignorata e i payload già cifrati con essa restano leggibili perché anche quella chiave è stata distribuita).

Chi esce mantiene l'accesso a ciò che aveva già ricevuto (accettato e documentato).

### 8.5 Distribuzione di un documento a un nuovo pubblico

Il passaggio `distribute` (o il cambio di pubblico per stato) pubblica `document.distributed` con il **contenuto ricifrato** sul canale del nuovo pubblico. Il contenuto non cambia; l'evento referenzia il documento originale. Per i lotti si produce un solo evento per lotto (contenuto = elenco di documenti ricifrati).

### 8.6 Protezioni locali

- Blocco app con PIN e/o biometria (`biometric` plugin); blocco automatico dopo inattività (predefinito 5 minuti, configurabile dall'utente tra 1 e 30).
- Schermate sensibili escluse da screenshot/anteprime su mobile (FLAG_SECURE su Android, oscuramento in app switcher su iOS).
- Cancellazione dei dati locali dopo N tentativi di PIN falliti (predefinito: disattivata; configurabile dall'organizzazione).

---

## 9. Sincronizzazione e allegati

### 9.1 Endpoint iroh

- Un `Endpoint` per dispositivo con la chiave Ed25519.
- Relay e DNS di discovery configurati dall'evento di genesi (in sviluppo: relay pubblici di n0).
- Discovery locale mDNS attiva (stessa rete).
- Protocolli (ALPN):

| ALPN | Uso |
| --- | --- |
| `org-app/sync/1` | Riconciliazione degli eventi e ricevute |
| `org-app/pair/1` | Abbinamento di un nuovo dispositivo |
| `iroh-blobs` | Trasferimento allegati (ALPN della libreria) |
| `iroh-gossip` | Annunci (ALPN della libreria) |

### 9.2 Diffusione e riconciliazione

- **Gossip**: un argomento per organizzazione, `topic = blake3("gossip" || org)`. Ogni nuovo evento locale viene annunciato con `(id, depth)`; chi non lo ha lo richiede via `sync`.
- **Riconciliazione** (`org-app/sync/1`), alla connessione tra due dispositivi della stessa organizzazione:
  1. scambio delle teste e di un riassunto per fasce di profondità (hash XOR degli id per fascia);
  2. per le fasce diverse, scambio degli id;
  3. invio degli eventi mancanti in ordine di profondità, a lotti (max 256 eventi o 1 MB per messaggio);
  4. invio delle **ricevute**: per ogni evento ricevuto da un dispositivo il ricevente risponde con una ricevuta firmata; l'autore la conserva per lo stato "inviato" (§9.3).
- **Chi contattare**: i dispositivi noti dagli eventi (`device.added`). Priorità: dispositivi d'archivio, dispositivi delle persone nei pubblici dei propri eventi in `outbox`, poi altri a caso. Massimo 8 connessioni simultanee su mobile, 32 su desktop.
- **Quando**: avvio dell'app, ritorno in primo piano, arrivo di una push, ogni 60 s in primo piano, aggiornamento in background del sistema operativo (§11.4), continuamente sul desktop nella system tray.

### 9.3 Stato di consegna

Un evento locale è:

| Stato | Condizione |
| --- | --- |
| `pending` | Nessuna ricevuta |
| `stored` | Ricevuta del nodo di backup cieco (§10.3): il dato è al sicuro sul server, nessun'altra persona lo ha ancora |
| `sent` | Ricevute da almeno **2** dispositivi diversi da quelli della stessa persona |
| `delivered:<pubblico>` | Ricevuta da almeno un dispositivo di ogni persona del pubblico di destinazione rilevante (es. sorveglianza del gruppo) |

L'interfaccia mostra lo stato (§12.3). L'`outbox` mantiene gli eventi fino a `sent`. La ricevuta del nodo di backup cieco non conta per `sent` (non è il dispositivo di una persona).

### 9.4 Allegati (foto)

**Preparazione sul dispositivo** (in Rust, crate `image` o equivalente):

- ridimensionamento a 2048 px sul lato lungo, JPEG qualità 80 (WebP se supportato in modo affidabile su tutte le piattaforme);
- rimozione EXIF (incluso GPS) salvo campo configurato diversamente;
- miniatura 256 px, ~15–20 KB, incorporata nel payload cifrato dell'evento;
- l'immagine completa è cifrata con la chiave del canale del documento e salvata come blob `iroh-blobs` (hash del contenuto cifrato).

**Replica**:

- attiva verso: pubblico di lettura del documento e dispositivi d'archivio;
- su richiesta: chiunque nel pubblico apra la foto la scarica da qualunque peer che la possiede;
- il download automatico su rete mobile è disattivabile (predefinito: solo Wi-Fi per foto non proprie).

**Conservazione predefinita** (configurabile nel pacchetto):

| Dispositivo | Allegati conservati |
| --- | --- |
| Autore | fino a registrazione + 90 giorni |
| Membri del pubblico di lettura | 90 giorni dalla ricezione, poi solo miniatura |
| Dispositivi d'archivio | tutti, a tempo indeterminato |
| Altri | nessuno (solo miniature dentro gli eventi che non possono comunque leggere) |

Un allegato risulta inviato solo quando almeno 2 altri dispositivi ne confermano la ricezione completa.

### 9.5 Dispositivi d'archivio

Desktop indicati con `device.archive_set`: facoltativi da quando c'è il nodo di backup cieco (§10.3), restano utili come copia in chiaro dell'organizzazione. Conservano tutti gli eventi e tutti gli allegati che possono leggere, e **pinnano** comunque tutti i blob (anche non leggibili) per ridondanza. Backup cifrato esportabile (file `.orgbackup` cifrato con passphrase, Argon2id + XChaCha20-Poly1305). Non hanno permessi aggiuntivi.

### 9.6 Abbinamento di un nuovo dispositivo

1. Un amministratore (o la persona stessa da un dispositivo già abbinato, per i propri dispositivi) crea un **invito**: `device.invited { invite_id, person_id, expires_at }` e un **ticket** offline contenente `org`, indirizzo iroh dell'invitante, relay URL, `invite_id`, segreto monouso (32 byte), scadenza (predefinito 15 minuti).
2. Il ticket è mostrato come QR o stringa (`orgapp:` deep link).
3. Il nuovo dispositivo genera le sue chiavi, si connette con ALPN `org-app/pair/1`, presenta il segreto e le chiavi pubbliche.
4. Entrambi mostrano un **codice di verifica** di 6 cifre derivato da `blake3(segreto || chiavi)`; l'invitante conferma.
5. L'invitante pubblica `device.added` e distribuisce le chiavi dei canali pertinenti; il nuovo dispositivo riceve tutti gli eventi.

**Creazione di un'organizzazione**: primo avvio → "Crea organizzazione" → nome, primo amministratore (sé stesso) → genesi con la configurazione del bundle. Il secondo amministratore radice si aggiunge con un `org.settings_changed` firmato dal primo.

---

## 10. Server di supporto

Il server **non legge** eventi né allegati: li conserva solo cifrati, nel nodo di backup cieco (§10.3). Una sola VPS (2 vCPU, 4 GB RAM, 40 GB SSD, traffico incluso ampio; ~5–10 €/mese) esegue:

| Servizio | Immagine/binario | Porte | Note |
| --- | --- | --- | --- |
| `iroh-relay` | binario ufficiale della release iroh | TCP 80, 443; UDP 7842 | Certificato Let's Encrypt (`cert_mode = "LetsEncrypt"`), hostname `relay.<dominio>`; accesso limitato (token o allowlist) in produzione; metriche su 9090 solo interne |
| `iroh-dns-server` | binario ufficiale | UDP/TCP 53; HTTPS per pkarr | Sottodominio delegato `dns.<dominio>`; facoltativo nella prima messa in produzione |
| `push-service` | `server/push-service` | dietro Caddy | API sotto `api.<dominio>` |
| `blind-node` | `server/blind-node` | UDP/QUIC via relay | Backup cieco degli eventi e degli allegati (§10.3) |
| `caddy` | immagine ufficiale | 443 su IP/hostname diverso dal relay, oppure relay su porte alternative | TLS per `api.<dominio>` |

Se relay e Caddy condividono l'IP, il relay usa `relay.<dominio>` con porte dedicate (es. 8443) configurate nel relay map dei client; documentalo in `docs/DEPLOY.md`.

### 10.1 Servizio push: API

Tutte le richieste sono firmate dal dispositivo: header `X-Device` (chiave pubblica), `X-Timestamp` (ms, tolleranza ±5 minuti), `X-Signature` = Ed25519 su `method || path || timestamp || blake3(body)`.

| Metodo e percorso | Corpo | Effetto |
| --- | --- | --- |
| `POST /v1/register` | `{ org, platform: "fcm"|"apns", token, app_id }` | Registra o aggiorna il token del dispositivo |
| `DELETE /v1/register` | — | Rimuove il token |
| `POST /v1/org/membership` | `{ org, devices: [pubkey], proof: [event ids] }` | Aggiorna l'elenco dei dispositivi autorizzati dell'organizzazione; accettato solo se firmato da un dispositivo già noto come amministratore (bootstrapping: il dispositivo che registra per primo un `org` ne diventa amministratore per il servizio push; gli altri amministratori sono aggiunti da lui) |
| `POST /v1/wake` | `{ org, targets: [pubkey], reason: "sync" }` | Invia una push di risveglio ai target che appartengono alla stessa `org` del mittente |

- **Contenuto della push**: nessun dato. Android: messaggio *data* FCM ad alta priorità `{ "t": "sync" }`; iOS: notifica APNs **visibile** con testo generico configurato dall'organizzazione (es. "Ci sono aggiornamenti") e `content-available: 1`, perché le push silenziose iOS sono limitate.
- **Limiti**: max 60 `wake` per dispositivo mittente all'ora, max 10 per destinatario all'ora (coalescenza: più richieste in 2 minuti → una sola push).
- **Archivio**: SQLite con `devices(pubkey, org, platform, token, updated_at)`, `orgs(id, admins)`. Backup giornaliero.
- **Credenziali**: service account FCM (HTTP v1) e chiave APNs `.p8` in variabili d'ambiente/segreti Docker.
- **Log**: nessun contenuto, nessun IP conservato oltre 7 giorni.

### 10.2 Deploy

`server/deploy/docker-compose.yml` con i quattro servizi, volumi per certificati e SQLite, `restart: unless-stopped`. `docs/DEPLOY.md` descrive: acquisto dominio, record DNS (A/AAAA per `relay`, `api`; NS per `dns`), firewall (porte sopra), primo avvio, rinnovo certificati (automatico), aggiornamenti, monitoraggio (metriche relay su Prometheus interno o semplice controllo di salute), backup.

### 10.3 Nodo di backup cieco

Un nodo sempre acceso, senza chiavi di canale, che conserva **per sempre** tutti gli eventi e tutti gli allegati dell'organizzazione così come viaggiano (cifrati) e li consegna a chi si collega dopo. Risolve il caso in cui i telefoni non sono mai accesi nello stesso momento e fa da backup cifrato.

- **Identità**: chiave Ed25519 propria (= id dell'endpoint iroh), conservata sul server. Un amministratore radice la dichiara con l'evento pubblico `org.backup_node_set { node: Option<pubkey> }` (`None` lo toglie). I dispositivi lo includono tra i peer ammessi e lo contattano per primo (§9.2), prima dei dispositivi d'archivio.
- **Cosa conserva**: ogni evento verificato (id, firma, struttura, organizzazione: tutto in chiaro, §7.1) e ogni allegato elencato in `attachments`. Non decifra nulla. Gli allegati di **tutti** gli oggetti (anche i campi foto dei documenti) devono comparire in `attachments`.
- **Chi può collegarsi**: solo i dispositivi autorizzati dell'organizzazione. L'elenco esatto richiede lo stato cifrato, quindi è quello pubblicato dagli amministratori radice al servizio push (`POST /v1/org/membership`); il nodo lo legge dal servizio push. Finché l'elenco non c'è, valgono i dispositivi radice della genesi.
- **Ricevute**: firma le ricevute con la propria chiave (non servono chiavi di canale). Danno lo stato `stored` (§9.3), non `sent`.
- **Registrazione dell'organizzazione**: come per il servizio push, con l'evento di genesi inviato da un dispositivo radice; il nodo tiene un archivio per organizzazione.
- **Cancellazione e conservazione** (§14): il nodo applica gli eventi pubblici `person.data_erasure_requested` e `retention.applied` agli oggetti indicati (intestazioni in chiaro): sostituisce il contenuto con il guscio e cancella gli allegati relativi, come i dispositivi.
- **Limiti**: quota di spazio per organizzazione (⚙ 5 GB), allegati come §9.4, connessioni come il relay.
- **Cosa vede il gestore del server**: chiavi pubbliche dei dispositivi, tipi di evento e di oggetto, date, dimensioni, grafo degli eventi. Mai contenuti, nomi o valori.
- **Archivio**: SQLCipher come sui dispositivi, chiave nei segreti del server; allegati su disco; backup giornaliero del volume.


---

## 11. Notifiche push e ciclo di vita su mobile

### 11.1 Notifiche locali

Generate da `core-engine` dopo ogni sincronizzazione confrontando la proiezione con lo stato "già notificato" (tabella locale). Testi presi dal pacchetto e resi nella lingua del dispositivo. Mostrate con il plugin `notification` di Tauri. Ogni regola ha una categoria; l'utente può silenziare le categorie non marcate `mandatory` nel pacchetto.

### 11.2 Quando chiedere un risveglio

Dopo aver creato eventi locali, `core-sync` tenta la consegna diretta per 10 secondi; per ogni **persona destinataria rilevante** (pubblico della notifica associata all'evento) i cui dispositivi non hanno inviato ricevuta, chiede `POST /v1/wake` per quei dispositivi. Coalescenza lato client: max una richiesta per destinatario ogni 2 minuti.

### 11.3 Plugin push

Interfaccia Rust `PushProvider { fn token() -> Option<PushToken>; fn on_token_refresh(cb); fn on_push(cb) }` con implementazione sopra il plugin scelto (es. `tauri-plugin-notifications` con `push-notifications`). Su desktop: implementazione vuota (il desktop resta connesso in tray). Isolare il plugin dietro questa interfaccia: se il plugin della community non è mantenuto, sostituirlo con un plugin proprio (Kotlin `FirebaseMessagingService`, Swift `UNUserNotificationCenter` + `registerForRemoteNotifications`).

### 11.4 Background

- **Android**: `WorkManager` periodico (minimo 15 minuti, a discrezione del sistema) che avvia una sincronizzazione breve (max 30 s); push FCM *data* ad alta priorità avvia la stessa sincronizzazione.
- **iOS**: `BGAppRefreshTask` (frequenza decisa dal sistema); la push visibile porta l'utente ad aprire l'app, che sincronizza in primo piano.
- **Desktop**: avvio con il sistema (opzionale, predefinito attivo per i dispositivi d'archivio), icona in system tray, sincronizzazione continua.

Il codice nativo necessario va in un plugin Tauri del repository (`apps/app/src-tauri/plugins/background-sync/`).

---

## 12. Interfaccia

### 12.1 Principi

- Lingua del pacchetto (italiano); testo scalabile; contrasto AA; uso con una mano su telefono; layout a due colonne su desktop.
- **Nessuna schermata di dominio scritta a mano.** Tutto deriva dalle viste generiche e dalla configurazione.
- Ogni azione che produce un evento mostra il suo stato di consegna.
- Funziona interamente offline.

### 12.2 Viste generiche del nucleo

| Vista (`view`) | Contenuto |
| --- | --- |
| `onboarding` | Crea organizzazione / unisciti con QR o codice / verifica codice a 6 cifre / blocco app |
| `home` | Composta da `ui.yaml`: documenti da compilare, assegnazioni, risorse, bacheca |
| `form` | Renderer di un modulo: campi per variante, validazione, foto con fotocamera e galleria, salvataggio bozza, invio |
| `my_documents` | Documenti del soggetto per periodo, con stato e stato di consegna |
| `work_queue` | Per un flusso: documenti o lotti su cui l'utente può eseguire passaggi; azioni rapide; filtro per unità e periodo |
| `batch` | Lotto: presenti, mancanti (con pulsante "sollecita" che crea un promemoria), integrazioni, inoltro |
| `aggregate` | Documento di sintesi: misure per gruppo, mancanti, documenti individuali; esportazione PDF e CSV |
| `program_list` / `program_editor` | Elenco programmi per tipo e periodo; editor a griglia eventi × slot con selettore persone filtrato dai requisiti, storico per equità, avvisi |
| `my_assignments` | Le mie parti e turni con accetta/declina e promemoria |
| `board` | Programmi pubblicati e avvisi, differenze tra versioni |
| `resource_list` / `resource_detail` | Elenco con stato, filtri; dettaglio con allegati, storico, registrazioni, richieste |
| `my_resources` | Risorse detenute, richieste aperte, registra/restituisci |
| `people` | Persone, attributi, qualifiche con periodi, appartenenze, incarichi, dispositivi |
| `units` | Unità per tipo, membri, incarichi, avvisi di copertura |
| `roles` | Conferimenti per incarico, avvisi di requisiti decaduti |
| `sync_status` | Ultimo contatto, eventi in uscita, dispositivi visti di recente, stato relay |
| `admin_alerts` | Eventi non validi, in quarantena, conflitti di configurazione, incarichi scoperti |
| `config_editor` | Solo desktop (§12.4) |
| `settings` | Lingua, notifiche, blocco app, download su rete mobile, esportazione dei propri dati |

### 12.3 Stato di consegna visibile

Icone e testi: "In attesa di sincronizzazione", "Inviato", "Ricevuto da <pubblico>", poi lo stato del flusso. Indicatore globale nella barra superiore con il numero di eventi in uscita.

### 12.4 Editor di configurazione (desktop)

- Modifica guidata di: incarichi e loro requisiti, pubblici, campi dei moduli, slot dei programmi, testi, promemoria.
- Vista testuale YAML con validazione in tempo reale (stessa di `core-config`).
- **Simulazione**: applica la nuova versione a una copia della proiezione e mostra cosa cambia (permessi, pubblici, documenti aperti) prima della pubblicazione.
- Differenze tra versioni e pubblicazione con `config.published`.

---

## 13. Pacchetto "Congregazione"

Configurazione completa del primo pacchetto. I file YAML sotto sono **normativi**: l'agente li deve creare in `packages/congregazione/` adattandoli solo alla sintassi finale dello schema. Le scelte marcate ⚙ sono valori predefiniti di questioni aperte (§17), modificabili.

### 13.1 Attributi persona

```yaml
person_attributes:
  - { id: sex, type: choice, options: [m, f], read_audience: admins_and_elders }   # serve per i requisiti di alcuni incarichi
  - { id: phone, type: text, read_audience: contacts_readers }
  - { id: email, type: text, read_audience: contacts_readers }
  - { id: uses_app, type: boolean, read_audience: elders }                         # false = un incaricato compila per conto
```

### 13.2 Qualifiche (`qualifications.yaml`)

```yaml
sets: [full_time]
qualifications:
  - { id: publisher, label: q.publisher, duration: permanent, visibility: congregation }
  - { id: baptized, label: q.baptized, duration: permanent, implies: [publisher], visibility: elders }
  - id: elder
    label: q.elder
    duration: permanent
    requires: { all: [ { has_qualification: baptized }, { attribute: { name: sex, op: eq, value: m } } ] }
    excludes: [ministerial_servant]
    visibility: congregation            # ⚙
  - id: ministerial_servant
    label: q.ministerial_servant
    duration: permanent
    requires: { all: [ { has_qualification: baptized }, { attribute: { name: sex, op: eq, value: m } } ] }
    excludes: [elder]
    visibility: congregation            # ⚙
  - { id: auxiliary_pioneer, label: q.auxiliary_pioneer, duration: fixed_period, fixed_period_unit: month,
      requires: { has_qualification: baptized }, sets: [full_time],
      excludes: [regular_pioneer, special_pioneer, field_missionary], visibility: elders }
  - { id: regular_pioneer, label: q.regular_pioneer, duration: permanent,
      requires: { has_qualification: baptized }, sets: [full_time],
      excludes: [auxiliary_pioneer, special_pioneer, field_missionary], visibility: congregation }   # ⚙
  - { id: special_pioneer, label: q.special_pioneer, duration: permanent,
      requires: { has_qualification: baptized }, sets: [full_time],
      excludes: [auxiliary_pioneer, regular_pioneer, field_missionary], visibility: congregation }
  - { id: field_missionary, label: q.field_missionary, duration: permanent,
      requires: { has_qualification: baptized }, sets: [full_time],
      excludes: [auxiliary_pioneer, regular_pioneer, special_pioneer], visibility: congregation }
```

### 13.3 Unità (`units.yaml`)

```yaml
unit_types:
  - { id: group, label: u.group, membership: exclusive, member_requires: { has_qualification: publisher } }
```

### 13.4 Incarichi (`roles.yaml`)

```yaml
roles:
  # --- di gruppo ---
  - id: group_overseer
    label: r.group_overseer
    scope: unit:group
    requires: { has_qualification: elder }
    holders: { min: 0, max: 1 }
    exclusive_with: [group_servant]
    priority: 300
    permissions: &group_report_perms
      - { permission: document.read, on: form:service_report, target: own_scope }
      - { permission: batch.collect, on: form:service_report, target: own_scope }
      - { permission: batch.forward, on: form:service_report, target: own_scope }
      - { permission: document.fill_on_behalf, on: form:service_report, target: own_scope }
    mandatory: { always: true }          # coperto da questo o da group_servant
  - id: group_servant
    label: r.group_servant
    scope: unit:group
    requires: { has_qualification: ministerial_servant }
    holders: { min: 0, max: 1 }
    substitute_for: group_overseer
    exclusive_with: [group_overseer]
    priority: 250
    permissions: *group_report_perms
  - id: group_supervisor
    label: r.group_supervisor
    scope: unit:group
    requires: { has_qualification: elder }
    holders: { min: 0, max: 1 }
    supervises: group_servant
    mandatory: { holds_role: { role: group_servant, where: subject_unit } }
    priority: 300
    permissions:
      - { permission: document.read, on: form:service_report, target: own_scope }
      - { permission: batch.forward, on: form:service_report, target: own_scope }
  - id: group_assistant
    label: r.group_assistant
    scope: unit:group
    requires: { any: [ { has_qualification: elder }, { has_qualification: ministerial_servant } ] }
    holders: { min: 0, max: 1 }
    priority: 200
    permissions: *group_report_perms
  # --- di congregazione ---
  - id: secretary
    label: r.secretary
    scope: organization
    requires: { has_qualification: elder }
    holders: { min: 1, max: 1 }
    priority: 400
    permissions:
      - { permission: document.read, on: form:service_report, target: organization }
      - { permission: batch.register, on: form:service_report, target: organization }
      - { permission: aggregate.generate, on: aggregate:monthly_summary, target: organization }
      - { permission: document.distribute, on: aggregate:monthly_summary, target: organization }
      - { permission: document.fill_on_behalf, on: form:service_report, target: organization }
  - { id: lmm_overseer, label: r.lmm_overseer, scope: organization, requires: { has_qualification: elder }, holders: { min: 1, max: 1 },
      permissions: [ { permission: program.draft, on: program:midweek }, { permission: program.assign, on: program:midweek } ] }
  - { id: public_meeting_coordinator, label: r.public_meeting_coordinator, scope: organization, requires: { has_qualification: elder }, holders: { min: 1, max: 1 },
      permissions: [ { permission: program.draft, on: program:weekend }, { permission: program.assign, on: program:weekend } ] }
  - { id: av_manager, label: r.av_manager, scope: organization, requires: &elder_or_ms { any: [ { has_qualification: elder }, { has_qualification: ministerial_servant } ] }, holders: { min: 1, max: 1 },
      permissions: [ { permission: program.draft, on: program:av }, { permission: program.assign, on: program:av } ] }
  - { id: av_assistant, label: r.av_assistant, scope: organization, requires: *elder_or_ms, holders: { min: 0, max: null }, assistant_of: av_manager,
      permissions: [ { permission: program.draft, on: program:av }, { permission: program.assign, on: program:av } ] }
  - { id: attendant_manager, label: r.attendant_manager, scope: organization, requires: *elder_or_ms, holders: { min: 1, max: 1 },
      permissions: [ { permission: program.draft, on: program:attendants }, { permission: program.assign, on: program:attendants } ] }
  - { id: attendant_assistant, label: r.attendant_assistant, scope: organization, requires: *elder_or_ms, holders: { min: 0, max: null }, assistant_of: attendant_manager,
      permissions: [ { permission: program.draft, on: program:attendants }, { permission: program.assign, on: program:attendants } ] }
  - { id: territory_manager, label: r.territory_manager, scope: organization, requires: *elder_or_ms, holders: { min: 1, max: 1 },
      permissions: [ { permission: resource.manage, on: resource:territory }, { permission: resource.assign, on: resource:territory }, { permission: resource.request, on: resource:territory } ] }
  - { id: territory_assistant, label: r.territory_assistant, scope: organization, requires: *elder_or_ms, holders: { min: 0, max: null }, assistant_of: territory_manager,
      permissions: [ { permission: resource.assign, on: resource:territory }, { permission: resource.request, on: resource:territory } ] }
  - { id: field_service_organizer, label: r.field_service_organizer, scope: organization, requires: *elder_or_ms, holders: { min: 0, max: null },   # ⚙
      permissions: [ { permission: program.draft, on: program:field_service }, { permission: program.assign, on: program:field_service } ] }
  - { id: board_manager, label: r.board_manager, scope: organization, requires: *elder_or_ms, holders: { min: 1, max: null },   # ⚙
      permissions: [ { permission: board.publish }, { permission: board.withdraw } ] }
  - { id: app_admin, label: r.app_admin, scope: organization, requires: { has_qualification: elder }, holders: { min: 2, max: null },   # ⚙
      permissions: [ { permission: people.manage }, { permission: qualifications.manage }, { permission: units.manage },
                     { permission: roles.grant }, { permission: devices.manage }, { permission: calendar.manage }, { permission: config.publish } ] }
```

Nota: gli ancoraggi YAML (`&`, `*`) sono ammessi nel formato sorgente e valgono solo all'interno dello stesso file; il loader li espande prima della validazione. Le chiavi di primo livello che iniziano con `x-` sono ignorate.

### 13.5 Pubblici (`audiences.yaml`)

```yaml
audiences:
  - { id: congregation, select: { with_qualification: publisher } }
  - { id: elders, select: { with_qualification: elder } }
  - { id: elders_and_servants, select: { union: [ { with_qualification: elder }, { with_qualification: ministerial_servant } ] } }
  - { id: full_time, select: { with_set: full_time } }
  - id: group_oversight
    select: { union: [ { role_holders: { role: group_overseer, where: subject_unit } },
                       { role_holders: { role: group_servant, where: subject_unit } },
                       { role_holders: { role: group_assistant, where: subject_unit } },
                       { supervisors_of: subject_unit } ] }
  - { id: secretariat, select: { role_holders: { role: secretary, where: anywhere } } }
  - { id: report_submitted_readers, select: { union: [ { subject: true }, { audience: group_oversight }, { audience: secretariat } ] } }
  - { id: report_registered_readers, select: { union: [ { audience: report_submitted_readers }, { audience: elders } ] } }
  - { id: av_team, select: { union: [ { role_holders: { role: av_manager, where: anywhere } }, { role_holders: { role: av_assistant, where: anywhere } } ] } }
  - { id: attendant_team, select: { union: [ { role_holders: { role: attendant_manager, where: anywhere } }, { role_holders: { role: attendant_assistant, where: anywhere } } ] } }
  - { id: territory_team, select: { union: [ { role_holders: { role: territory_manager, where: anywhere } }, { role_holders: { role: territory_assistant, where: anywhere } } ] } }
  - { id: board_public, select: { audience: congregation } }           # ⚙
  - { id: contacts_readers, select: { union: [ { audience: elders }, { audience: group_oversight }, { subject: true } ] } }   # ⚙
  - { id: admins_and_elders, select: { union: [ { audience: elders }, { role_holders: { role: app_admin, where: anywhere } } ] } }
```

### 13.6 Rapporto di servizio (`forms.yaml`, `workflows.yaml`)

```yaml
forms:
  - id: service_report
    label: f.service_report
    subject: { has_qualification: publisher }
    periodicity: monthly
    open_at: { offset_days: 0 }          # 1° del mese successivo
    due: { offset_days: 5 }              # ⚙
    batching: { unit_type: group }
    workflow: service_report
    fillers_on_behalf: group_oversight
    read_audience_by_state:
      draft: subject_only
      submitted: report_submitted_readers
      collected: report_submitted_readers
      forwarded: report_submitted_readers
      registered: report_registered_readers
    variants:
      - id: full_time
        when: { in_set: full_time }      # valutato sul mese del rapporto
        fields:
          - { id: hours, type: duration, label: fld.hours, min: 0, max: 31000 }       # ⚙ max in minuti
          - { id: extra_hours, type: duration, label: fld.extra_hours, min: 0 }       # ⚙ significato da confermare
          - { id: bible_studies, type: integer, label: fld.bible_studies, min: 0, max: 99 }
          - { id: notes, type: long_text, label: fld.notes, max_length: 1000 }
      - id: publisher
        when: { always: true }
        fields:
          - { id: participated, type: boolean, label: fld.participated }
          - { id: bible_studies, type: integer, label: fld.bible_studies, min: 0, max: 99 }

workflows:
  - id: service_report
    states: [draft, submitted, collected, forwarded, registered]
    initial: draft
    steps:
      - { id: submit, kind: transition, from: [draft], to: submitted, by: subject_or_on_behalf }
      - { id: collect, kind: collect, from: [submitted], to: collected }            # automatico nel lotto del gruppo
      - { id: forward, kind: forward, from: [collected], to: forwarded, by: group_oversight, rule: any,
          allow_incomplete_after: { offset_days: 8 },                              # ⚙
          effects: [ { notify: { audience: secretariat, text: n.batch_forwarded } } ] }
      - { id: register, kind: register, from: [forwarded], to: registered, by: secretariat }
      - { id: send_back, kind: transition, from: [submitted, collected], to: draft, by: group_oversight, note: required,
          effects: [ { notify: { audience: subject_only, text: n.report_sent_back } } ] }
    timers:
      - { at: { offset_days: 5, from: period_end }, do: { remind: { audience: subject_missing, text: n.report_missing } } }
      - { at: { offset_days: 8, from: period_end }, do: { remind: { audience: group_oversight, text: n.batch_incomplete } } }
      - { at: { offset_days: 15, from: period_end }, do: { remind: { audience: secretariat, text: n.batches_missing } } }
```

`subject_only`, `subject_or_on_behalf`, `subject_missing` sono pubblici di sistema del nucleo (sempre disponibili).

### 13.7 Resoconto mensile (`aggregates.yaml`)

```yaml
aggregates:
  - id: monthly_summary
    label: a.monthly_summary
    source: { form: service_report, state: registered, period: same }
    group_by: [variant, unit:group]
    measures:
      - { id: publishers_participated, op: count_true, field: participated }
      - { id: publisher_studies, op: sum, field: bible_studies, filter: { field: { name: participated, op: set } } }
      - { id: full_time_count, op: count, filter: { in_set: full_time } }
      - { id: auxiliary_count, op: count, filter: { has_qualification: auxiliary_pioneer } }
      - { id: regular_count, op: count, filter: { has_qualification: regular_pioneer } }
      - { id: special_and_missionary_count, op: count, filter: { any: [ { has_qualification: special_pioneer }, { has_qualification: field_missionary } ] } }
      - { id: hours_total, op: sum, field: hours }
      - { id: extra_hours_total, op: sum, field: extra_hours }
      - { id: full_time_studies, op: sum, field: bible_studies, filter: { in_set: full_time } }
    include_individual_documents: true
    list_missing: true
    read_audience: elders
```

Flusso del resoconto: il segretario genera (`aggregate.generate`) quando tutti i lotti sono registrati (o forzando con mancanti), può aggiungere note, poi distribuisce (`document.distribute`) al pubblico `elders`. Integrazioni successive → nuova versione e nuova distribuzione.

### 13.8 Calendario (`calendar.yaml`)

```yaml
event_types:
  - { id: midweek_meeting, label: e.midweek, default_recurrence: "FREQ=WEEKLY;BYDAY=TH", default_time: "19:00", default_duration_min: 105 }   # ⚙ giorno/ora
  - { id: weekend_meeting, label: e.weekend, default_recurrence: "FREQ=WEEKLY;BYDAY=SU", default_time: "10:00", default_duration_min: 105 }   # ⚙
  - { id: field_service_meeting, label: e.field_service, default_recurrence: null, location_required: true, scope: organization }
```

Settimane speciali (visita del sorvegliante di circoscrizione, assemblee) = eccezioni della ricorrenza gestite da `calendar.manage`.

### 13.9 Programmi (`programs.yaml`)

```yaml
# gli ancoraggi YAML valgono solo nel file in cui sono definiti
x-requirements:
  elder: &elder { has_qualification: elder }
  elder_or_ms: &elder_or_ms { any: [ { has_qualification: elder }, { has_qualification: ministerial_servant } ] }
program_types:
  - id: midweek
    label: p.midweek
    event_type: midweek_meeting
    period: week
    owner_role: lmm_overseer
    acceptance: true                     # ⚙
    publish_audience: board_public
    remind_before: { days: 2 }
    slots:                               # ⚙ struttura da verificare con il sorvegliante; requisiti modificabili
      - { id: chairman, label: s.chairman, requires: *elder }   # ⚙ da confermare
      - { id: opening_prayer, label: s.opening_prayer, requires: { has_qualification: baptized } }
      - { id: treasures_talk, label: s.treasures_talk, requires: *elder_or_ms, extra_fields: [ { id: title, type: text } ] }
      - { id: spiritual_gems, label: s.spiritual_gems, requires: *elder_or_ms }
      - { id: bible_reading, label: s.bible_reading, requires: { attribute: { name: sex, op: eq, value: m } } }
      - { id: student_part, label: s.student_part, repeat: { count: 4 }, requires: { has_qualification: publisher },
          extra_fields: [ { id: title, type: text }, { id: minutes, type: integer } ], pair_with: student_assistant }
      - { id: student_assistant, label: s.student_assistant, repeat: { count: 4 }, requires: { has_qualification: publisher } }
      - { id: living_part, label: s.living_part, repeat: { count: 2 }, requires: *elder_or_ms, extra_fields: [ { id: title, type: text } ] }
      - { id: cbs_conductor, label: s.cbs_conductor, requires: *elder_or_ms }
      - { id: cbs_reader, label: s.cbs_reader, requires: { attribute: { name: sex, op: eq, value: m } } }
      - { id: closing_prayer, label: s.closing_prayer, requires: { has_qualification: baptized } }
  - id: weekend
    label: p.weekend
    event_type: weekend_meeting
    period: week
    owner_role: public_meeting_coordinator
    acceptance: true
    publish_audience: board_public
    slots:
      - { id: talk, label: s.public_talk, allow_external: true, requires: *elder_or_ms,
          extra_fields: [ { id: theme, type: text }, { id: home_congregation, type: text } ] }
      - { id: opening_prayer, label: s.opening_prayer, requires: { has_qualification: baptized } }
      - { id: closing_prayer, label: s.closing_prayer, requires: { has_qualification: baptized } }
      - { id: wt_reader, label: s.wt_reader, requires: { attribute: { name: sex, op: eq, value: m } } }
  - id: av
    label: p.av
    event_type: [midweek_meeting, weekend_meeting]
    period: month
    owner_role: av_manager
    assistant_roles: [av_assistant]
    acceptance: true
    publish_audience: board_public
    slots:
      - { id: mixer, label: s.mixer, count: 1, requires: { has_qualification: baptized } }   # ⚙
      - { id: microphones, label: s.microphones, count: 2, requires: { has_qualification: baptized } }   # ⚙ numero
  - id: attendants
    label: p.attendants
    event_type: [midweek_meeting, weekend_meeting]
    period: month
    owner_role: attendant_manager
    assistant_roles: [attendant_assistant]
    acceptance: true
    publish_audience: board_public
    slots:
      - { id: entrance, label: s.entrance, count: 1, requires: { has_qualification: baptized } }   # ⚙
      - { id: hall, label: s.hall, count: 1, requires: { has_qualification: baptized } }           # ⚙
  - id: field_service
    label: p.field_service
    event_type: field_service_meeting
    period: week
    owner_role: field_service_organizer
    acceptance: true
    publish_audience: board_public
    slots:
      - { id: conductor, label: s.fs_conductor, requires: *elder_or_ms }   # ⚙
```

Le chiavi che iniziano con `x-` sono ignorate dal loader (servono solo a contenere ancoraggi). Persone esterne (oratori ospiti) sono `ExternalPerson`: non ricevono notifiche; l'organizzatore registra la loro conferma con `assignment.respond_on_behalf`.

### 13.10 Bacheca (`board.yaml`)

```yaml
board:
  default_audience: board_public
  default_visibility: { from: period_start_minus_days_30, until: period_end }
  free_notices: { allowed: true, audiences: [board_public, elders_and_servants, elders] }
```

Ciclo: i programmi in stato `assigned` compaiono nella coda del responsabile della bacheca, che li pubblica (`program.published` + `board.posted`).

### 13.11 Territori (`resources.yaml`)

```yaml
resource_types:
  - id: territory
    label: res.territory
    attributes:
      - { id: number, type: text, label: fld.territory_number }
      - { id: name, type: text, label: fld.territory_name }
      - { id: kind, type: choice, options: [residential, business, rural, other], label: fld.territory_kind }
      - { id: notes, type: long_text, label: fld.notes }
      - { id: map, type: photos, max_count: 3, label: fld.map }
    holder_requires: { has_qualification: publisher }
    manager_role: territory_manager
    assistant_roles: [territory_assistant]
    log_fields:
      - { id: completed_on, type: date, label: fld.completed_on }
      - { id: notes, type: long_text, label: fld.notes }
    requests:
      - { id: log_or_return, label: rq.log_or_return, answers: [logged, returned, not_yet], due: { days: 14 } }
      - { id: return, label: rq.return, answers: [returned], due: { days: 14 } }
    rules:
      max_per_holder: 3                                  # ⚙
      remind_holder_after_no_log: { days: 120 }          # ⚙
      alert_manager_not_worked: { days: 365 }            # ⚙
    read_audience:
      list: territory_team
      history: territory_team
      attachments_holder: true
```

### 13.12 Notifiche (`notifications.yaml`)

| Trigger / promemoria | Pubblico | Testo (chiave) |
| --- | --- | --- |
| apertura `service_report` (1° del mese) | soggetto | `n.report_open` |
| `document.missing_after` 5 gg | soggetto | `n.report_missing` |
| `document.state_changed` → `submitted` | `group_oversight` | `n.report_received` |
| `batch.incomplete_after` 8 gg | `group_oversight` | `n.batch_incomplete` |
| `batch.forwarded` | `secretariat` | `n.batch_forwarded` |
| `batch.missing_after` 15 gg | `secretariat` | `n.batches_missing` |
| `aggregate.distributed` | `elders` | `n.summary_available` |
| `assignment.created` | assegnato | `n.assignment_new` |
| `assignment.declined` | responsabile + assistenti del programma | `n.assignment_declined` |
| `event.assignment_before` 2 gg | assegnato | `n.assignment_reminder` |
| `program.published` | `board_public` | `n.program_published` |
| `resource.assigned`, `resource.request_opened` | detentore | `n.territory_assigned`, `n.territory_request` |
| `qualification.expiring` (`auxiliary_pioneer`) | persona | `n.aux_pioneer_ending` |
| `role.uncovered`, `grant.invalid` | `app_admin` | `n.admin_role_uncovered`, `n.admin_grant_invalid` |

### 13.13 Riservatezza riepilogata

| Dato | Pubblico di lettura |
| --- | --- |
| Rapporto inviato | Soggetto, sorveglianza del gruppo, segretario |
| Rapporto registrato | + anziani |
| Lotto | Sorveglianza del gruppo, segretario; dopo la registrazione, anziani |
| Resoconto | Segretario, anziani |
| Qualifiche | Secondo `visibility` di ciascuna (§13.2) |
| Assegnazioni in bozza/notificate | Responsabile e assistenti del programma, assegnato |
| Programmi pubblicati | `board_public` |
| Territori | Squadra territori; detentore durante la detenzione |
| Contatti | `contacts_readers` |

### 13.14 Testi

`texts/it.yaml` contiene tutte le chiavi usate sopra (prefissi `q.`, `u.`, `r.`, `f.`, `fld.`, `a.`, `e.`, `p.`, `s.`, `res.`, `rq.`, `n.`, `menu.`), il nome dell'app, i testi di onboarding e della push iOS generica. Le etichette in italiano seguono la terminologia usata dalla congregazione (es. "Proclamatore", "Anziano", "Servitore di ministero", "Pioniere ausiliare", "Sorvegliante di gruppo", "Rapporto di servizio", "Adunanza Vita e ministero cristiano", "Studio Torre di Guardia", "Uscieri", "Territori", "Bacheca").

---

## 14. Privacy e conformità

- I dati del pacchetto Congregazione rientrano nelle **categorie particolari** dell'art. 9 GDPR (convinzioni religiose). La realizzazione deve permettere all'organizzazione di adempiere ai propri obblighi; le decisioni giuridiche (base giuridica, titolare, informativa, durate) spettano all'organizzazione.
- **Minimizzazione**: solo i campi dichiarati; EXIF rimossi; nessun dato nei log; sul server solo dati cifrati che il server non può leggere (§10.3).
- **Informativa**: `privacy/it.md` mostrata al primo avvio con presa visione registrata (evento locale, non condiviso).
- **Diritti**: esportazione dei dati di una persona (JSON + PDF) da `people`; cancellazione con `person.data_erasure_requested` → ogni dispositivo elimina i payload decifrabili relativi e gli allegati; gli eventi restano come gusci (id, autore, tipo) per l'integrità del grafo, con payload sostituito da tombstone.
- **Conservazione**: per tipo di documento, configurabile nel pacchetto (`retention` in `forms.yaml`, predefinito ⚙ 5 anni per i rapporti, 2 anni per programmi, storico territori senza scadenza); applicata con `retention.applied`.
- **Dispositivo perso**: revoca (`device.revoked`) + rotazione dei canali + rimozione del token push.

---

## 15. Test

### 15.1 Test unitari (per crate)

| Crate | Cosa testare |
| --- | --- |
| `core-model` | Valutazione di `Cond` e risoluzione dei pubblici su casi limite (date di confine, qualifiche a periodo fisso, unità esclusive) |
| `core-config` | Ogni regola di validazione di §6.4 con un caso valido e uno non valido; messaggi d'errore con file e percorso |
| `core-crypto` | Cifratura/decifratura, distribuzione chiavi, rotazione, rifiuto di AAD alterato, vettori di test fissi |
| `core-events` | Firma e verifica, calcolo `id`, ordinamento deterministico, eventi con genitori mancanti, archivio SQLite |
| `core-engine` | Permessi sul passato causale, ogni tipo di passaggio di flusso, conflitti di §7.6, aggregazioni, programmi, risorse, promemoria |
| `core-sync` | Riconciliazione tra due archivi con differenze casuali, ricevute, stato di consegna |
| `push-service` | Firme delle richieste, limiti, autorizzazione per organizzazione (FCM/APNs simulati) |

### 15.2 Test di convergenza (proprietà)

Con `proptest`: generare insiemi casuali di eventi validi e non validi da più dispositivi, consegnarli in **ordini diversi** a N archivi, verificare che le proiezioni finali siano **identiche byte per byte** (hash dello stato serializzato). Includere revoche concorrenti, passaggi concorrenti, rotazioni concorrenti e configurazioni concorrenti.

### 15.3 Simulatore multi-dispositivo (`core-testkit`)

- Avvia N nodi nello stesso processo con endpoint iroh in locale (senza relay) o con un trasporto in memoria che rispetta la stessa interfaccia.
- Modella disponibilità realistica: telefoni online pochi minuti al giorno, desktop sempre online, reti che cadono.
- Comando `cargo xtask simulate` genera un'organizzazione fittizia dal pacchetto (es. 120 persone, 7 gruppi), simula 60 giorni di attività e produce un rapporto: tempi di consegna (mediana, 95° percentile) per tipo di evento, spazio occupato per dispositivo, eventi non validi, conflitti.
- **Obiettivo**: con almeno un desktop d'archivio sempre acceso, il 95% dei rapporti arriva alla sorveglianza del gruppo entro 24 ore dall'invio nella simulazione realistica.

### 15.4 Test di scenario del pacchetto

File YAML in `packages/<nome>/tests/`, eseguiti da `xtask test-package`:

```yaml
name: rapporto inviato e registrato
given:
  people:
    - { id: anna, qualifications: [publisher, baptized], group: g1 }
    - { id: marco, qualifications: [publisher, baptized, elder], group: g1, roles: [group_overseer@g1, secretary] }
    - { id: luca, qualifications: [publisher, baptized, elder], group: g2 }
  date: 2026-12-01
when:
  - { as: anna, do: document.fill, form: service_report, period: 2026-11, values: { participated: true, bible_studies: 1 } }
  - { as: anna, do: document.step, step: submit }
  - { as: marco, do: batch.forward, group: g1, period: 2026-11, at: 2026-12-09 }
  - { as: marco, do: batch.register, group: g1, period: 2026-11 }
then:
  - { can_read: { who: luca, document: anna/service_report/2026-11 } }        # anziano dopo registrazione
  - { cannot_read: { who: luca, document: anna/service_report/2026-11, before_step: register } }
  - { state: { document: anna/service_report/2026-11, is: registered } }
  - { denied: { as: anna, do: batch.forward, group: g1, period: 2026-11 } }
```

Scenari minimi obbligatori per il pacchetto Congregazione:

1. Rapporto proclamatore e rapporto pioniere ausiliare (variante corretta per il mese).
2. Pioniere ausiliare solo per ottobre: rapporto di novembre torna alla variante proclamatore.
3. Gruppo con servitore di gruppo e supervisore: entrambi vedono e inoltrano.
4. Assistente inoltra al posto del sorvegliante; sorvegliante e assistente inoltrano offline in contemporanea (un solo inoltro efficace).
5. Proclamatore di un altro gruppo **non** legge il rapporto.
6. Resoconto: somme e conteggi corretti, mancanti elencati, distribuito agli anziani, integrazione tardiva → nuova versione.
7. Conferimento di sorvegliante a un servitore di ministero rifiutato.
8. Cambio di gruppo a metà mese: il rapporto del mese segue il gruppo alla data di fine periodo ⚙.
9. Programma Vita e ministero: requisiti di slot, assegnazione declinata, pubblicazione in bacheca da parte del responsabile della bacheca, visibilità prima/dopo.
10. Oratore esterno all'adunanza pubblica con conferma registrata dall'organizzatore.
11. Territorio: assegnazione, richiesta "registra o restituisci", registrazione, restituzione, storico; limite `max_per_holder`.
12. Revoca di un dispositivo e rotazione: il dispositivo revocato non legge i rapporti successivi.
13. Fine dell'incarico di segretario: il nuovo segretario legge i lotti successivi, il precedente no.
14. Compilazione per conto di un proclamatore senza app.

### 15.5 Test dell'interfaccia

- Playwright sulla build desktop di sviluppo (o sul frontend con un back-end simulato): onboarding, compilazione rapporto con foto, coda di lavoro, editor programma, bacheca, territori.
- Checklist manuale per mobile in `docs/MOBILE_CHECKLIST.md`: push, background, fotocamera, QR, blocco biometrico, oscuramento in app switcher, rotazione schermo, testo grande.

### 15.6 Controllo di fuga del dominio

`cargo xtask check-domain-leak` scorre `crates/` e `apps/app/` (esclusi i file generati) e fallisce se trova, senza distinzione di maiuscole, termini presi dall'elenco `domain_terms` dichiarato in ogni pacchetto (`package.yaml`). Per il pacchetto Congregazione: `anzian`, `elder`, `servitor`, `servant`, `pionier`, `pioneer`, `proclamator`, `publisher`, `congregaz`, `congregation`, `territor`, `uscier`, `attendant`, `rapporto`, `service_report`, `adunanz`, `meeting`, `sorvegliant`, `overseer`, `segretari`, `secretary`, `torre di guardia`, `watchtower`. Gira in CI.

### 15.7 CI

`ci.yml` su ogni push: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, test di proprietà con seed fisso più un seed casuale, `xtask validate` e `xtask test-package` per ogni pacchetto, `xtask check-domain-leak`, lint e test del frontend, build desktop Linux. `release.yml` su tag: build Windows, macOS, Linux, Android (AAB) e iOS (runner macOS) con il pacchetto indicato.

---

## 16. Fasi di realizzazione e criteri di accettazione

Ogni fase si chiude solo se **tutti** i criteri passano e i test relativi sono in CI.

| Fase | Contenuto | Criteri di accettazione |
| --- | --- | --- |
| **F0 — Fondamenta** | Workspace, crate vuoti, CI, `xtask` con `schema`/`validate`/`check-domain-leak`, `docs/DECISIONS.md` | CI verde; schema JSON generato; pacchetto vuoto validato |
| **F1 — Modello e configurazione** | `core-model`, `core-config`, pacchetto Congregazione completo in YAML, testi | `xtask validate --package congregazione` passa; ogni regola di §6.4 ha test |
| **F2 — Eventi e crittografia** | `core-events`, `core-crypto`, archivio SQLCipher, genesi | Firma/verifica, canali, distribuzione e rotazione testati; vettori di test |
| **F3 — Motore** | `core-engine`: permessi, pubblici, flussi, lotti, aggregati, programmi, risorse, bacheca, promemoria, conflitti | Test di convergenza (§15.2) verdi; tutti gli scenari di §15.4 verdi via `xtask test-package` |
| **F4 — Sincronizzazione** | `core-sync`: endpoint iroh, gossip, riconciliazione, ricevute, blob, abbinamento | Due e dieci nodi convergono; simulatore (§15.3) produce il rapporto; obiettivo del 95% entro 24 ore |
| **F5 — App desktop** | Tauri desktop, `core-api`, binding TS, viste generiche §12.2 (senza editor configurazione), notifiche locali, tray | Flusso completo rapporto → resoconto tra 3 istanze desktop su macchine diverse via relay pubblico; test Playwright |
| **F6 — Server** | `push-service`, deploy con relay e DNS propri, `docs/DEPLOY.md` | Deploy riproducibile da `docker compose up`; app configurate sul relay proprio; test del servizio push |
| **F7 — Mobile** | Build Android e iOS, `SecretStore` mobile, push, background, fotocamera, QR, biometria | Checklist mobile completata su almeno un dispositivo reale per piattaforma; risveglio via push funzionante |
| **F8 — Amministrazione e configurazione** | Editor di configurazione desktop con simulazione, migrazioni, dispositivi d'archivio, backup | Modifica di uno slot e di un incarico pubblicata senza nuova build; backup ripristinato su un nuovo desktop |
| **F9 — Privacy e rifinitura** | Esportazione e cancellazione dati, conservazione, informativa, accessibilità, prestazioni | Scenario di cancellazione verificato su 3 dispositivi; avvio a freddo < 2 s su telefono di fascia media con un anno di dati simulati |
| **F10 — Pilota** | Uso reale con un gruppo ridotto | Registro dei problemi in `docs/PILOT.md`; nessun dato perso; tempi di consegna misurati |
| **F11 — Backup cieco** | `org.backup_node_set`, stato `stored`, allegati di tutti gli oggetti in `attachments`, `server/blind-node`, cancellazione e conservazione sul nodo, deploy | Tre dispositivi mai accesi insieme convergono passando dal nodo; il nodo non ha chiavi e non decifra nulla; cancellazione applicata sul nodo; deploy da `docker compose up` |

---

## 17. Valori predefiniti per le questioni aperte

Ogni valore è configurabile nel pacchetto. L'agente li applica e li elenca in `docs/DECISIONS.md` come "da confermare".

| Questione | Valore predefinito |
| --- | --- |
| Accettazione delle assegnazioni | Attiva per tutti i programmi |
| Pubblico della bacheca | Tutti i proclamatori (`congregation`) |
| Requisiti del responsabile della bacheca | Anziano o servitore di ministero |
| Organizzatore del servizio di campo | Incarico `field_service_organizer` (anziano o servitore), più titolari |
| Struttura Vita e ministero | Come §13.9; presidente: anziano |
| Adunanza pubblica: presidente e conduttore Torre di Guardia | Non gestiti nella v1; aggiungibili come slot |
| Scadenze rapporti | Invio entro il 5, inoltro con mancanti dal giorno 8, promemoria al segretario il 15 |
| Ore extra | Campo durata libero, senza controlli oltre `min: 0` |
| Correzione dei rapporti | Prima della registrazione: il soggetto se rimandato; la sorveglianza del gruppo per conto. Dopo: solo il segretario, con nuova versione tracciata |
| Mappe dei territori | Immagini (foto o PNG/JPEG); coordinate non gestite nella v1 |
| Storico territori | Conservato senza scadenza, consultabile per anno di servizio (settembre–agosto) |
| Persone senza app | Attributo `uses_app = false`; rapporto compilato per conto dalla sorveglianza del gruppo o dal segretario; assegnazioni registrate per conto dal responsabile |
| Gruppo di appartenenza per il rapporto | Il gruppo alla data di fine del periodo |
| Visibilità delle qualifiche | Anziano, servitore, pioniere regolare/speciale, missionario: congregazione; battezzato e pioniere ausiliare: anziani |
| Contatti | Leggibili da anziani, sorveglianza del gruppo e dalla persona stessa |
| Conservazione | Rapporti 5 anni; programmi 2 anni; resoconti 5 anni; storico territori senza scadenza |
| Amministratori radice | Almeno 2 anziani |
| Push iOS | Notifica visibile con testo generico "Ci sono aggiornamenti" |
| Dispositivi d'archivio | Almeno 2 desktop (es. segretario e un altro anziano) |
| Indicazioni della congregazione su strumenti digitali | Non note: l'organizzazione deve verificarle prima del pilota |

---

## 18. Glossario (dominio → codice)

| Italiano (pacchetto) | Concetto del nucleo | Identificatore nel pacchetto |
| --- | --- | --- |
| Congregazione | Organizzazione | `org` |
| Proclamatore | Qualifica | `publisher` |
| Battezzato | Qualifica | `baptized` |
| Anziano | Qualifica | `elder` |
| Servitore di ministero | Qualifica | `ministerial_servant` |
| Pioniere ausiliare / regolare / speciale | Qualifica | `auxiliary_pioneer` / `regular_pioneer` / `special_pioneer` |
| Missionario sul campo | Qualifica | `field_missionary` |
| Servizio a tempo pieno | Insieme di qualifiche | `full_time` |
| Gruppo | Tipo di unità | `group` |
| Sorvegliante di gruppo | Incarico | `group_overseer` |
| Assistente di gruppo | Incarico | `group_assistant` |
| Servitore di gruppo | Incarico sostituto | `group_servant` |
| Supervisore di gruppo | Incarico di supervisione | `group_supervisor` |
| Segretario | Incarico | `secretary` |
| Sorvegliante adunanza Vita e ministero | Incarico | `lmm_overseer` |
| Organizzatore adunanza pubblica | Incarico | `public_meeting_coordinator` |
| Responsabile / assistente audio/video | Incarichi | `av_manager` / `av_assistant` |
| Responsabile / assistente uscieri | Incarichi | `attendant_manager` / `attendant_assistant` |
| Responsabile / assistente territori | Incarichi | `territory_manager` / `territory_assistant` |
| Responsabile della bacheca | Incarico | `board_manager` |
| Rapporto di servizio | Tipo di modulo | `service_report` |
| Rapporti del gruppo | Lotto | batch di `service_report` per `group` |
| Resoconto mensile | Aggregazione | `monthly_summary` |
| Adunanza infrasettimanale / del fine settimana | Tipi di evento | `midweek_meeting` / `weekend_meeting` |
| Adunanza per il servizio di campo | Tipo di evento | `field_service_meeting` |
| Programma Vita e ministero / adunanza pubblica / audio-video / uscieri / servizio di campo | Tipi di programma | `midweek` / `weekend` / `av` / `attendants` / `field_service` |
| Parte, turno | Slot | `slots[].id` |
| Oratore ospite | Persona esterna | `ExternalPerson` |
| Territorio | Tipo di risorsa | `territory` |
| Percorrenza | Registrazione della risorsa | `resource.logged` |
| Bacheca | Bacheca | `board` |
