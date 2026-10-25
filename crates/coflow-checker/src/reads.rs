use std::collections::{BTreeMap, BTreeSet};

use coflow_model::CfdRecordHandle;

/// The projection of a model field observed while executing a check root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckReadProjection {
    Base,
    Variant(String),
    AllVariants,
}

/// A read is rooted in its storage record, including reads reached through references.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckFieldRead {
    pub record: CfdRecordHandle,
    pub field: String,
    pub projection: CheckReadProjection,
}

/// Dependencies observed by one root evaluation. Handles belong to the input model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReadSet {
    pub fields: BTreeSet<CheckFieldRead>,
    pub record_sets: BTreeSet<coflow_language::cft::TypeName>,
    pub(crate) tracked: bool,
}

impl CheckReadSet {
    #[must_use]
    pub const fn is_tracked(&self) -> bool {
        self.tracked
    }

    pub(crate) fn tracked() -> Self {
        Self {
            tracked: true,
            ..Self::default()
        }
    }
}

/// 求值期间按记录及字段分组，重复访问借用字段名查找，避免每个表达式都分配依赖键。
#[derive(Debug, Default)]
pub(crate) struct ReadCollector {
    fields: BTreeMap<CfdRecordHandle, BTreeMap<String, ReadProjections>>,
    pub(crate) record_sets: BTreeSet<coflow_language::cft::TypeName>,
}

// 常见字段只读取基础值，直接内联该投影；确有多种投影时才分配集合。
#[derive(Debug)]
enum ReadProjections {
    One(CheckReadProjection),
    Many(BTreeSet<CheckReadProjection>),
}

impl ReadProjections {
    fn insert(&mut self, projection: CheckReadProjection) {
        match self {
            Self::One(current) if *current == projection => {}
            Self::One(current) => {
                *self = Self::Many(BTreeSet::from([current.clone(), projection]));
            }
            Self::Many(projections) => {
                projections.insert(projection);
            }
        }
    }

    fn into_iter(self) -> impl Iterator<Item = CheckReadProjection> {
        let (one, many) = match self {
            Self::One(projection) => (Some(projection), None),
            Self::Many(projections) => (None, Some(projections)),
        };
        one.into_iter().chain(many.into_iter().flatten())
    }
}

impl ReadCollector {
    pub(crate) fn finish(self) -> CheckReadSet {
        let mut reads = BTreeSet::new();
        for (record, fields) in self.fields {
            for (mut field, projections) in fields {
                let mut projections = projections.into_iter().peekable();
                while let Some(projection) = projections.next() {
                    reads.insert(CheckFieldRead {
                        record,
                        field: if projections.peek().is_some() {
                            field.clone()
                        } else {
                            std::mem::take(&mut field)
                        },
                        projection,
                    });
                }
            }
        }
        CheckReadSet {
            fields: reads,
            record_sets: self.record_sets,
            tracked: true,
        }
    }

    pub(crate) fn field(
        &mut self,
        record: CfdRecordHandle,
        field: &str,
        projection: CheckReadProjection,
    ) {
        let fields = self.fields.entry(record).or_default();
        if let Some(projections) = fields.get_mut(field) {
            projections.insert(projection);
        } else {
            fields.insert(field.to_owned(), ReadProjections::One(projection));
        }
    }

    pub(crate) fn location(&mut self, location: &crate::eval::ValueLocation) {
        self.cursor(&location.storage);
        // 引用链上的来源字段控制目标解析；目标字段变化和引用改向都必须使读取者失效。
        for reference in &location.references {
            self.cursor(reference);
        }
    }

    pub(crate) fn cursor(&mut self, cursor: &crate::eval::ModelCursor) {
        if let Some(dimension) = &cursor.dimension {
            self.field(
                cursor.record,
                &dimension.field,
                CheckReadProjection::Variant(dimension.variant.clone()),
            );
        } else if let Some(coflow_model::CfdPathSegment::Field(field)) =
            cursor.path.segments.first()
        {
            self.field(cursor.record, field, CheckReadProjection::Base);
        }
    }
}
