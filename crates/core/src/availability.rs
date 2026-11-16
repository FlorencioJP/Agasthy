use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A value that may legitimately be missing.
///
/// AGASTHY never reports an unknown metric as zero. Anything that could not be
/// measured is carried as one of the non-`Available` variants with a reason, so
/// the UI can render "unavailable" distinctly from a real zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
#[ts(export)]
pub enum Availability<T> {
    Available { value: T },
    Unavailable { reason: String },
    PermissionDenied { detail: String },
    Unsupported { platform: String },
}

impl<T> Availability<T> {
    pub fn available(value: T) -> Self {
        Self::Available { value }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    pub fn permission_denied(detail: impl Into<String>) -> Self {
        Self::PermissionDenied {
            detail: detail.into(),
        }
    }

    pub fn unsupported(platform: impl Into<String>) -> Self {
        Self::Unsupported {
            platform: platform.into(),
        }
    }

    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Available { value } => Some(value),
            _ => None,
        }
    }

    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Available { value } => Some(value),
            _ => None,
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Availability<U> {
        match self {
            Self::Available { value } => Availability::Available { value: f(value) },
            Self::Unavailable { reason } => Availability::Unavailable { reason },
            Self::PermissionDenied { detail } => Availability::PermissionDenied { detail },
            Self::Unsupported { platform } => Availability::Unsupported { platform },
        }
    }
}

impl<T> From<Option<T>> for Availability<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => Self::available(v),
            None => Self::unavailable("not reported by the operating system"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_status_tag() {
        let a: Availability<u64> = Availability::available(0);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            r#"{"status":"available","value":0}"#
        );
        let u: Availability<u64> = Availability::unavailable("warming up");
        assert_eq!(
            serde_json::to_string(&u).unwrap(),
            r#"{"status":"unavailable","reason":"warming up"}"#
        );
        let p: Availability<u64> = Availability::permission_denied("no access");
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"status":"permission_denied","detail":"no access"}"#
        );
    }

    #[test]
    fn zero_is_distinct_from_unavailable() {
        let zero: Availability<u64> = Availability::available(0);
        let missing: Availability<u64> = None.into();
        assert_ne!(zero, missing);
        assert_eq!(zero.value(), Some(&0));
        assert_eq!(missing.value(), None);
    }

    #[test]
    fn round_trips() {
        let a: Availability<String> = Availability::unsupported("windows");
        let json = serde_json::to_string(&a).unwrap();
        let back: Availability<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}
