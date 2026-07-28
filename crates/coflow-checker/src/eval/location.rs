use coflow_data_model::{
    CfdDataModel, CfdDictKey, CfdPath, CfdRecordId, DimensionRefCoordinate, RefSite,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelCursor {
    pub(crate) record: CfdRecordId,
    pub(crate) path: CfdPath,
    pub(crate) dimension: Option<DimensionCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DimensionCursor {
    pub(crate) field: String,
    pub(crate) variant: String,
}

impl ModelCursor {
    pub(crate) fn root(record: CfdRecordId) -> Self {
        Self {
            record,
            path: CfdPath::root(),
            dimension: None,
        }
    }

    pub(crate) fn dimension(
        record: CfdRecordId,
        field: impl Into<String>,
        variant: impl Into<String>,
    ) -> Self {
        Self {
            record,
            path: CfdPath::root(),
            dimension: Some(DimensionCursor {
                field: field.into(),
                variant: variant.into(),
            }),
        }
    }

    pub(crate) fn field(&self, name: impl Into<String>) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().field(name),
            dimension: self.dimension.clone(),
        }
    }

    pub(crate) fn index(&self, index: usize) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().index(index),
            dimension: self.dimension.clone(),
        }
    }

    pub(crate) fn dict_key_value(&self, key: &CfdDictKey) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key_value(key),
            dimension: self.dimension.clone(),
        }
    }

    pub(crate) fn dict_key(&self, key: impl Into<String>) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key(key),
            dimension: self.dimension.clone(),
        }
    }

    pub(crate) fn ref_site(&self, model: &CfdDataModel) -> Option<RefSite> {
        let Some(dimension) = &self.dimension else {
            return Some(RefSite::new(self.record, self.path.clone()));
        };
        let record = model.record(self.record)?;
        let (field, values) = record
            .dimension_fields
            .get_key_value(dimension.field.as_str())?;
        let (variant, _) = values.variants.get_key_value(dimension.variant.as_str())?;
        let mut edge_path = CfdPath::root().field(field.as_str());
        edge_path
            .segments
            .extend(self.path.segments.iter().cloned());
        Some(RefSite::in_dimension(
            self.record,
            edge_path,
            DimensionRefCoordinate {
                field: field.clone(),
                dimension: values.dimension.clone(),
                variant: variant.clone(),
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueLocation {
    pub(crate) storage: ModelCursor,
    pub(crate) blame: ModelCursor,
    pub(crate) references: Vec<ModelCursor>,
}

impl ValueLocation {
    pub(crate) fn root(record: CfdRecordId) -> Self {
        let cursor = ModelCursor::root(record);
        Self {
            storage: cursor.clone(),
            blame: cursor,
            references: Vec::new(),
        }
    }

    pub(crate) fn field(&self, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            storage: self.storage.field(name.clone()),
            blame: self.blame.field(name),
            references: self.references.clone(),
        }
    }

    pub(crate) fn index(&self, index: usize) -> Self {
        Self {
            storage: self.storage.index(index),
            blame: self.blame.index(index),
            references: self.references.clone(),
        }
    }

    pub(crate) fn dict_key_value(&self, key: &CfdDictKey) -> Self {
        Self {
            storage: self.storage.dict_key_value(key),
            blame: self.blame.dict_key_value(key),
            references: self.references.clone(),
        }
    }

    pub(crate) fn dict_key(&self, key: impl Into<String>) -> Self {
        let key = key.into();
        Self {
            storage: self.storage.dict_key(key.clone()),
            blame: self.blame.dict_key(key),
            references: self.references.clone(),
        }
    }

    pub(crate) fn backed_by(&self, storage: ModelCursor) -> Self {
        Self {
            storage,
            blame: self.blame.clone(),
            references: self.references.clone(),
        }
    }

    pub(crate) fn dereference(mut self, target: CfdRecordId) -> Self {
        self.references.push(self.blame);
        let target = ModelCursor::root(target);
        Self {
            storage: target.clone(),
            blame: target,
            references: self.references,
        }
    }
}
