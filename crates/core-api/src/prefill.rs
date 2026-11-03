//! Details of the slots read from a web page per event (`prefill` of a
//! program type): the page's outline — headings with the text below them —
//! goes through the package's rules. Only empty details are filled.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use core_engine::ops::{SlotKey, Values};
use core_model::config::{FieldType, PrefillDef, PrefillRule, ProgramTypeDef, Repeat};
use core_model::{Scalar, Slug};
use regex::Regex;
use time::Date;

use crate::dto::ProgramEditor;
use crate::views::cfg;
use crate::{ApiError, ApiResult, Core, parse_id};

/// A heading of a page, with its enclosing heading and the text below it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    pub section: String,
    pub heading: String,
    pub text: String,
}

/// A page reduced to what the rules look at.
#[derive(Debug, Default)]
pub struct Page {
    pub outline: Vec<OutlineItem>,
    /// `(href, text)` of every link, in order.
    pub links: Vec<(String, String)>,
}

/// Reads headings (`h1`…`h6`), text and links out of HTML. Not a full
/// parser: enough for pages built of headings and paragraphs.
pub fn parse_page(html: &str) -> Page {
    let mut page = Page::default();
    // Open headings by level, for the section of each new one.
    let mut open: [String; 7] = Default::default();
    let mut current: Option<(usize, String, String)> = None; // level, section, heading
    let mut heading: Option<(usize, String)> = None;
    let mut link: Option<(String, String)> = None;
    let mut text = String::new();
    let mut rest = html;
    let mut skip_until: Option<&str> = None;

    let flush =
        |current: &mut Option<(usize, String, String)>, text: &mut String, page: &mut Page| {
            if let Some((_, section, h)) = current.take() {
                page.outline.push(OutlineItem {
                    section,
                    heading: h,
                    text: tidy_lines(text),
                });
            }
            text.clear();
        };

    while let Some(lt) = rest.find('<') {
        let chunk = &rest[..lt];
        rest = &rest[lt..];
        if skip_until.is_none() {
            let t = decode(chunk);
            if let Some((_, h)) = heading.as_mut() {
                h.push_str(&t);
            } else {
                text.push_str(&t);
            }
            if let Some((_, l)) = link.as_mut() {
                l.push_str(&t);
            }
        }
        if rest.starts_with("<!--") {
            rest = rest.find("-->").map_or("", |e| &rest[e + 3..]);
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(end) = skip_until {
            if closing && name == end {
                skip_until = None;
            }
            continue;
        }
        match (name.as_str(), closing) {
            ("script", false) => skip_until = Some("script"),
            ("style", false) => skip_until = Some("style"),
            ("a", false) => link = Some((attr(tag, "href").unwrap_or_default(), String::new())),
            ("a", true) => {
                if let Some((href, t)) = link.take() {
                    page.links.push((href, squash(&t)));
                }
            }
            (h, false) if is_heading(h) => {
                flush(&mut current, &mut text, &mut page);
                heading = Some((level(h), String::new()));
            }
            (h, true) if is_heading(h) => {
                if let Some((lv, t)) = heading.take() {
                    let t = squash(&t);
                    let section = (1..lv)
                        .rev()
                        .find_map(|l| open.get(l).filter(|s| !s.is_empty()).cloned())
                        .unwrap_or_default();
                    for slot in open.iter_mut().skip(lv) {
                        slot.clear();
                    }
                    if let Some(slot) = open.get_mut(lv) {
                        slot.clone_from(&t);
                    }
                    current = Some((lv, section, t));
                }
            }
            ("p" | "div" | "li" | "br" | "tr" | "ul" | "ol" | "section" | "figure", _)
                if heading.is_none() =>
            {
                text.push('\n');
            }
            _ => {}
        }
    }
    flush(&mut current, &mut text, &mut page);
    page
}

fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn level(name: &str) -> usize {
    name.get(1..).and_then(|n| n.parse().ok()).unwrap_or(6)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))?;
    let v = &tag[at + name.len() + 2..];
    Some(decode(&v[..v.find('"')?]))
}

/// Whitespace collapsed to single spaces.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Non-empty lines, each squashed.
fn tidy_lines(s: &str) -> String {
    s.lines()
        .map(squash)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';').filter(|i| *i <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The address for one event.
pub fn url_for(template: &str, date: Date) -> String {
    let (iso_year, week, _) = date.to_iso_week_date();
    template
        .replace("{year}", &iso_year.to_string())
        .replace("{week}", &week.to_string())
        .replace("{month}", &u8::from(date.month()).to_string())
        .replace("{day}", &date.day().to_string())
        .replace(
            "{date}",
            &format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                u8::from(date.month()),
                date.day()
            ),
        )
}

/// An absolute address for `href` found on `base`.
pub fn join(base: &str, href: &str) -> String {
    if href.starts_with("https://") || href.starts_with("http://") {
        return href.to_owned();
    }
    let origin_end = base
        .find("://")
        .and_then(|s| base[s + 3..].find('/').map(|e| s + 3 + e))
        .unwrap_or(base.len());
    if href.starts_with('/') {
        format!("{}{href}", &base[..origin_end])
    } else {
        let dir = base.rfind('/').map_or(base, |i| &base[..=i]);
        format!("{dir}{href}")
    }
}

/// The first link of the page whose text matches `follow`.
pub fn follow_link(page: &Page, follow: &str) -> Option<String> {
    let re = Regex::new(follow).ok()?;
    page.links
        .iter()
        .find(|(href, text)| !href.is_empty() && re.is_match(text))
        .map(|(href, _)| href.clone())
}

struct Compiled<'a> {
    rule: &'a PrefillRule,
    section: Option<Regex>,
    heading: Regex,
    text: Option<Regex>,
}

fn compile(rules: &[PrefillRule]) -> Vec<Compiled<'_>> {
    rules
        .iter()
        .filter_map(|r| {
            Some(Compiled {
                rule: r,
                section: match &r.section {
                    Some(s) => Some(Regex::new(s).ok()?),
                    None => None,
                },
                heading: Regex::new(&r.heading).ok()?,
                text: match &r.text {
                    Some(s) => Some(Regex::new(s).ok()?),
                    None => None,
                },
            })
        })
        .collect()
}

/// The details found in one event's outline: `(slot, position) → values`.
pub fn details_from(
    def: &ProgramTypeDef,
    rules: &[PrefillRule],
    outline: &[OutlineItem],
) -> Vec<(Slug, u16, Values)> {
    let rules = compile(rules);
    let mut next: BTreeMap<Slug, u16> = BTreeMap::new();
    let mut out = Vec::new();
    for item in outline {
        for c in &rules {
            let mut groups: BTreeMap<String, String> = BTreeMap::new();
            let mut take = |re: &Regex, s: &str| -> bool {
                let Some(caps) = re.captures(s) else {
                    return false;
                };
                for name in re.capture_names().flatten() {
                    if let Some(m) = caps.name(name) {
                        groups.insert(name.to_owned(), m.as_str().trim().to_owned());
                    }
                }
                true
            };
            let matched = c.section.as_ref().is_none_or(|re| take(re, &item.section))
                && take(&c.heading, &item.heading)
                && c.text.as_ref().is_none_or(|re| take(re, &item.text));
            if !matched {
                continue;
            }
            // The first matching rule takes the heading.
            let Some(slot_id) = &c.rule.slot else { break };
            let Some(slot) = def.slots.iter().find(|s| &s.id == slot_id) else {
                break;
            };
            let reps = match &slot.repeat {
                None => 1,
                Some(Repeat::Count(n)) => *n,
                Some(Repeat::Field(_)) => 50,
            };
            let capacity = slot.count.saturating_mul(reps);
            let index = next.entry(slot_id.clone()).or_default();
            if u32::from(*index) < capacity {
                let mut values = Values::new();
                for (field, template) in &c.rule.set {
                    // A group that took no part (optional) leaves its place empty.
                    let mut v = template.clone();
                    for name in core_config::template_names(template) {
                        let value = groups.get(&name).map_or("", String::as_str);
                        v = v.replace(&format!("{{{name}}}"), value);
                    }
                    let ty = slot
                        .extra_fields
                        .iter()
                        .find(|f| &f.id == field)
                        .map(|f| f.ty);
                    if let Some(s) = scalar(ty, v.trim()) {
                        values.insert(field.clone(), s);
                    }
                }
                if !values.is_empty() {
                    out.push((slot_id.clone(), *index, values));
                }
                *index += 1;
            }
            break;
        }
    }
    out
}

/// Text read from a page as the value of a field of type `ty`.
pub(crate) fn scalar(ty: Option<FieldType>, v: &str) -> Option<Scalar> {
    if v.is_empty() {
        return None;
    }
    match ty {
        Some(FieldType::Integer) => v.parse().ok().map(Scalar::Int),
        Some(FieldType::Decimal) => v.replace(',', ".").parse().ok().map(Scalar::Num),
        _ => Some(Scalar::Str(v.to_owned())),
    }
}

async fn fetch(client: &reqwest::Client, url: &str) -> ApiResult<String> {
    let r = client
        .get(url)
        .send()
        .await
        .map_err(|e| ApiError::new("network", e.to_string()))?;
    if !r.status().is_success() {
        return Err(ApiError::new("network", format!("{url}: {}", r.status())));
    }
    r.text()
        .await
        .map_err(|e| ApiError::new("network", e.to_string()))
}

/// The outline of the page for one event, following `follow` if set.
async fn outline_for(
    client: &reqwest::Client,
    pf: &PrefillDef,
    date: Date,
) -> ApiResult<Vec<OutlineItem>> {
    let url = url_for(&pf.url, date);
    let page = parse_page(&fetch(client, &url).await?);
    let Some(follow) = &pf.follow else {
        return Ok(page.outline);
    };
    let href = follow_link(&page, follow).ok_or_else(|| ApiError::not_found("link to follow"))?;
    Ok(parse_page(&fetch(client, &join(&url, &href)).await?).outline)
}

impl Core {
    /// Fills the empty details of a program from its type's `prefill`
    /// pages; events whose page cannot be read are left as they are.
    pub async fn prefill_program(self: &Arc<Self>, program: String) -> ApiResult<ProgramEditor> {
        let id = parse_id(&program)?;
        let (def, occurrences, filled) = {
            let v = self.view()?;
            let c = cfg(&v)?;
            let p = v
                .proj
                .programs
                .get(&id)
                .ok_or_else(|| ApiError::not_found("program"))?;
            let def = c
                .program_type(&p.program_type)
                .ok_or_else(|| ApiError::not_found("program type"))?
                .clone();
            (def, p.occurrences.clone(), p.details.clone())
        };
        let Some(pf) = def.prefill.clone() else {
            return self.program(program);
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("Mozilla/5.0")
            .build()
            .map_err(ApiError::internal)?;
        let mut details: Vec<(SlotKey, Values)> = Vec::new();
        let mut read = 0;
        for (occ, date) in occurrences.iter().enumerate() {
            let Ok(outline) = outline_for(&client, &pf, *date).await else {
                continue;
            };
            read += 1;
            let occurrence = u16::try_from(occ).map_err(ApiError::invalid)?;
            for (slot, index, values) in details_from(&def, &pf.rules, &outline) {
                let key = SlotKey {
                    occurrence,
                    slot,
                    index,
                };
                // Never over what someone already wrote.
                let mut merged = filled.get(&key).cloned().unwrap_or_default();
                let before = merged.len();
                for (f, v) in values {
                    merged.entry(f).or_insert(v);
                }
                if merged.len() > before {
                    details.push((key, merged));
                }
            }
        }
        if read == 0 && !occurrences.is_empty() {
            return Err(ApiError::new("network", "the pages could not be read"));
        }
        if !details.is_empty() {
            self.act_events(|c| c.set_program_details(id, details))?;
        }
        self.program(program)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head><script>var x = "<h2>no</h2>";</script></head><body>
<h1>2-8 NOVEMBRE</h1>
<h2><a href="/b/1">BOOK 49-50</a></h2>
<h2><strong>FIRST PART</strong></h2>
<h3><strong>1. Help others &amp; more</strong></h3>
<div><p>(10 min)</p></div><p>Some text</p>
<h3><strong>3. Reading</strong></h3>
<p>(4 min) <a href="/x">Jer 50:24-40</a> (<em>th</em> lesson 11)</p>
<h2><strong>SECOND PART</strong></h2>
<h3>4. Start a conversation</h3><p>(3 min) HOUSE TO HOUSE.</p>
<h3>5. Follow up</h3><p>(4 min) INFORMAL.</p>
<h3>6. Make disciples</h3><p>(5 min)</p>
<h3>7. Talk</h3><p>(5 min)</p>
<h2>Next</h2><a href="/d/r6/lp-i/2026560">Trust in God</a>
</body></html>"#;

    #[test]
    fn outline_has_sections_headings_and_text() {
        let p = parse_page(PAGE);
        let h: Vec<_> = p
            .outline
            .iter()
            .map(|i| (i.section.as_str(), i.heading.as_str()))
            .collect();
        assert!(
            h.contains(&("FIRST PART", "1. Help others & more")),
            "{h:?}"
        );
        assert!(h.contains(&("SECOND PART", "5. Follow up")), "{h:?}");
        assert!(!h.iter().any(|(_, x)| *x == "no"), "scripts are skipped");
        let reading = p
            .outline
            .iter()
            .find(|i| i.heading == "3. Reading")
            .unwrap();
        assert_eq!(reading.text, "(4 min) Jer 50:24-40 (th lesson 11)");
        assert_eq!(
            follow_link(&p, "^Trust").map(|h| join("https://example.org/a/b", &h)),
            Some("https://example.org/d/r6/lp-i/2026560".into())
        );
    }

    #[test]
    fn rules_fill_slots_in_order() {
        let yaml = r#"
id: weekly
label: p.weekly
event_type: session
period: week
owner_role: lead
publish_audience: all
slots:
  - { id: talk, label: s.talk, extra_fields: [ { id: title, type: text } ] }
  - { id: reading, label: s.reading, extra_fields: [ { id: title, type: text } ] }
  - { id: part, label: s.part, repeat: { count: 2 },
      extra_fields: [ { id: title, type: text }, { id: minutes, type: integer } ] }
"#;
        let def: ProgramTypeDef = serde_norway_like(yaml);
        let rules: Vec<PrefillRule> = serde_norway_like(
            r#"
- { section: "FIRST", heading: '^1\. (?<title>.+)$', slot: talk, set: { title: "{title}" } }
- { section: "FIRST", heading: 'Reading', text: '\(\d+ min\) (?<p>[^(]+?) \(', slot: reading, set: { title: "{p}" } }
- { section: "SECOND", heading: '^\d+\. (?<title>.+)$', text: '\((?<m>\d+) min\)', slot: part, set: { title: "{title}", minutes: "{m}" } }
"#,
        );
        let found = details_from(&def, &rules, &parse_page(PAGE).outline);
        let get = |slot: &str, i: u16, f: &str| {
            found
                .iter()
                .find(|(s, x, _)| s.as_str() == slot && *x == i)
                .and_then(|(_, _, v)| v.get(&Slug::new(f).unwrap()).cloned())
        };
        assert_eq!(
            get("talk", 0, "title"),
            Some(Scalar::Str("Help others & more".into()))
        );
        assert_eq!(
            get("reading", 0, "title"),
            Some(Scalar::Str("Jer 50:24-40".into()))
        );
        assert_eq!(
            get("part", 1, "title"),
            Some(Scalar::Str("Follow up".into()))
        );
        assert_eq!(get("part", 1, "minutes"), Some(Scalar::Int(4)));
        // Only two positions: the third and fourth parts are left out.
        assert_eq!(
            found
                .iter()
                .filter(|(s, _, _)| s.as_str() == "part")
                .count(),
            2
        );
    }

    #[test]
    fn address_of_an_event() {
        let d = Date::from_calendar_date(2026, time::Month::November, 5).unwrap();
        assert_eq!(
            url_for("https://x/{year}/{week}?d={date}", d),
            "https://x/2026/45?d=2026-11-05"
        );
    }

    /// Reads the real pages with a package's rules (network): run with
    /// `PREFILL_PACKAGE=<package dir> PREFILL_DATE=YYYY-MM-DD cargo test -p core-api
    /// --lib live_pages -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_pages() {
        let (Ok(dir), Ok(day)) = (
            std::env::var("PREFILL_PACKAGE"),
            std::env::var("PREFILL_DATE"),
        ) else {
            return;
        };
        let (pkg, _) = core_config::load_and_validate(std::path::Path::new(&dir)).unwrap();
        let date = Date::parse(
            &day,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .unwrap();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0")
            .build()
            .unwrap();
        for def in &pkg.programs.program_types {
            let Some(pf) = &def.prefill else { continue };
            let outline = outline_for(&client, pf, date).await.unwrap();
            let found = details_from(def, &pf.rules, &outline);
            for (slot, i, v) in &found {
                println!("{} {slot}[{i}] {v:?}", def.id);
            }
            assert!(!found.is_empty(), "{}: nothing recognized", def.id);
        }
    }

    fn serde_norway_like<T: serde::de::DeserializeOwned>(yaml: &str) -> T {
        // Through JSON, like the package loader: enums are maps there.
        let v: serde_json::Value = serde_norway::from_str(yaml).unwrap();
        serde_json::from_value(v).unwrap()
    }
}
