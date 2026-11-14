//! Typed identifiers at the wire boundary.
//!
//! IDs are strings in JSON and newtypes in the core. ULID-backed ids validate the
//! 26-character Crockford base32 shape; slug-backed ids validate VOWP's slug grammar.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;

macro_rules! ulid_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal, $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Creates an id with a fresh ULID.
            #[must_use]
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, ulid::Ulid::generate()))
            }

            /// Parses and canonicalises an id.
            pub fn parse(input: &str) -> Result<Self, SpiritError> {
                let ulid = input.strip_prefix($prefix).ok_or_else(|| {
                    SpiritError::invalid_input($kind, "missing id prefix")
                })?;
                if ulid.len() != 26 || !ulid.chars().all(is_ulid_char) {
                    return Err(SpiritError::invalid_input($kind, "not a 26-character ULID"));
                }
                let upper = ulid.to_ascii_uppercase();
                if upper.as_bytes().first().is_some_and(|first| *first > b'7') {
                    return Err(SpiritError::invalid_input($kind, "ULID value out of range"));
                }
                if ulid::Ulid::from_string(&upper).is_err() {
                    return Err(SpiritError::invalid_input($kind, "not a ULID"));
                }
                Ok(Self(format!("{}{}", $prefix, upper)))
            }

            /// Borrows the id as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = SpiritError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                Self::parse(input)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = SpiritError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

macro_rules! slug_id {
    ($(#[$meta:meta])* $name:ident $(, $allow_node:literal)?) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Parses a VOWP slug.
            pub fn parse(input: &str) -> Result<Self, SpiritError> {
                let allow_node = false $(|| $allow_node)?;
                if is_slug(input) || (allow_node && input == "_node") {
                    Ok(Self(input.to_owned()))
                } else {
                    Err(SpiritError::invalid_input("slug", "invalid slug"))
                }
            }

            /// Borrows the id as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = SpiritError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                Self::parse(input)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = SpiritError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

fn is_ulid_char(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(
            c,
            'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z'
                | 'a'..='h' | 'j' | 'k' | 'm' | 'n' | 'p'..='t' | 'v'..='z'
        )
}

fn is_slug(input: &str) -> bool {
    let bytes = input.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let Some(first) = bytes.first() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    if !bytes[1..]
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    {
        return false;
    }
    !matches!(bytes.last(), Some(&b'-'))
}

ulid_id!(
    /// Session id: 26-character ULID.
    SessionId,
    "",
    "session id"
);
ulid_id!(
    /// Event id: 26-character ULID.
    EventId,
    "",
    "event id"
);
ulid_id!(
    /// Run id: 26-character ULID.
    RunId,
    "",
    "run id"
);
ulid_id!(
    /// Inbox item id: 26-character ULID.
    InboxId,
    "",
    "inbox id"
);
ulid_id!(
    /// Capsule id: `cap_` followed by a 26-character ULID.
    CapsuleId,
    "cap_",
    "capsule id"
);

slug_id!(
    /// Job id slug, for example `ss-2-desktop-shell`.
    JobId
);
slug_id!(
    /// Node name slug, for example `vps`.
    NodeName
);
slug_id!(
    /// Project name slug. `_node` is reserved for project-less session runs.
    ProjectName,
    true
);
