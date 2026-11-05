# Messa in produzione del server

Il server **non conserva eventi né allegati** (PROGETTO.md §10). Serve solo a:

- **relay iroh**: mette in contatto i dispositivi quando non riescono a collegarsi direttamente;
- **servizio push**: sveglia i telefoni quando c'è qualcosa da sincronizzare (la push non contiene dati);
- **DNS di discovery** (facoltativo): permette ai dispositivi di trovarsi senza il servizio di n0.

Tutto gira su una sola VPS con Docker Compose (`server/deploy/`).

## 1. Cosa serve

| Cosa | Esempio | Costo indicativo |
| --- | --- | --- |
| VPS Linux, 2 vCPU, 4 GB RAM, 40 GB SSD, IPv4 pubblico | Hetzner CX22 o simili | 5–10 €/mese |
| Un dominio | `briccolab.it` | ~10 €/anno |
| Docker con il plugin Compose (≥ 2.24) | `apt install docker.io docker-compose-v2` | — |
| Per i telefoni: progetto Firebase (Android) e chiave APNs (iOS) | vedi §5 | Apple 99 €/anno |

## 2. Record DNS

Con `DOMAIN=example.org` e l'IP pubblico della VPS `203.0.113.10`:

| Nome | Tipo | Valore |
| --- | --- | --- |
| `api.example.org` | A (e AAAA se c'è IPv6) | `203.0.113.10` |
| `relay.example.org` | A (e AAAA) | `203.0.113.10` |
| `dns.example.org` | A | `203.0.113.10` (solo con la discovery) |
| `dns.example.org` | NS | `ns1.dns.example.org.` (solo con la discovery) |
| `ns1.dns.example.org` | A | `203.0.113.10` (solo con la discovery) |

## 3. Firewall

| Porta | Protocollo | Servizio |
| --- | --- | --- |
| 80, 443 | TCP | Caddy (certificati, `api.`, `dns.`) |
| 8443 | TCP | relay (HTTPS) |
| 7842 | UDP | relay (scoperta dell'indirizzo via QUIC) |
| 53 | UDP e TCP | DNS di discovery (solo con il profilo `dns`) |
| 22 | TCP | SSH per l'amministrazione |

Il relay usa la 8443 perché la 443 è di Caddy sullo stesso IP. Il certificato di
`relay.<dominio>` lo ottiene Caddy; il relay lo rilegge dal volume di Caddy ogni tanto, quindi i
rinnovi sono automatici.

## 4. Primo avvio

```bash
git clone https://github.com/jonxduo/mosaic_agenda.git
cd mosaic_agenda/server/deploy
cp .env.example .env        # compila DOMAIN, PUBLIC_IP, ACME_EMAIL
mkdir -p secrets            # vedi §5 per i file di FCM e APNs
docker compose up -d --build
# con la discovery propria:
docker compose --profile dns up -d --build
```

La prima build compila i binari ufficiali di iroh (versione fissata nel `Dockerfile.iroh`) e il
servizio push: ci vogliono alcuni minuti. Il relay riparte da solo finché Caddy non ha il
certificato (di solito meno di un minuto).

Controlli:

```bash
curl https://api.example.org/health          # → ok
curl https://relay.example.org:8443/ping     # → 200
docker compose ps
```

## 5. Credenziali push

Senza credenziali il servizio parte lo stesso e risponde, ma non invia push.

- **Android (FCM)**: console Firebase → Impostazioni progetto → Account di servizio → Genera
  nuova chiave privata. Salva il JSON in `server/deploy/secrets/fcm-service-account.json`.
- **iOS (APNs)**: developer.apple.com → Certificates, IDs & Profiles → Keys → nuova chiave con
  "Apple Push Notifications service". Salva il `.p8` in `server/deploy/secrets/apns-key.p8` e
  scrivi in `.env` `APNS_KEY_ID` (id della chiave) e `APNS_TEAM_ID`. `APNS_SANDBOX=true` per le
  build di sviluppo.

I file in `secrets/` e `.env` non vanno mai nel repository (sono in `.gitignore`).

## 6. Collegare l'app al server

Le organizzazioni create da un'app compilata così usano il server indicato: indirizzi di relay,
discovery e push finiscono nell'evento di genesi e valgono per tutti i dispositivi.

```bash
cargo xtask build --package congregazione --target desktop --release --server example.org
# con la discovery propria:
cargo xtask build --package congregazione --target desktop --release --server example.org --dns
```

Senza `--server` l'app usa i relay pubblici di n0 (solo per lo sviluppo).

Il primo amministratore radice che sincronizza registra l'organizzazione sul servizio push,
dimostrando di essere amministratore con l'evento di genesi. Da quel momento il servizio accetta
solo i dispositivi dell'organizzazione, aggiornati automaticamente.

## 6b. Nodo di backup cieco

Tiene per sempre, cifrati, tutti gli eventi e le foto dell'organizzazione e li consegna ai
dispositivi che si collegano dopo (PROGETTO.md §10.3). Non ha chiavi: non legge nulla.

1. Nell'app, da un amministratore radice: **Sincronizzazione** → copiare l'id
   dell'organizzazione.
2. Sul server, in `.env`: `BLIND_ORG=<id>` (e se serve `BLIND_QUOTA_GB`, predefinito 5), poi:

```bash
docker compose --profile backup up -d --build
docker compose logs blind-node | grep "blind node"
```

3. La riga `blind node <chiave>` dà la chiave del nodo: nell'app, **Sincronizzazione** → nodo di
   backup → incollarla e salvare. Da quel momento tutti i dispositivi lo contattano per primo e i
   documenti mostrano "Sul server" appena il nodo li ha.

Chiave del nodo e chiave dell'archivio sono in `/data/secrets` (volume `blind_data`): se si
perdono, il nodo riparte con una chiave nuova da dichiarare di nuovo e si riempie dai dispositivi.
Il volume va incluso nel backup (§8); contiene solo dati cifrati.

## 7. Aggiornamenti

```bash
cd mosaic_agenda && git pull
cd server/deploy && docker compose up -d --build
```

Per cambiare versione di iroh: `IROH_VERSION` in `Dockerfile.iroh` (deve essere compatibile con la
versione usata dall'app, vedi `Cargo.toml`).

## 8. Backup

- Il servizio push salva ogni giorno una copia del suo archivio in `/data/backups` (volume
  `push_data`), tenendo le ultime 7. L'archivio contiene solo chiavi pubbliche dei dispositivi e
  token push: se si perde, i telefoni si registrano di nuovo da soli.
- Il nodo di backup cieco è nel volume `blind_data` (dati cifrati e le sue chiavi): copiarlo
  come gli altri, a nodo fermo (`docker compose stop blind-node`).
- I certificati sono nel volume `caddy_data`; se si perdono, Caddy li richiede di nuovo.
- Copia fuori dal server (es. ogni notte):

```bash
docker run --rm -v orgapp_push_data:/d -v "$PWD":/out debian:bookworm-slim \
  tar czf /out/push-backup.tgz -C /d backups
```

## 9. Monitoraggio

- `docker compose ps` e `docker compose logs --tail 100 <servizio>`.
- `GET https://api.<dominio>/health` e `GET https://relay.<dominio>:8443/ping` da un controllo
  esterno (es. un servizio di uptime gratuito).
- Metriche del relay su `127.0.0.1:9090` dentro il container (non esposte).

## 10. Privacy

- Caddy non scrive log di accesso; il servizio push non registra IP né contenuti.
- Il relay inoltra pacchetti già cifrati end to end: non può leggerli.
- Il servizio push conosce: chiavi pubbliche dei dispositivi, a quale organizzazione appartengono
  (un identificativo opaco), i token push e il testo generico della notifica iOS. Alla prima
  registrazione riceve l'evento di genesi per verificare l'amministratore: contiene il nome
  dell'organizzazione e la configurazione del pacchetto, che non vengono salvati.

## 11. Prova locale (senza dominio)

```bash
cd server/deploy && cp .env.example .env
docker compose -f docker-compose.yml -f docker-compose.ci.yml up -d --build iroh-relay push-service
# relay su http://localhost:3340, push su http://localhost:8080
OWN_RELAY_URL=http://localhost:3340 cargo test -p core-api --test flow three_devices_over_own_relay -- --ignored
```

È quello che fa la CI (job `server`) a ogni push.
