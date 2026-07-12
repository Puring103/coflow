use crate::{Diagnostic, DiagnosticSet};
use std::any::Any;
use std::fmt;
use std::sync::Arc;

trait ErasedProviderOptions: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn type_name(&self) -> &'static str;
}

impl<T> ErasedProviderOptions for T
where
    T: fmt::Debug + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// Provider-owned source options after the project-facing JSON shape has been
/// decoded and validated.
#[derive(Clone)]
pub struct DecodedSourceOptions {
    provider_id: String,
    value: Arc<dyn ErasedProviderOptions>,
}

/// Provider-owned output options after the project-facing JSON shape has been
/// decoded and validated.
#[derive(Clone)]
pub struct DecodedOutputOptions {
    provider_id: String,
    value: Arc<dyn ErasedProviderOptions>,
}

impl DecodedOutputOptions {
    #[must_use]
    pub fn new<T>(provider_id: impl Into<String>, value: T) -> Self
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        Self {
            provider_id: provider_id.into(),
            value: Arc::new(value),
        }
    }

    #[must_use]
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Downcast to the option type owned by the expected provider.
    ///
    /// # Errors
    ///
    /// Returns a contract diagnostic when provider identity or option type
    /// does not match the consumer.
    pub fn require<T>(&self, expected_provider_id: &str) -> Result<&T, DiagnosticSet>
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        if self.provider_id != expected_provider_id {
            return Err(contract_error(format!(
                "output options decoded for provider `{}` were passed to provider `{expected_provider_id}`",
                self.provider_id
            )));
        }
        self.value
            .as_ref()
            .as_any()
            .downcast_ref::<T>()
            .ok_or_else(|| {
                contract_error(format!(
                    "provider `{expected_provider_id}` expected output options `{}`, but received `{}`",
                    std::any::type_name::<T>(),
                    self.value.as_ref().type_name()
                ))
            })
    }
}

impl fmt::Debug for DecodedOutputOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedOutputOptions")
            .field("provider_id", &self.provider_id)
            .field("type_name", &self.value.as_ref().type_name())
            .finish_non_exhaustive()
    }
}

impl DecodedSourceOptions {
    #[must_use]
    pub fn new<T>(provider_id: impl Into<String>, value: T) -> Self
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        Self {
            provider_id: provider_id.into(),
            value: Arc::new(value),
        }
    }

    #[must_use]
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// Downcast to the option type owned by the expected provider.
    ///
    /// # Errors
    ///
    /// Returns a contract diagnostic when provider identity or option type
    /// does not match the consumer.
    pub fn require<T>(&self, expected_provider_id: &str) -> Result<&T, DiagnosticSet>
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        if self.provider_id != expected_provider_id {
            return Err(contract_error(format!(
                "source options decoded for provider `{}` were passed to provider `{expected_provider_id}`",
                self.provider_id
            )));
        }
        self.value
            .as_ref()
            .as_any()
            .downcast_ref::<T>()
            .ok_or_else(|| {
                contract_error(format!(
                "provider `{expected_provider_id}` expected source options `{}`, but received `{}`",
                std::any::type_name::<T>(),
                self.value.as_ref().type_name()
            ))
            })
    }
}

impl fmt::Debug for DecodedSourceOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedSourceOptions")
            .field("provider_id", &self.provider_id)
            .field("type_name", &self.value.as_ref().type_name())
            .finish_non_exhaustive()
    }
}

fn contract_error(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "PROVIDER-OPTIONS-CONTRACT",
        "PROVIDER",
        message,
    ))
}
