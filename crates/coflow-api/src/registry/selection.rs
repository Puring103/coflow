use super::{LoaderSelectionError, ProviderRegistry};
use crate::{DataLoader, ProjectSourceRef};
use std::cmp::Reverse;
use std::sync::Arc;

impl ProviderRegistry {
    /// Selects a loader by explicit source type or by provider probe result.
    ///
    /// # Errors
    ///
    /// Returns an error when no provider matches, the explicit provider id is
    /// unknown, or multiple providers report the same highest confidence.
    pub fn select_loader(
        &self,
        source: &ProjectSourceRef<'_>,
    ) -> Result<Arc<dyn DataLoader>, LoaderSelectionError> {
        if let Some(source_type) = source.source_type {
            return self
                .loader(source_type)
                .ok_or_else(|| LoaderSelectionError::UnknownLoader {
                    id: source_type.to_string(),
                });
        }

        let mut matches = self
            .loaders
            .values()
            .filter_map(|loader| {
                let probe = loader.probe(source);
                probe.is_match().then(|| (probe.confidence, loader.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|(confidence, _)| Reverse(*confidence));

        let Some((confidence, loader)) = matches.first().cloned() else {
            return Err(LoaderSelectionError::NoLoader);
        };
        let tied = matches
            .iter()
            .filter(|(candidate_confidence, _)| *candidate_confidence == confidence)
            .map(|(_, candidate)| candidate.descriptor().id.to_string())
            .collect::<Vec<_>>();
        if tied.len() > 1 {
            return Err(LoaderSelectionError::AmbiguousLoaders { ids: tied });
        }
        Ok(loader)
    }
}
