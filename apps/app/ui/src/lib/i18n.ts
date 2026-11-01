// Generic interface words of the core. Everything domain-specific comes
// from the package through the commands (labels already resolved).
const it = {
  save: 'Salva bozza', saved: 'Salvato', cancel: 'Annulla', confirm: 'Conferma', back: 'Indietro', close: 'Chiudi',
  yes: 'Sì', no: 'No', fill: 'Compila', fill_on_behalf: 'Compila per conto', respond: 'Rispondi', accept: 'Accetto',
  decline: 'Non posso', open: 'Apri', todo: 'Da fare', board_news: 'Novità in bacheca', nothing: 'Niente da fare per ora',
  missing: 'Mancano', received: 'Ricevuti', done: 'Già avanzati', note: 'Nota', note_required: 'Scrivi una nota',
  create_org: 'Crea una nuova organizzazione', org_name: "Nome dell'organizzazione", your_name: 'Il tuo nome',
  join_org: 'Unisciti con un invito', ticket: "Incolla il codice d'invito", join: 'Unisciti', create: 'Crea',
  code_check: 'Controlla che questo codice sia uguale sul dispositivo che ti ha invitato', waiting: 'In attesa di conferma…',
  locked: 'App bloccata', pin: 'PIN', unlock: 'Sblocca', wrong_pin: 'PIN errato', privacy: 'Informativa sulla privacy',
  privacy_accept: 'Ho letto', welcome: 'Benvenuto', synced: 'Sincronizzato', unsynced: 'Da sincronizzare',
  sync_now: 'Sincronizza ora', events: 'Eventi', outbox: 'In uscita', last_sync: 'Ultima sincronizzazione', peers: 'Dispositivi',
  network: 'Rete', archive: 'archivio', invite: 'Invita un dispositivo', invite_hint: "Inquadra il codice QR o copia l'invito sul nuovo dispositivo. Scade alle",
  pairing_requests: 'Richieste di abbinamento', pairing_confirm: 'Il codice coincide', pairing_refuse: 'Rifiuta',
  generate: 'Genera', distribute: 'Distribuisci', version: 'Versione', distributed: 'Distribuito', not_distributed: 'Non ancora distribuito',
  registered: 'Documenti registrati', period: 'Periodo', none_visible: 'Nessun dato visibile',
  new_program: 'Nuovo programma', dates: 'Date (una per riga, AAAA-MM-GG)', send: 'Invia le assegnazioni', publish: 'Pubblica in bacheca',
  status_draft: 'Bozza', status_assigned: 'Assegnato', status_published: 'Pubblicato', unassigned: '— non assegnato —',
  response_pending: 'In attesa', response_accepted: 'Accettato', response_declined: 'Declinato', add_external: 'Aggiungi persona esterna',
  available: 'Disponibile', assigned: 'Assegnata', holder: 'Detentore', history: 'Storico', add: 'Aggiungi', assign: 'Assegna',
  request: 'Richiedi', log: 'Registra', return: 'Restituisci', new_resource: 'Nuova risorsa',
  people: 'Persone', add_person: 'Aggiungi persona', name: 'Nome', qualification: 'Qualifica', role: 'Incarico', unit: 'Unità',
  from: 'Dal', set_unit: 'Imposta unità', grant: 'Conferisci', devices: 'Dispositivi', new_unit: 'Nuova unità', type: 'Tipo',
  members: 'Membri', alerts: 'Avvisi', no_alerts: 'Nessun avviso', settings: 'Impostazioni', auto_lock: 'Blocco automatico (minuti)',
  set_pin: 'Imposta PIN', change_pin: 'Cambia PIN', remove_pin: 'Rimuovi PIN',
  pin_hint: "Con il PIN i dati su questo dispositivo sono cifrati e si aprono solo con il PIN. Mentre l'app è bloccata continua a ricevere gli aggiornamenti, che vedrai dopo lo sblocco. Se dimentichi il PIN, abbina di nuovo il dispositivo: i dati tornano dagli altri dispositivi.", mobile_photos: 'Scarica foto anche su rete mobile', muted: 'Notifiche silenziate',
  error: 'Errore', err_denied: 'Operazione non consentita in questo momento', err_not_found: 'Elemento non trovato',
  err_invalid: 'Dati non validi', err_locked: "L'app è bloccata", err_network: 'Problema di rete, riprova più tardi',
  err_internal: 'Si è verificato un errore', menu: 'Menu', more: 'Altro', lock_now: 'Blocca ora', copy: 'Copia', copied: 'Copiato'
};

export type Key = keyof typeof it;
export const t = (k: Key): string => it[k];
