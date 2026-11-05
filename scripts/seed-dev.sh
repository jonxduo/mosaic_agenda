#!/usr/bin/env bash
# Test data for the dev server (cargo devserver, port 8787) with the
# congregazione package: an organization, a few people with dates,
# qualifications and groups. Run on empty data (target/dev-data).
set -euo pipefail
API=${API:-http://localhost:8787/api}
c(){ curl -sf -X POST "$API/$1" -H 'content-type: application/json' -d "$2"; }
g(){ c grant_qualification "{\"person\":\"$1\",\"qualification\":\"$2\",\"from\":${3:-null},\"until\":null}" >/dev/null; }

c accept_privacy '{}' >/dev/null
c create_org '{"name":"Congregazione Centro","myName":"Francesco Monti"}' >/dev/null
me=$(c people '{}' | python3 -c "import sys,json; print(json.load(sys.stdin)['persons'][0]['person_id'])")
c update_person "{\"person\":\"$me\",\"name\":null,\"attributes\":{\"sex\":\"m\",\"birth_date\":\"1982-04-14\",\"hope\":\"other_sheep\"}}" >/dev/null
g "$me" publisher '"1995-01-01"'; g "$me" baptized '"1996-07-20"'; g "$me" ministerial_servant '"2010-03-01"'
for q in can_av can_microphones can_attendant can_prayer can_bible_reading; do g "$me" $q '"2015-09-01"'; done
c grant_role "{\"person\":\"$me\",\"role\":\"av_manager\",\"unit\":null}" >/dev/null
c grant_role "{\"person\":\"$me\",\"role\":\"attendant_manager\",\"unit\":null}" >/dev/null
u=$(c create_unit '{"unitType":"group","name":"Gruppo Centro"}' | tr -d '"')
u2=$(c create_unit '{"unitType":"group","name":"Gruppo Nord"}' | tr -d '"')

# name|sex|birth|hope|baptism|qualifications from 2015-09-01|group (c/n)
while IFS='|' read -r name sx birth hope bapt extra unit; do
  id=$(c add_person "{\"name\":\"$name\",\"attributes\":{\"sex\":\"$sx\",\"birth_date\":\"$birth\",\"hope\":\"$hope\"}}" | tr -d '"')
  g "$id" publisher '"2000-01-01"'
  [ -n "$bapt" ] && g "$id" baptized "\"$bapt\""
  for q in $extra; do g "$id" "$q" '"2015-09-01"'; done
  c set_membership "{\"person\":\"$id\",\"unit\":\"$([ "$unit" = n ] && echo "$u2" || echo "$u")\",\"from\":null}" >/dev/null
done <<'L'
Andreani Giovanni|m|1958-02-11|other_sheep|1974-08-03|elder can_lmm_chairman can_public_chairman can_prayer can_treasures can_gems can_living can_cbs_conductor can_public_talk can_wt_conductor|c
Bianchi Marco|m|1990-06-30|other_sheep|2006-04-22|ministerial_servant regular_pioneer can_av can_microphones can_prayer can_bible_reading can_cbs_reader can_wt_reader|c
Neri Anna|f|1971-11-05|other_sheep|1988-07-16|regular_pioneer can_student|n
Rossi Paolo|m|1949-01-19|anointed|1966-07-09|elder can_public_chairman can_public_talk can_prayer can_attendant can_wt_conductor can_lmm_chairman can_treasures can_gems|n
Gallo Sara|f|2009-03-12|other_sheep||can_student|n
Verdi Luca|m|1985-09-25|other_sheep|2001-05-12|special_pioneer can_attendant can_microphones can_av can_bible_reading can_student|c
Esposito Chiara|f|1994-02-08|other_sheep|2010-10-23|can_student|n
Romano Davide|m|1998-12-01|other_sheep|2014-04-05|can_attendant can_microphones can_bible_reading can_student can_prayer can_wt_reader|n
L
echo "ok: $(c people '{}' | python3 -c "import sys,json; print(len(json.load(sys.stdin)['persons']))") persone"
