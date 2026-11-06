#!/usr/bin/env bash
# Test data for the dev server (cargo devserver, port 8787) with the
# congregazione package: an organization, a few people with dates,
# qualifications and groups. Run on empty data (target/dev-data).
set -euo pipefail
API=${API:-http://localhost:8787/api}
c(){ curl -sf -X POST "$API/$1" -H 'content-type: application/json' -d "$2"; }
g(){ c grant_qualification "{\"person\":\"$1\",\"qualification\":\"$2\",\"from\":${3:-null},\"until\":null}" >/dev/null; }

c accept_privacy '{}' >/dev/null
c create_org '{"name":"Congregazione Centro","myName":"Monti Francesco"}' >/dev/null
me=$(c people '{}' | python3 -c "import sys,json; print(json.load(sys.stdin)['persons'][0]['person_id'])")
c update_person "{\"person\":\"$me\",\"name\":null,\"attributes\":{\"sex\":\"m\",\"birth_date\":\"1982-04-14\",\"hope\":\"other_sheep\"}}" >/dev/null
g "$me" publisher '"1995-01-01"'; g "$me" baptized '"1996-07-20"'; g "$me" ministerial_servant '"2010-03-01"'
for q in can_av can_microphones can_attendant can_cleaning can_prayer can_bible_reading; do g "$me" $q '"2015-09-01"'; done
c grant_role "{\"person\":\"$me\",\"role\":\"av_manager\",\"unit\":null}" >/dev/null
c grant_role "{\"person\":\"$me\",\"role\":\"attendant_manager\",\"unit\":null}" >/dev/null
c grant_role "{\"person\":\"$me\",\"role\":\"cleaning_manager\",\"unit\":null}" >/dev/null
u=$(c create_unit '{"unitType":"group","name":"Gruppo Centro"}' | tr -d '"')
u2=$(c create_unit '{"unitType":"group","name":"Gruppo Nord"}' | tr -d '"')
c set_membership "{\"person\":\"$me\",\"unit\":\"$u\",\"from\":\"2026-01-01\"}" >/dev/null

# name|sex|birth|hope|baptism|qualifications from 2015-09-01|group (c/n)
while IFS='|' read -r name sx birth hope bapt extra unit; do
  id=$(c add_person "{\"name\":\"$name\",\"attributes\":{\"sex\":\"$sx\",\"birth_date\":\"$birth\",\"hope\":\"$hope\"}}" | tr -d '"')
  g "$id" publisher '"2000-01-01"'
  [ -n "$bapt" ] && g "$id" baptized "\"$bapt\""
  for q in $extra; do g "$id" "$q" '"2015-09-01"'; done
  c set_membership "{\"person\":\"$id\",\"unit\":\"$([ "$unit" = n ] && echo "$u2" || echo "$u")\",\"from\":\"2026-01-01\"}" >/dev/null
done <<'L'
Andreani Giovanni|m|1958-02-11|other_sheep|1974-08-03|elder can_lmm_chairman can_public_chairman can_prayer can_treasures can_gems can_living can_cbs_conductor can_public_talk can_wt_conductor|c
Bianchi Marco|m|1990-06-30|other_sheep|2006-04-22|ministerial_servant regular_pioneer can_av can_microphones can_prayer can_bible_reading can_cbs_reader can_wt_reader|c
Neri Anna|f|1971-11-05|other_sheep|1988-07-16|regular_pioneer can_student can_cleaning|n
Rossi Paolo|m|1949-01-19|anointed|1966-07-09|elder can_public_chairman can_public_talk can_prayer can_attendant can_wt_conductor can_lmm_chairman can_treasures can_gems|n
Gallo Sara|f|2009-03-12|other_sheep||can_student can_cleaning|n
Verdi Luca|m|1985-09-25|other_sheep|2001-05-12|special_pioneer can_attendant can_microphones can_av can_bible_reading can_student|c
Esposito Chiara|f|1994-02-08|other_sheep|2010-10-23|can_student can_cleaning|n
Romano Davide|m|1998-12-01|other_sheep|2014-04-05|can_attendant can_microphones can_bible_reading can_student can_prayer can_wt_reader can_cleaning|n
L
echo "ok: $(c people '{}' | python3 -c "import sys,json; print(len(json.load(sys.stdin)['persons']))") persone"

# Calendar and assignments as set up for testing (scripts/dev-config: meeting
# days and times, auxiliary class on, weekly rotations, the slots in use):
# published over the package before any program is made.
python3 - "$API" "$(dirname "$0")/dev-config" <<'PY'
import json, os, sys, urllib.request
api, cfg = sys.argv[1], sys.argv[2]
def call(cmd, body):
    r = urllib.request.Request(f"{api}/{cmd}", data=json.dumps(body).encode(), headers={"content-type": "application/json"})
    return json.load(urllib.request.urlopen(r, timeout=150))
files = call("config_files", {})["files"]
for f in files:
    local = os.path.join(cfg, f["path"])
    if os.path.isfile(local):
        f["text"] = open(local, encoding="utf-8").read()
print("configurazione", call("config_publish", {"files": files}))
PY

# Past months: reports (August registered, September forwarded; October is
# left to fill) and programs of September and October, filled and published.
python3 - "$API" <<'PY'
import json, random, sys, urllib.error, urllib.request
api = sys.argv[1]
random.seed(7)
def call(cmd, body):
    r = urllib.request.Request(f"{api}/{cmd}", data=json.dumps(body).encode(), headers={"content-type": "application/json"})
    try:
        return json.load(urllib.request.urlopen(r, timeout=300))
    except urllib.error.HTTPError as e:
        raise SystemExit(f"{cmd}: {e.read().decode()}")

people = call("people", {})["persons"]
for period in ("2026-08", "2026-09"):
    for p in people:
        v = call("form", {"form": "service_report", "subject": p["person_id"], "period": period})
        ids = {f["id"] for f in v["fields"]}
        values = {}
        if "hours" in ids: values["hours"] = random.choice([2700, 3000, 3300, 3600, 4200])
        if "participated" in ids: values["participated"] = random.random() > 0.1
        if "bible_studies" in ids: values["bible_studies"] = random.choice([0, 0, 1, 1, 2])
        v = call("save_document", {"form": "service_report", "subject": p["person_id"], "period": period, "values": values})
        call("step_document", {"document": v["document_id"], "step": "submit", "note": None})
    # The work queue lists only the current month: step each group's batch directly.
    for u in call("people", {})["units"]:
        call("batch_step", {"form": "service_report", "unit": u["id"], "period": period, "step": "forward"})
        if period == "2026-08":
            call("batch_step", {"form": "service_report", "unit": u["id"], "period": period, "step": "register"})
print("rapporti: agosto registrati, settembre inoltrati")

for ty in ("midweek", "weekend", "av", "attendants", "cleaning"):
    for period in ("2026-09", "2026-10"):
        d = call("program_dates", {"programType": ty, "period": period})
        if not d["dates"] or d["exists"]:
            continue
        pid = call("draft_program", {"programType": ty, "dates": d["dates"]})
        e = call("program", {"program": pid})
        if e["can_prefill"]:
            try:
                e = call("prefill_program", {"program": pid, "occurrences": None})
            except SystemExit:
                pass
        changes, used = [], {}
        for c in e["cells"]:
            if not (c["assignable"] and c["shown"] and c["candidates"]) or c["column"]:
                continue
            taken = used.setdefault(c["occurrence"], set())
            free = [o for o in c["candidates"] if o["id"] not in taken] or c["candidates"]
            pick = random.choice(free)["id"]
            taken.add(pick)
            changes.append({"occurrence": c["occurrence"], "slot": c["slot"], "index": c["index"], "person": pick})
        call("assign_slots", {"program": pid, "changes": changes, "details": [], "parts": []})
        call("send_program", {"program": pid})
        call("publish_program", {"program": pid})
print("programmi: settembre e ottobre pubblicati")
PY
