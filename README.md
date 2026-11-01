# App P2P per attività di organizzazioni

Nucleo astratto (Rust, `crates/`) + pacchetti di configurazione (`packages/`), uniti in fase
di build. Specifica completa: `PROGETTO.md`. Decisioni tecniche: `docs/DECISIONS.md`.

## Stato

| Fase | Stato |
| --- | --- |
| F0 — Fondamenta | fatta: workspace, crate, `xtask` (`schema`, `validate`, `bundle`, `check-domain-leak`), CI |
| F1 — Modello e configurazione | fatta: `core-model`, `core-config`, pacchetto `congregazione` |
| F2 — Eventi e crittografia | fatta: `core-crypto`, `core-events` (busta firmata, genesi, archivio SQLCipher) |
| F3 — Motore | fatto: `core-engine` (permessi sul passato causale, flussi, lotti, resoconti, programmi, territori, bacheca, promemoria, conflitti), `core-testkit`, 16 scenari |
| F4 — Sincronizzazione | fatta: `core-sync` (riconciliazione, ricevute, stato di consegna, abbinamento, allegati, trasporto iroh con gossip), simulatore `cargo xtask simulate` |
| F5–F10 | da fare |

## Comandi

```bash
cargo test --workspace
cargo xtask schema            # rigenera schemas/package.schema.json
cargo xtask validate --all    # valida ogni pacchetto
cargo xtask bundle --package <nome>
cargo xtask check-domain-leak
cargo xtask test-package --package <nome>   # scenari in packages/<nome>/tests
cargo run --release -p xtask -- simulate --package <nome> --devices 120 --days 60
```

Su macOS con l'SDK 27 installato accanto a un linker più vecchio, il link fallisce con
`tapi error: unknown architecture arm64e.x1-macos`. Soluzione locale:

```bash
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
```

## Riferimento grafico

Mockup delle prime schermate (desktop e mobile: Oggi, Rapporto del mese, Rapporti del gruppo):
canvas "Mockup app Congregazione" su claude.ai. Le viste generiche della fase F5 devono
riprodurne layout, colori (`#2952CC` primario) e tipografia (Inter).
