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
    inner: DecodedProviderOptions,
}

/// Provider-owned output options after the project-facing JSON shape has been
/// decoded and validated.
#[derive(Clone)]
pub struct DecodedOutputOptions {
    inner: DecodedProviderOptions,
}

#[derive(Clone)]
struct DecodedProviderOptions {
    provider_id: String,
    value: Arc<dyn ErasedProviderOptions>,
}

impl DecodedProviderOptions {
    fn new<T>(provider_id: impl Into<String>, value: T) -> Self
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        Self {
            provider_id: provider_id.into(),
            value: Arc::new(value),
        }
    }

    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn require<T>(&self, expected_provider_id: &str, option_kind: &str) -> Result<&T, DiagnosticSet>
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        if self.provider_id != expected_provider_id {
            return Err(contract_error(format!(
                "{option_kind} options decoded for provider `{}` were passed to provider `{expected_provider_id}`",
                self.provider_id
            )));
        }
        self.value
            .as_ref()
            .as_any()
            .downcast_ref::<T>()
            .ok_or_else(|| {
                contract_error(format!(
                    "provider `{expected_provider_id}` expected {option_kind} options `{}`, but received `{}`",
                    std::any::type_name::<T>(),
                    self.value.as_ref().type_name()
                ))
            })
    }

    fn fmt(&self, name: &str, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct(name)
            .field("provider_id", &self.provider_id)
            .field("type_name", &self.value.as_ref().type_name())
            .finish_non_exhaustive()
    }
}

macro_rules! decoded_options {
    ($options:ident, $kind:literal) => {
        impl $options {
            #[must_use]
            pub fn new<T>(provider_id: impl Into<String>, value: T) -> Self
            where
                T: fmt::Debug + Send + Sync + 'static,
            {
                Self {
                    inner: DecodedProviderOptions::new(provider_id, value),
                }
            }

            #[must_use]
            pub fn provider_id(&self) -> &str {
                self.inner.provider_id()
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
                self.inner.require(expected_provider_id, $kind)
            }
        }

        impl fmt::Debug for $options {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.inner.fmt(stringify!($options), formatter)
            }
        }
    };
}

decoded_options!(DecodedSourceOptions, "source");
decoded_options!(DecodedOutputOptions, "output");

fn contract_error(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error(
        "PROVIDER-OPTIONS-CONTRACT",
        "PROVIDER",
        message,
    ))
}
