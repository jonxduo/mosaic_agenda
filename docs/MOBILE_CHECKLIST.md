# Checklist mobile (PROGETTO.md §15, F7)

Da completare su **almeno un telefono reale per piattaforma** prima di chiudere F7. Annotare
modello, versione del sistema, data ed esito (✅ / ❌ con nota).

| Dispositivo | Sistema | Build | Data | Chi |
| --- | --- | --- | --- | --- |
| | Android | | | |
| | iOS | | | |

## Installazione e primo avvio

| # | Prova | Android | iOS |
| --- | --- | --- | --- |
| 1 | L'app si installa e si apre; nome e icona sono quelli del pacchetto | | |
| 2 | Informativa privacy mostrata al primo avvio | | |
| 3 | Unirsi con il **QR**: "Inquadra il codice QR" apre la fotocamera, legge il codice mostrato dal desktop, appare il codice di verifica | | |
| 4 | Il codice coincide su desktop e telefono; dopo la conferma il telefono mostra la home | | |
| 5 | Permesso fotocamera negato → messaggio chiaro, si può incollare l'invito a mano | | |

## Segreti e blocco

| # | Prova | Android | iOS |
| --- | --- | --- | --- |
| 6 | Dopo aver chiuso e riaperto l'app i dati ci sono ancora (chiavi nel Keystore/Keychain) | | |
| 7 | Impostare un PIN, chiudere l'app: alla riapertura chiede il PIN | | |
| 8 | "Sblocco con impronta o volto" attivato: lo sblocco funziona con la biometria | | |
| 9 | Biometria annullata o fallita: resta possibile il PIN | | |
| 10 | Blocco automatico dopo il tempo impostato in Impostazioni | | |
| 11 | **Oscuramento nel selettore app**: nell'elenco delle app recenti il contenuto non si vede | | |

## Sincronizzazione e notifiche

| # | Prova | Android | iOS |
| --- | --- | --- | --- |
| 12 | Un rapporto inviato dal telefono arriva al desktop (stato "Consegnato") | | |
| 13 | **Push**: app chiusa, dal desktop si invia qualcosa che riguarda il telefono → entro pochi minuti arriva la notifica di risveglio e, aprendo l'app, il dato c'è | | |
| 14 | **Background** (Android): app chiusa per 30 minuti, i dati arrivano senza aprirla (sincronizzazione periodica) | | |
| 15 | Notifiche locali (es. "Puoi compilare il rapporto") nella lingua del telefono | | |
| 16 | Ad app bloccata la notifica è solo il testo generico | | |
| 17 | Senza rete: si compila e si invia; tornata la rete il dato parte da solo | | |

## Interfaccia

| # | Prova | Android | iOS |
| --- | --- | --- | --- |
| 18 | **Foto** con fotocamera e galleria in un modulo che le prevede | | |
| 19 | **Rotazione** dello schermo: nessun contenuto tagliato, nessun dato perso nel modulo aperto | | |
| 20 | **Testo grande** (impostazione di accessibilità al massimo): tutto leggibile, pulsanti raggiungibili | | |
| 21 | Navigazione in basso, tasto indietro (Android) e gesto indietro (iOS) | | |
| 22 | Tema scuro del sistema | | |

## Problemi trovati

| Data | # | Descrizione | Stato |
| --- | --- | --- | --- |
