use std::collections::{BTreeMap, VecDeque};

/// Source-neutral plan for reconciling an existing table header with a target header.
///
/// A target column is matched to the next unused source column with the same
/// header text. Matching by occurrence keeps repeated empty columns used by
/// expanded table fields distinct and stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderReconciliationPlan {
    target_header: Vec<String>,
    source_columns: Vec<Option<usize>>,
    added: Vec<String>,
    removed: Vec<String>,
    source_width: usize,
}

impl HeaderReconciliationPlan {
    #[must_use]
    pub fn new(source_header: &[String], target_header: &[String]) -> Self {
        let mut available = BTreeMap::<&str, VecDeque<usize>>::new();
        for (index, header) in source_header.iter().enumerate() {
            available.entry(header).or_default().push_back(index);
        }

        let mut used = vec![false; source_header.len()];
        let mut source_columns = Vec::with_capacity(target_header.len());
        let mut added = Vec::new();
        for header in target_header {
            let source_column = available
                .get_mut(header.as_str())
                .and_then(VecDeque::pop_front);
            if let Some(index) = source_column {
                used[index] = true;
            } else {
                added.push(header.clone());
            }
            source_columns.push(source_column);
        }

        let removed = source_header
            .iter()
            .enumerate()
            .filter(|(index, _)| !used[*index])
            .map(|(_, header)| header.clone())
            .collect();

        Self {
            target_header: target_header.to_vec(),
            source_columns,
            added,
            removed,
            source_width: source_header.len(),
        }
    }

    #[must_use]
    pub fn target_header(&self) -> &[String] {
        &self.target_header
    }

    #[must_use]
    pub fn source_column(&self, target_column: usize) -> Option<usize> {
        self.source_columns.get(target_column).copied().flatten()
    }

    #[must_use]
    pub fn source_width(&self) -> usize {
        self.source_width
    }

    #[must_use]
    pub fn target_width(&self) -> usize {
        self.target_header.len()
    }

    #[must_use]
    pub fn storage_width(&self) -> usize {
        self.source_width.max(self.target_width())
    }

    #[must_use]
    pub fn added(&self) -> &[String] {
        &self.added
    }

    #[must_use]
    pub fn removed(&self) -> &[String] {
        &self.removed
    }

    #[must_use]
    pub fn project_row(&self, source_row: &[String]) -> Vec<String> {
        self.source_columns
            .iter()
            .map(|source_column| {
                source_column
                    .and_then(|index| source_row.get(index))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }

    #[must_use]
    pub fn project_rows(&self, source_rows: &[Vec<String>]) -> Vec<Vec<String>> {
        let mut rows = Vec::with_capacity(source_rows.len().max(1));
        rows.push(self.target_header.clone());
        rows.extend(source_rows.iter().skip(1).map(|row| self.project_row(row)));
        rows
    }
}
