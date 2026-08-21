use super::{CfdProviderBindings, CfdSourceSelectionError};
use crate::{ProjectSourceRef, CfdSourceAdapter};
use std::cmp::Reverse;
use std::sync::Arc;

impl CfdProviderBindings {
    /// Selects a source provider by explicit source type or by provider probe result.
    ///
    /// # Errors
    ///
    /// Returns an error when no provider matches, the explicit provider id is
    /// unknown, or multiple providers report the same highest confidence.
    pub fn select_source_provider(
        &self,
        source: &ProjectSourceRef<'_>,
    ) -> Result<Arc<dyn CfdSourceAdapter>, CfdSourceSelectionError> {
        if let Some(source_type) = source.source_type {
            return self.source_provider(source_type).ok_or_else(|| {
                CfdSourceSelectionError::UnknownCfdSourceAdapter {
                    id: source_type.to_string(),
                }
            });
        }

        let mut matches = self
            .source_providers
            .values()
            .filter_map(|source_provider| {
                let probe = source_provider.probe(source);
                probe
                    .is_match()
                    .then(|| (probe.confidence, source_provider.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(confidence, _)| Reverse(*confidence));

        let Some((confidence, source_provider)) = matches.first().cloned() else {
            return Err(CfdSourceSelectionError::NoCfdSourceAdapter);
        };
        let tied = matches
            .iter()
            .filter(|(candidate_confidence, _)| *candidate_confidence == confidence)
            .map(|(_, candidate)| candidate.descriptor().id.to_string())
            .collect::<Vec<_>>();
        if tied.len() > 1 {
            return Err(CfdSourceSelectionError::AmbiguousCfdSourceAdapters { ids: tied });
        }
        Ok(source_provider)
    }
}
