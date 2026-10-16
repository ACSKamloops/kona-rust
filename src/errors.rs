#![forbid(unsafe_code)]

/// KonaError represents K execution, parsing, and system errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KonaError {
    Type,
    Length,
    Valence,
    Domain,
    Index,
    Rank,
    Value(String),
    Parse(String),
    File(String),
    System(String),
    FeatureDisabled(String),
    Limit,
    Int,
    WsFull,
}

impl std::fmt::Display for KonaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KonaError::Type => write!(f, "type"),
            KonaError::Length => write!(f, "length"),
            KonaError::Valence => write!(f, "valence"),
            KonaError::Domain => write!(f, "domain"),
            KonaError::Index => write!(f, "index"),
            KonaError::Rank => write!(f, "rank"),
            KonaError::Limit => write!(f, "limit"),
            KonaError::Int => write!(f, "int"),
            KonaError::WsFull => write!(f, "wsfull"),
            KonaError::Value(msg) => write!(f, "value: {}", msg),
            KonaError::Parse(msg) => write!(f, "parse: {}", msg),
            KonaError::File(msg) => write!(f, "file: {}", msg),
            KonaError::System(msg) => write!(f, "system: {}", msg),
            KonaError::FeatureDisabled(msg) => write!(f, "feature disabled: {}", msg),
        }
    }
}

impl std::error::Error for KonaError {}
