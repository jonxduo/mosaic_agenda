//! Pictures of photo fields and reading printed cards from a photo
//! (DECISIONS.md, "Lettura di schede da foto").
//!
//! The interface sends new pictures as `data:` URLs; before the event is
//! built each one is encrypted with a key of its own, stored as a blob and
//! replaced by `"<hash>:<key>"`.

use std::collections::BTreeMap;
use std::sync::Arc;

use core_engine::ops::Values;
use core_engine::values::{attachment_ref, parse_geo};
use core_model::config::{FieldType, ScanRead};
use core_model::strings::{ObjectKind, ObjectRef};
use core_model::{Permission, Scalar};
use core_sync::blobs::{open_attachment, seal_attachment};
use data_encoding::{BASE64, HEXLOWER};

use crate::access::holds_any;
use crate::dto::ScanResult;
use crate::views::cfg;
use crate::{ApiError, ApiResult, Core, lock};

/// The bytes of a `data:` URL, or of plain base64.
fn data_url_bytes(s: &str) -> ApiResult<Vec<u8>> {
    let b64 = match s.strip_prefix("data:") {
        Some(rest) => rest
            .split_once(";base64,")
            .map(|(_, b)| b)
            .ok_or_else(|| ApiError::invalid("not a base64 data URL"))?,
        None => s,
    };
    BASE64
        .decode(b64.trim().as_bytes())
        .map_err(ApiError::invalid)
}

fn jpeg_url(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", BASE64.encode(bytes))
}

/// A Google Maps link to a point.
pub(crate) fn map_link(lat: f64, lng: f64) -> String {
    format!("https://www.google.com/maps/search/?api=1&query={lat},{lng}")
}

impl Core {
    /// Replaces the new pictures (`data:` URLs) of the photo fields of
    /// `values` by references to encrypted blobs, storing the blobs.
    pub(crate) fn store_pictures(
        &self,
        fields: &[core_model::config::FieldDef],
        values: &mut Values,
    ) -> ApiResult<()> {
        let org = self.view()?.proj.org;
        for f in fields.iter().filter(|f| f.ty == FieldType::Photos) {
            let Some(Scalar::List(items)) = values.get_mut(&f.id) else {
                continue;
            };
            for item in items.iter_mut() {
                let Scalar::Str(s) = item else { continue };
                if attachment_ref(s).is_some() {
                    continue;
                }
                let sealed = seal_attachment(&data_url_bytes(s)?, &org).map_err(ApiError::invalid)?;
                lock(&self.state)?
                    .blobs
                    .put(&sealed.hash, &sealed.blob)
                    .map_err(ApiError::internal)?;
                *s = format!(
                    "{}:{}",
                    HEXLOWER.encode(&sealed.hash),
                    HEXLOWER.encode(&sealed.key)
                );
            }
        }
        Ok(())
    }

    /// A stored picture as a `data:` URL; the reference carries its key.
    pub fn attachment(&self, reference: String) -> ApiResult<String> {
        self.ensure_unlocked()?;
        let (hash, key) =
            attachment_ref(&reference).ok_or_else(|| ApiError::invalid("not an attachment"))?;
        let org = self.view()?.proj.org;
        let blob = {
            let mut st = lock(&self.state)?;
            match st.blobs.get(&hash).map_err(ApiError::internal)? {
                Some(b) => b,
                None => {
                    // Not here yet: ask the peers for it on the next sync.
                    st.wanted_blobs.insert(hash);
                    return Err(ApiError::new(
                        "not_yet",
                        "the picture has not arrived on this device yet",
                    ));
                }
            }
        };
        let jpeg = open_attachment(&blob, &org, &key).map_err(ApiError::internal)?;
        Ok(jpeg_url(&jpeg))
    }

    /// Reads a printed card of `resource_type` from a photo: values for the
    /// new resource form, pictures as `data:` URLs. Only who manages the
    /// resources of that type may do it.
    pub async fn scan_resource(
        self: &Arc<Self>,
        resource_type: String,
        image: String,
    ) -> ApiResult<ScanResult> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let def = cfg(&v)?
            .resource_type(&crate::commands::slug(&resource_type)?)
            .ok_or_else(|| ApiError::not_found("resource type"))?
            .clone();
        let obj = ObjectRef {
            kind: ObjectKind::Resource,
            id: def.id.clone(),
        };
        if !holds_any(&v, me, &[(Permission::ResourceManage, Some(obj))], self.now()) {
            return Err(ApiError::new("denied", "you do not manage these resources"));
        }
        let layout = def
            .scan
            .clone()
            .ok_or_else(|| ApiError::invalid("these resources have no card to read"))?;
        let bytes = data_url_bytes(&image)?;
        let scan = tokio::task::spawn_blocking(move || {
            let ocr = core_scan::Tesseract::find();
            core_scan::scan(&bytes, &layout, ocr.as_ref().map(|o| o as &dyn core_scan::Ocr))
        })
        .await
        .map_err(ApiError::internal)?
        .map_err(|e| ApiError::new("unreadable", e.to_string()))?;
        let mut values = BTreeMap::new();
        let field = |id: &str| def.attributes.iter().find(|a| a.id.as_str() == id);
        for (id, text) in &scan.texts {
            let value = match field(id).map(|f| f.ty) {
                Some(FieldType::Integer) => {
                    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
                    digits.parse::<i64>().map(serde_json::Value::from).ok()
                }
                _ => Some(serde_json::Value::from(text.clone())),
            };
            if let Some(v) = value {
                values.insert(id.clone(), v);
            }
        }
        for (id, code) in &scan.codes {
            let value = match field(id).map(|f| f.ty) {
                Some(FieldType::Geo) => {
                    core_scan::geo_from_link(code).map(|(lat, lng)| format!("{lat},{lng}"))
                }
                _ => Some(code.clone()),
            };
            if let Some(v) = value {
                values.insert(id.clone(), serde_json::Value::from(v));
            }
        }
        for (id, jpeg) in &scan.images {
            values.insert(id.clone(), serde_json::json!([jpeg_url(jpeg)]));
        }
        let missing = def
            .scan
            .iter()
            .flat_map(|s| &s.boxes)
            .filter(|b| !values.contains_key(b.field.as_str()))
            .filter(|b| b.read != ScanRead::Image)
            .map(|b| {
                field(b.field.as_str()).map_or_else(
                    || b.field.to_string(),
                    |f| crate::views::field_dto(&self.texts(), f, None).label,
                )
            })
            .collect();
        Ok(ScanResult {
            values: crate::dto::JsonMap(values),
            text_read: scan.ocr,
            missing,
        })
    }
}

/// `(lat, lng, link)` of a `geo` value.
pub(crate) fn geo_value(v: &Scalar) -> Option<(f64, f64, String)> {
    match v {
        Scalar::Str(s) => parse_geo(s).map(|(lat, lng)| (lat, lng, map_link(lat, lng))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn data_urls_and_plain_base64() {
        assert_eq!(data_url_bytes("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert_eq!(data_url_bytes("aGk=").unwrap(), b"hi");
        assert!(data_url_bytes("data:text/plain,hi").is_err());
    }
}
