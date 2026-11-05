//! Texts: the package's (labels of everything configured) and the core's own
//! (generic words of the interface), with a small subset of ICU
//! MessageFormat: `{name}` placeholders and `{n, plural, one {...} other {...}}`.

use std::collections::BTreeMap;

use time::{Date, Month};

/// Generic interface texts of the core, per locale. Domain words never
/// appear here: they come from the package.
fn core_texts(locale: &str) -> &'static [(&'static str, &'static str)] {
    match locale {
        "it" => &[
            ("core.month.1", "gennaio"),
            ("core.month.2", "febbraio"),
            ("core.month.3", "marzo"),
            ("core.month.4", "aprile"),
            ("core.month.5", "maggio"),
            ("core.month.6", "giugno"),
            ("core.month.7", "luglio"),
            ("core.month.8", "agosto"),
            ("core.month.9", "settembre"),
            ("core.month.10", "ottobre"),
            ("core.month.11", "novembre"),
            ("core.month.12", "dicembre"),
            ("core.weekday.1", "lunedì"),
            ("core.weekday.2", "martedì"),
            ("core.weekday.3", "mercoledì"),
            ("core.weekday.4", "giovedì"),
            ("core.weekday.5", "venerdì"),
            ("core.weekday.6", "sabato"),
            ("core.weekday.7", "domenica"),
            ("core.week_of", "settimana del {date}"),
            ("core.due_by", "Da inviare entro il {date}"),
            ("core.greeting", "Buongiorno, {name}"),
            ("core.delivery.pending", "In attesa di sincronizzazione"),
            ("core.delivery.stored", "Sul server"),
            ("core.delivery.sent", "Inviato"),
            ("core.delivery.delivered", "Ricevuto"),
            ("core.readers", "Lo vedranno: {readers}"),
            (
                "core.missing_count",
                "{n, plural, one {Manca #} other {Mancano #}}",
            ),
            ("core.received_count", "{n} di {total} ricevuti"),
            ("core.incomplete_from", "Con mancanti, possibile dal {date}"),
            (
                "core.resource_no_log",
                "Ricordati di registrare la tua attività",
            ),
            (
                "core.unsynced",
                "{n, plural, one {# evento da inviare} other {# eventi da inviare}}",
            ),
            ("core.total", "Totale"),
            ("core.menu.config", "Configurazione"),
            ("core.menu.schedule", "Calendario e turni"),
            ("core.menu.profile", "Il mio profilo"),
            ("core.work.open", "Apri"),
            ("core.work.fill", "Compila"),
            (
                "core.work.alerts",
                "{n, plural, one {# avviso da vedere} other {# avvisi da vedere}}",
            ),
            (
                "core.work.alerts_hint",
                "Situazioni da sistemare nell'organizzazione",
            ),
            ("core.work.pairing", "Un dispositivo chiede di unirsi"),
            (
                "core.work.pairing_hint",
                "{name}: confronta il codice e conferma",
            ),
            ("core.work.aggregate", "Pronto da preparare"),
            ("core.work.aggregate_send", "Pronto da inviare"),
            (
                "core.work.program_publish",
                "Da verificare e pubblicare in bacheca",
            ),
        ],
        _ => &[
            ("core.month.1", "January"),
            ("core.month.2", "February"),
            ("core.month.3", "March"),
            ("core.month.4", "April"),
            ("core.month.5", "May"),
            ("core.month.6", "June"),
            ("core.month.7", "July"),
            ("core.month.8", "August"),
            ("core.month.9", "September"),
            ("core.month.10", "October"),
            ("core.month.11", "November"),
            ("core.month.12", "December"),
            ("core.weekday.1", "Monday"),
            ("core.weekday.2", "Tuesday"),
            ("core.weekday.3", "Wednesday"),
            ("core.weekday.4", "Thursday"),
            ("core.weekday.5", "Friday"),
            ("core.weekday.6", "Saturday"),
            ("core.weekday.7", "Sunday"),
            ("core.week_of", "week of {date}"),
            ("core.due_by", "Due by {date}"),
            ("core.greeting", "Hello, {name}"),
            ("core.delivery.pending", "Waiting to sync"),
            ("core.delivery.stored", "On the server"),
            ("core.delivery.sent", "Sent"),
            ("core.delivery.delivered", "Received"),
            ("core.readers", "Visible to: {readers}"),
            (
                "core.missing_count",
                "{n, plural, one {# missing} other {# missing}}",
            ),
            ("core.received_count", "{n} of {total} received"),
            ("core.incomplete_from", "With missing ones, from {date}"),
            ("core.resource_no_log", "Remember to log your work"),
            (
                "core.unsynced",
                "{n, plural, one {# event to send} other {# events to send}}",
            ),
            ("core.total", "Total"),
            ("core.menu.config", "Configuration"),
            ("core.menu.schedule", "Calendar and rotations"),
            ("core.menu.profile", "My profile"),
            ("core.work.open", "Open"),
            ("core.work.fill", "Fill in"),
            (
                "core.work.alerts",
                "{n, plural, one {# alert to review} other {# alerts to review}}",
            ),
            ("core.work.alerts_hint", "Things to fix in the organization"),
            ("core.work.pairing", "A device asks to join"),
            (
                "core.work.pairing_hint",
                "{name}: compare the code and confirm",
            ),
            ("core.work.aggregate", "Ready to prepare"),
            ("core.work.aggregate_send", "Ready to send"),
            (
                "core.work.program_publish",
                "To check and publish on the board",
            ),
        ],
    }
}

#[derive(Debug, Clone, Default)]
pub struct Texts {
    pub locale: String,
    map: BTreeMap<String, String>,
}

impl Texts {
    pub fn new(locale: &str, package: Option<&BTreeMap<String, String>>) -> Self {
        let mut map: BTreeMap<String, String> = core_texts(locale)
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        if let Some(p) = package {
            map.extend(p.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        Self {
            locale: locale.to_owned(),
            map,
        }
    }

    /// The text for `key`, or the key itself when missing.
    pub fn get(&self, key: &str) -> String {
        self.map.get(key).cloned().unwrap_or_else(|| key.to_owned())
    }

    /// The text for `key`, or `fallback` when missing.
    pub fn get_or(&self, key: &str, fallback: &str) -> String {
        self.map
            .get(key)
            .cloned()
            .unwrap_or_else(|| fallback.to_owned())
    }

    pub fn fmt(&self, key: &str, params: &[(&str, String)]) -> String {
        format(&self.get(key), params)
    }

    pub fn all(&self) -> &BTreeMap<String, String> {
        &self.map
    }

    pub fn month_name(&self, month: Month) -> String {
        self.get(&format!("core.month.{}", u8::from(month)))
    }

    /// `5 novembre` / `November 5`.
    pub fn day_month(&self, d: Date) -> String {
        if self.locale == "it" {
            format!("{} {}", d.day(), self.month_name(d.month()))
        } else {
            format!("{} {}", self.month_name(d.month()), d.day())
        }
    }

    /// `03/11/1980` / `11/03/1980`.
    pub fn short_date(&self, d: Date) -> String {
        let (day, month) = (d.day(), u8::from(d.month()));
        if self.locale == "en" {
            format!("{month:02}/{day:02}/{}", d.year())
        } else {
            format!("{day:02}/{month:02}/{}", d.year())
        }
    }

    /// `domenica 1 novembre`.
    pub fn long_date(&self, d: Date) -> String {
        let wd = self.get(&format!(
            "core.weekday.{}",
            d.weekday().number_from_monday()
        ));
        format!("{wd} {}", self.day_month(d))
    }

    pub fn period(&self, p: &core_engine::time::PeriodKey) -> String {
        use core_engine::time::PeriodKey as P;
        match p {
            P::Once => String::new(),
            P::Month { month, year } => match Month::try_from(*month) {
                Ok(m) => format!("{} {year}", self.month_name(m)),
                Err(_) => p.to_string(),
            },
            P::Week { monday } => self.fmt("core.week_of", &[("date", self.day_month(*monday))]),
        }
    }
}

/// Replaces `{name}` and resolves `{name, plural, one {..} other {..}}` (with `#`).
pub fn format(template: &str, params: &[(&str, String)]) -> String {
    let lookup = |name: &str| {
        params
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.clone())
    };
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // Find the matching brace.
        let mut depth = 0;
        let mut j = i;
        while j < chars.len() {
            match chars[j] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let inner: String = chars[i + 1..j.min(chars.len())].iter().collect();
        i = j + 1;
        let mut parts = inner.splitn(3, ',');
        let name = parts.next().unwrap_or_default().trim();
        match (parts.next().map(str::trim), parts.next()) {
            (Some("plural"), Some(rest)) => {
                let n: i64 = lookup(name).and_then(|v| v.parse().ok()).unwrap_or(0);
                let choose = if n == 1 { "one" } else { "other" };
                let body = plural_branch(rest, choose)
                    .or_else(|| plural_branch(rest, "other"))
                    .unwrap_or_default();
                out.push_str(&format(&body.replace('#', &n.to_string()), params));
            }
            _ => out.push_str(&lookup(name).unwrap_or_else(|| format!("{{{name}}}"))),
        }
    }
    out
}

fn plural_branch(rest: &str, which: &str) -> Option<String> {
    let start = rest.find(which)? + which.len();
    let open = rest[start..].find('{')? + start;
    let mut depth = 0;
    for (k, c) in rest[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[open + 1..open + k].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_and_plurals() {
        assert_eq!(
            format("Ciao {name}", &[("name", "Anna".into())]),
            "Ciao Anna"
        );
        let t = "{n, plural, one {Manca #} other {Mancano #}}";
        assert_eq!(format(t, &[("n", "1".into())]), "Manca 1");
        assert_eq!(format(t, &[("n", "3".into())]), "Mancano 3");
        assert_eq!(format("x {missing} y", &[]), "x {missing} y");
        let nested = "Gruppo {unit}: ne {missing, plural, one {manca # } other {mancano #}}";
        assert_eq!(
            format(nested, &[("unit", "3".into()), ("missing", "2".into())]),
            "Gruppo 3: ne mancano 2"
        );
    }

    #[test]
    fn dates() {
        let t = Texts::new("it", None);
        let d = core_engine::time::parse_date("2026-11-01").unwrap_or(Date::MIN);
        assert_eq!(t.long_date(d), "domenica 1 novembre");
        assert_eq!(
            t.period(&core_engine::time::PeriodKey::month(2026, 10)),
            "ottobre 2026"
        );
    }
}
