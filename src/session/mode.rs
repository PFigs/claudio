use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Speaking,
    Listening,
    Muted,
}

impl fmt::Display for SessionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Speaking => write!(f, "speaking"),
            Self::Listening => write!(f, "listening"),
            Self::Muted => write!(f, "muted"),
        }
    }
}

impl FromStr for SessionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "speaking" => Ok(Self::Speaking),
            "listening" => Ok(Self::Listening),
            "muted" => Ok(Self::Muted),
            _ => Err(format!("unknown mode: {s} (expected speaking, listening, or muted)")),
        }
    }
}
