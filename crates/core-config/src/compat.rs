//! Minimal version-range check for `core_compat` (`">=1.0, <2.0"`).

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid version range `{0}`")]
pub struct CompatError(pub String);

fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = parts.next().map_or(Some(0), |p| p.trim().parse().ok())?;
    let patch = parts.next().map_or(Some(0), |p| p.trim().parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// True when `version` satisfies every comma-separated comparator of `range`.
pub fn version_matches(range: &str, version: &str) -> Result<bool, CompatError> {
    let err = || CompatError(range.to_owned());
    let v = parse_version(version).ok_or_else(err)?;
    let mut any = false;
    for comparator in range.split(',') {
        let c = comparator.trim();
        let (op, rest) = ["<=", ">=", "<", ">", "="]
            .iter()
            .find_map(|op| c.strip_prefix(op).map(|r| (*op, r)))
            .unwrap_or(("=", c));
        let bound = parse_version(rest.trim()).ok_or_else(err)?;
        any = true;
        let ok = match op {
            "<=" => v <= bound,
            ">=" => v >= bound,
            "<" => v < bound,
            ">" => v > bound,
            _ => v == bound,
        };
        if !ok {
            return Ok(false);
        }
    }
    if any { Ok(true) } else { Err(err()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(version_matches(">=1.0, <2.0", "1.0.0"), Ok(true));
        assert_eq!(version_matches(">=1.0, <2.0", "1.9.3"), Ok(true));
        assert_eq!(version_matches(">=1.0, <2.0", "2.0.0"), Ok(false));
        assert_eq!(version_matches(">=1.1", "1.0.9"), Ok(false));
        assert_eq!(version_matches("1.2.3", "1.2.3"), Ok(true));
        assert!(version_matches("", "1.0.0").is_err());
        assert!(version_matches(">=x", "1.0.0").is_err());
    }
}
