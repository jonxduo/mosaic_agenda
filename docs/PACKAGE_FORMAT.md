# Formato dei pacchetti

Riferimento normativo: `schemas/package.schema.json` (generato da `crates/core-model`, una
definizione `$defs` per file) e PROGETTO.md §5–§6. Qui solo le regole che lo schema non dice.

## File

`package.yaml` più 13 file obbligatori (anche vuoti): `qualifications`, `units`, `roles`,
`audiences`, `forms`, `workflows`, `aggregates`, `calendar`, `programs`, `resources`, `board`,
`notifications`, `ui`. Inoltre `texts/<locale>.yaml` per ogni lingua e l'informativa indicata da
`privacy_notice` (per le altre lingue, stesso percorso con il nome della lingua).

## Regole del loader

- Ancore e alias YAML valgono nel file in cui sono definiti; le chiavi di merge `<<` sono espanse.
- Le chiavi di primo livello che iniziano con `x-` sono ignorate (contenitori di ancore).
- Un file vuoto equivale a `{}`.
- I testi possono essere piatti (`q.elder: ...`) o annidati (`q: { elder: ... }`).

## Convenzioni

| Elemento senza `label` | Chiave di testo usata |
| --- | --- |
| Campo, attributo persona | `fld.<id>` |
| Opzione di scelta, risposta a una richiesta | `opt.<id>` |
| Misura di un'aggregazione | `msr.<id>` |
| Passaggio non automatico di un flusso | `step.<id>` |

Opzioni: `[a, b]` oppure `[{ id: a, label: chiave }]`.

Pubblici di sistema (non ridefinibili): `subject_only`, `subject_or_on_behalf`,
`subject_missing`, `assignee`, `resource_holder`, `object_team`, `root_admins`.

Combinatori di pubblico: `union: [...]`, `intersect: [...]`, `except: [base, tolti...]`.

## Validazione

`cargo xtask validate --package <nome>`. Ogni errore indica file, percorso YAML e messaggio,
per esempio `error: roles.yaml: roles[0].requires.has_qualification: unknown qualification `x``.
