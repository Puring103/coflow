use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderSelectionError {
    UnknownLoader { id: String },
    NoLoader,
    AmbiguousLoaders { ids: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistrationError {
    provider_kind: &'static str,
    id: String,
}

impl ProviderRegistrationError {
    #[must_use]
    pub fn duplicate(provider_kind: &'static str, id: impl Into<String>) -> Self {
        Self {
            provider_kind,
            id: id.into(),
        }
    }

    #[must_use]
    pub const fn provider_kind(&self) -> &'static str {
        self.provider_kind
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for ProviderRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "duplicate {} provider id `{}`",
            self.provider_kind, self.id
        )
    }
}

impl std::error::Error for ProviderRegistrationError {}
