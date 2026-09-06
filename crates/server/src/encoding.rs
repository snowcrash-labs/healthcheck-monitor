//! Negotiate prebuilt representations, including explicit refusals and quality values.
#[derive(Debug, PartialEq, Eq)]
pub enum Coding {
    Raw,
    Gzip,
    Zstd,
}
impl Coding {
    pub fn directory(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Gzip => "gzip",
            Self::Zstd => "zstd",
        }
    }
    pub fn header(&self) -> Option<&'static str> {
        match self {
            Self::Raw => None,
            Self::Gzip => Some("gzip"),
            Self::Zstd => Some("zstd"),
        }
    }
}
pub fn select(header: Option<&str>) -> Option<Coding> {
    let Some(header) = header else {
        return Some(Coding::Raw);
    };
    let (mut gzip, mut zstd, mut identity, mut wildcard) = (None, None, None, None);
    for entry in header.split(',').take(32) {
        let mut parts = entry.split(';');
        let encoding = parts.next()?.trim();
        let weight = parts
            .find_map(|part| part.trim().strip_prefix("q="))
            .map_or(1000, quality);
        if encoding.eq_ignore_ascii_case("gzip") {
            gzip = Some(weight);
        } else if encoding.eq_ignore_ascii_case("zstd") {
            zstd = Some(weight);
        } else if encoding.eq_ignore_ascii_case("identity") {
            identity = Some(weight);
        } else if encoding == "*" {
            wildcard = Some(weight);
        }
    }
    let gzip = gzip.or(wildcard).unwrap_or(0);
    let zstd = zstd.or(wildcard).unwrap_or(0);
    let raw = identity.unwrap_or(if wildcard == Some(0) { 0 } else { 1 });
    if zstd > 0 && zstd >= gzip && zstd >= raw {
        Some(Coding::Zstd)
    } else if gzip > 0 && gzip >= raw {
        Some(Coding::Gzip)
    } else if raw > 0 {
        Some(Coding::Raw)
    } else {
        None
    }
}
fn quality(value: &str) -> u16 {
    let Some((whole, fraction)) = value.split_once('.') else {
        return if value == "1" { 1000 } else { 0 };
    };
    if fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return 0;
    }
    if whole == "1" {
        return if fraction.bytes().all(|byte| byte == b'0') {
            1000
        } else {
            0
        };
    }
    if whole != "0" {
        return 0;
    }
    fraction.parse::<u16>().unwrap_or(0) * 10u16.pow(3 - fraction.len() as u32)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_and_exclusions_select_only_acceptable_encodings() {
        assert_eq!(select(None), Some(Coding::Raw));
        assert_eq!(select(Some("gzip, zstd")), Some(Coding::Zstd));
        assert_eq!(select(Some("gzip;q=1, zstd;q=0.5")), Some(Coding::Gzip));
        assert_eq!(select(Some("gzip;q=0, zstd;q=0")), Some(Coding::Raw));
        assert_eq!(select(Some("*;q=0, gzip;q=0.4")), Some(Coding::Gzip));
        assert_eq!(select(Some("*;q=0")), None);
        assert_eq!(select(Some("identity;q=1, gzip;q=0.5")), Some(Coding::Raw));
    }
}
