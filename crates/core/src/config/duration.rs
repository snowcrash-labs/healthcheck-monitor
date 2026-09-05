//! Finite duration syntax shared by TOML and CLI overrides.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span(pub u64);
impl Span {
    pub fn duration(self) -> Duration {
        Duration::from_secs(self.0)
    }
}
impl FromStr for Span {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let duration =
            humantime::parse_duration(value).map_err(|_| "invalid duration".to_string())?;
        if duration.subsec_nanos() != 0
            || duration.as_secs() == 0
            || duration.as_secs() > 31_536_000
        {
            return Err("duration must be whole seconds between 1s and 365d".into());
        }
        Ok(Self(duration.as_secs()))
    }
}
impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}s", self.0)
    }
}
impl Serialize for Span {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Span {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
