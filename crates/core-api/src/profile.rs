//! The profile: the data a person changes about themselves (package
//! `ui.profile`), and address suggestions from OpenStreetMap.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use core_engine::time::date_of;
use core_model::{Id, Scalar, Slug};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dto::Field;
use crate::views::{attribute_field, cfg};
use crate::{ApiError, ApiResult, Core};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Profile {
    /// The name as shown everywhere ("family given").
    pub name: String,
    /// Given and family name, when the package keeps them apart.
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub fields: Vec<Field>,
    pub dated: Vec<ProfileDate>,
    /// The field filled with address suggestions.
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProfileDate {
    pub id: String,
    pub label: String,
    /// YYYY-MM-DD, none if not held.
    pub date: Option<String>,
}

impl Core {
    pub fn profile(&self) -> ApiResult<Profile> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let c = cfg(&v)?;
        let t = self.texts();
        let pr = &c.package().ui.profile;
        let rec = v
            .proj
            .auth
            .org
            .persons
            .get(&me)
            .ok_or_else(|| ApiError::not_found("person"))?;
        let defs = &c.package().manifest.person_attributes;
        let text_of = |a: &Option<Slug>| {
            a.as_ref().map(|id| match rec.attributes.get(id) {
                Some(Scalar::Str(s)) => s.clone(),
                _ => String::new(),
            })
        };
        // Before the first save the parts come from the name ("family given").
        let (mut given, mut family) = (text_of(&pr.given_name), text_of(&pr.family_name));
        if given.as_deref() == Some("") && family.as_deref() == Some("") {
            let mut words = rec.display_name.split_whitespace();
            family = Some(words.next().unwrap_or_default().to_owned());
            given = Some(words.collect::<Vec<_>>().join(" "));
        }
        let now = date_of(self.now());
        let auth = &v.proj.auth;
        Ok(Profile {
            name: rec.display_name.clone(),
            given_name: given,
            family_name: family,
            fields: pr
                .fields
                .iter()
                .filter_map(|id| defs.iter().find(|a| &a.id == id))
                .map(|a| attribute_field(&t, a, rec.attributes.get(&a.id)))
                .collect(),
            dated: pr
                .dated_qualifications
                .iter()
                .filter_map(|q| c.qualification(q))
                .map(|q| ProfileDate {
                    id: q.id.to_string(),
                    label: t.get(q.label.as_str()),
                    date: auth
                        .org
                        .qualification_grants
                        .iter()
                        .filter(|g| {
                            g.person == me
                                && g.qualification == q.id
                                && g.period.end.is_none_or(|e| e > now && e > g.period.start)
                        })
                        .map(|g| g.period.start)
                        .min()
                        .map(|d| d.to_string()),
                })
                .collect(),
            address: pr.address.as_ref().map(ToString::to_string),
        })
    }

    /// Saves the profile: names, attributes (an empty value clears it) and
    /// the dates of the dated qualifications (empty: not held).
    pub fn save_profile(
        self: &Arc<Self>,
        given_name: Option<String>,
        family_name: Option<String>,
        attributes: BTreeMap<String, serde_json::Value>,
        dated: BTreeMap<String, String>,
    ) -> ApiResult<Profile> {
        let me = self.me()?;
        let pr = {
            let v = self.view()?;
            cfg(&v)?.package().ui.profile.clone()
        };
        let mut attributes = attributes;
        // Only the profile's attributes: the engine refuses the others.
        let allowed: Vec<&Slug> = pr
            .fields
            .iter()
            .chain(pr.given_name.iter())
            .chain(pr.family_name.iter())
            .chain(pr.address.iter())
            .collect();
        attributes.retain(|k, _| allowed.iter().any(|a| a.as_str() == k));
        let mut name = None;
        if let (Some(gk), Some(fk), Some(g), Some(f)) =
            (&pr.given_name, &pr.family_name, given_name, family_name)
        {
            let (g, f) = (g.trim().to_owned(), f.trim().to_owned());
            if g.is_empty() || f.is_empty() {
                return Err(ApiError::invalid("name and surname are needed"));
            }
            name = Some(format!("{f} {g}"));
            attributes.insert(gk.to_string(), serde_json::json!(g));
            attributes.insert(fk.to_string(), serde_json::json!(f));
        }
        self.update_person(me.to_string(), name, attributes)?;
        // Dated qualifications: a new date replaces the old grant (which then never held).
        for q in &pr.dated_qualifications {
            let Some(wanted) = dated.get(q.as_str()) else {
                continue;
            };
            let current: Vec<(Id, String)> = {
                let v = self.view()?;
                let now = date_of(self.now());
                v.proj
                    .auth
                    .qualification_ids
                    .iter()
                    .filter_map(|(gid, i)| {
                        v.proj
                            .auth
                            .org
                            .qualification_grants
                            .get(*i)
                            .map(|g| (gid, g))
                    })
                    .filter(|(_, g)| {
                        g.person == me
                            && &g.qualification == q
                            && g.period.end.is_none_or(|e| e > now && e > g.period.start)
                    })
                    .map(|(gid, g)| (*gid, g.period.start.to_string()))
                    .collect()
            };
            if current.iter().any(|(_, from)| from == wanted) {
                continue;
            }
            for (gid, from) in &current {
                self.end_grant("qualification".into(), gid.to_string(), Some(from.clone()))?;
            }
            if !wanted.trim().is_empty() {
                self.grant_qualification(
                    me.to_string(),
                    q.to_string(),
                    Some(wanted.clone()),
                    None,
                )?;
            }
        }
        self.profile()
    }

    /// Address suggestions for `query` from OpenStreetMap (Nominatim),
    /// limited to the package's countries.
    pub async fn address_search(self: &Arc<Self>, query: String) -> ApiResult<Vec<String>> {
        let q = query.trim();
        if q.chars().count() < 4 {
            return Ok(Vec::new());
        }
        let countries = {
            let v = self.view()?;
            cfg(&v)?.package().ui.profile.countries.join(",")
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("mosaic-agenda/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ApiError::internal)?;
        let mut params = vec![
            ("q", q),
            ("format", "jsonv2"),
            ("limit", "6"),
            ("accept-language", "it"),
        ];
        if !countries.is_empty() {
            params.push(("countrycodes", countries.as_str()));
        }
        let url =
            reqwest::Url::parse_with_params("https://nominatim.openstreetmap.org/search", &params)
                .map_err(ApiError::internal)?;
        let req = client.get(url);
        let r = req
            .send()
            .await
            .map_err(|e| ApiError::new("network", e.to_string()))?;
        if !r.status().is_success() {
            return Err(ApiError::new("network", r.status().to_string()));
        }
        let found: Vec<serde_json::Value> = r
            .json()
            .await
            .map_err(|e| ApiError::new("network", e.to_string()))?;
        Ok(found
            .iter()
            .filter_map(|x| {
                x.get("display_name")
                    .and_then(|d| d.as_str())
                    .map(str::to_owned)
            })
            .collect())
    }
}
