use super::dimension_inputs::{DimensionInputIndex, DimensionKey, PreparedDimensions};
use super::draft::{RecordDraftSource, ValueDraft};
use super::resolve::ValueResolver;
use super::validate::Validator;
use super::{
    BuildSchema, CfdModelBuildOutput, CfdModelBuildStats, CfdModelBuilder, RecordDraft,
    RecordInputs,
};
use crate::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDimensionFieldValues, CfdDimensionValue,
    CfdObject, CfdPath, CfdRecord, CfdRecordHandle, CfdValueSemanticContext, DimensionValueDraft,
    ValueValidationMode, ValueValidationRequest,
};
use coflow_language::cft::{DimensionName, FieldName, TypeName, VariantName};
use coflow_language::limits::{StructuralLimits, TraversalCursor};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TemplateField {
    pub record: CfdRecordHandle,
    pub field: FieldName,
}

pub(crate) type TemplateReads = BTreeMap<TemplateField, BTreeSet<TemplateField>>;

#[derive(Debug, Clone, Default)]
pub(crate) struct TemplateDependencies {
    by_reader: BTreeMap<TemplateField, Arc<BTreeSet<TemplateField>>>,
    readers: BTreeMap<TemplateField, Arc<BTreeSet<TemplateField>>>,
}

impl TemplateDependencies {
    pub(crate) fn new(reads: TemplateReads) -> Self {
        let mut result = Self::default();
        for (host, targets) in reads {
            result.replace(host, targets);
        }
        result
    }

    fn replace(&mut self, host: TemplateField, targets: BTreeSet<TemplateField>) {
        let previous = self.by_reader.get(&host).cloned().unwrap_or_default();
        // 模板读集通常稳定；仅维护差集，避免反复拆除、重建高扇入关系。
        if previous.as_ref() == &targets {
            return;
        }
        for target in previous.difference(&targets) {
            if let Some(readers) = self.readers.get_mut(target) {
                Arc::make_mut(readers).remove(&host);
                if readers.is_empty() {
                    self.readers.remove(target);
                }
            }
        }
        for target in targets.difference(&previous) {
            Arc::make_mut(self.readers.entry(target.clone()).or_default()).insert(host.clone());
        }
        if targets.is_empty() {
            self.by_reader.remove(&host);
        } else {
            self.by_reader.insert(host, Arc::new(targets));
        }
    }

    fn affected(&self, changed: &[TemplateField]) -> BTreeSet<CfdRecordHandle> {
        let mut affected = changed.iter().cloned().collect::<BTreeSet<_>>();
        let mut pending = changed.to_vec();
        while let Some(target) = pending.pop() {
            if let Some(readers) = self.readers.get(&target) {
                for reader in readers.iter() {
                    if affected.insert(reader.clone()) {
                        pending.push(reader.clone());
                    }
                }
            }
        }
        affected.into_iter().map(|field| field.record).collect()
    }
}

struct LazyDimension<'a> {
    input: &'a DimensionValueDraft,
    value: OnceCell<Option<ValueDraft>>,
}

/// 只暂存本轮真正访问过的已验证草稿，不在模型里永久保留第三份完整数据树。
type DraftPage = Box<[OnceCell<Option<Box<RecordDraft>>>]>;
type DimensionPage<'a> = Box<[OnceCell<BTreeMap<DimensionKey, LazyDimension<'a>>>]>;

struct LazyDrafts<'a, 'input> {
    schema: BuildSchema<'a>,
    inputs: &'a RecordInputs<'input>,
    records: Vec<OnceCell<DraftPage>>,
    dimensions: PreparedDimensions<'a>,
    dimension_pages: Vec<OnceCell<DimensionPage<'a>>>,
    diagnostics: RefCell<Vec<CfdDiagnostic>>,
    validated: Cell<usize>,
    limits: StructuralLimits,
}

impl<'a> LazyDrafts<'a, '_> {
    fn dimension_entry(&self, key: &DimensionKey) -> Option<&LazyDimension<'a>> {
        self.dimensions.get(key)?;
        let host = key.0;
        self.dimension_pages
            .get(host.index() / 512)?
            .get_or_init(|| (0..512).map(|_| OnceCell::new()).collect())
            .get(host.index() % 512)?
            .get_or_init(|| {
                self.dimensions
                    .entries(host)
                    .map(|(key, input)| {
                        (
                            key,
                            LazyDimension {
                                input,
                                value: OnceCell::new(),
                            },
                        )
                    })
                    .collect()
            })
            .get(key)
    }
}

impl RecordDraftSource for LazyDrafts<'_, '_> {
    fn get(&self, index: usize) -> Option<&RecordDraft> {
        if index >= self.inputs.len() {
            return None;
        }
        self.records
            .get(index / 512)?
            .get_or_init(|| (0..512).map(|_| OnceCell::new()).collect())
            .get(index % 512)?
            .get_or_init(|| {
                let input = self.inputs.get(index)?;
                let mut diagnostics = Vec::new();
                let mut draft = Validator::new(&self.schema, &mut diagnostics, self.limits)
                    .validate_top_level_record(
                        None,
                        &input.id,
                        &input.actual_type,
                        &input.fields,
                        Some(CfdRecordHandle::new(index)),
                        CfdPath::root(),
                    );
                if let Some(draft) = &mut draft {
                    draft.origin = input.origin.clone();
                }
                self.validated.set(self.validated.get() + 1);
                self.diagnostics.borrow_mut().extend(diagnostics);
                draft.map(Box::new)
            })
            .as_deref()
    }

    fn actual_type(&self, index: usize) -> Option<&str> {
        // 目标身份已与旧模型逐项核对；裸引用校验不需要物化目标记录的字段。
        self.inputs
            .get(index)
            .map(|input| input.actual_type.as_str())
    }

    fn dimension_value(
        &self,
        record: CfdRecordHandle,
        field: &FieldName,
        dimension: &DimensionName,
        variant: &VariantName,
    ) -> Option<&ValueDraft> {
        let entry =
            self.dimension_entry(&(record, field.clone(), dimension.clone(), variant.clone()))?;
        entry
            .value
            .get_or_init(|| {
                let owner = self.inputs.get(record.index())?;
                let field = self.schema.field(&owner.actual_type, field.as_str())?;
                let path = CfdPath::root().field(field.name.as_str());
                let mut diagnostics = Vec::new();
                let value = Validator::new(&self.schema, &mut diagnostics, self.limits)
                    .validate_value(
                        &field.value_type,
                        &entry.input.value,
                        Some(record),
                        path.clone(),
                        TraversalCursor::root(),
                    );
                super::attach_origin_to_diagnostics(
                    &mut diagnostics,
                    &entry.input.origin,
                    Some(record),
                    &path,
                );
                self.diagnostics.borrow_mut().extend(diagnostics);
                value
            })
            .as_ref()
    }
}

pub(super) fn rebuild(
    mut builder: CfdModelBuilder<'_>,
    previous: &CfdDataModel,
    changed: &[CfdRecordHandle],
    changed_fields: Option<&BTreeMap<CfdRecordHandle, BTreeSet<FieldName>>>,
) -> Option<CfdModelBuildOutput> {
    let input_ranges = can_reuse_model(&builder, previous, changed)?;
    let input_records_checked = input_ranges.iter().map(ExactSizeIterator::len).sum();
    for input in &builder.dimension_values {
        builder
            .dimension_variants
            .entry(input.dimension.clone())
            .or_default()
            .insert(input.variant.clone());
    }
    if &builder.dimension_variants != previous.dimension_variants.as_ref() {
        return None;
    }
    let schema = BuildSchema::new(builder.schema);
    let dimensions = DimensionInputIndex::prepare(&builder, previous, schema)?;
    let drafts = LazyDrafts {
        schema,
        inputs: &builder.records,
        records: (0..builder.records.len().div_ceil(512))
            .map(|_| OnceCell::new())
            .collect(),
        dimensions,
        dimension_pages: (0..builder.records.len().div_ceil(512))
            .map(|_| OnceCell::new())
            .collect(),
        diagnostics: RefCell::new(Vec::new()),
        validated: Cell::new(0),
        limits: builder.structural_limits,
    };
    let seeds = template_seeds(schema, previous, changed, changed_fields)?;
    let mut affected = previous.template_dependencies.affected(&seeds);
    affected.extend(changed.iter().copied());
    let ResolvedIncremental {
        records: replacements,
        reads,
        resolved_root_fields,
        reused_literal_fields,
    } = resolve_replacements(&builder, previous, schema, &drafts, &affected)?;
    let mut model = previous.clone();
    let changed_records =
        apply_replacements(&mut model, &builder, &drafts, replacements, &input_ranges);
    model.input_manifest = builder.records.manifest().into();
    model.dimension_input_index = Some(Arc::new(drafts.dimensions.index.clone()));
    let context = SemanticContext(&model);
    for id in &affected {
        let record = model.record(*id)?;
        for (name, value) in record.fields() {
            let field = schema.field(record.actual_type(), name)?;
            let request = ValueValidationRequest::new(
                &field.value_type,
                value,
                ValueValidationMode::Complete,
            );
            if crate::validate_value_for_schema(builder.schema, &context, request).is_err() {
                return None;
            }
        }
    }
    model.template_dependencies = update_template_dependencies(&model, schema, &affected, &reads)?;
    let mut refs = previous.refs.as_ref().clone();
    let rebuilt_ref_edges = refs.replace_hosts(
        changed_records
            .iter()
            .filter_map(|id| model.record(*id).map(|record| (*id, record))),
        &model.record_by_domain_id,
        schema,
    );
    model.refs = Arc::new(refs);
    let statistics = CfdModelBuildStats {
        resolved_root_fields,
        reused_literal_fields,
        dimension_inputs_checked: drafts.dimensions.checked,
        identity_records_checked: input_records_checked,
        origin_records_checked: input_records_checked,
        validated_records: drafts.validated.get(),
        materialized_records: affected.len(),
        reused_records: previous.record_count() - changed_records.len(),
        rebuilt_ref_hosts: changed_records.len(),
        rebuilt_ref_edges,
    };
    Some(CfdModelBuildOutput {
        model,
        diagnostics: CfdDiagnostics::new(Vec::new()),
        statistics,
        changed_records,
    })
}

// 只更新读集变化的字段关系，未变化的依赖集合继续与旧代次共享。
fn update_template_dependencies(
    model: &CfdDataModel,
    schema: BuildSchema<'_>,
    affected: &BTreeSet<CfdRecordHandle>,
    reads: &TemplateReads,
) -> Option<Arc<TemplateDependencies>> {
    let mut dependencies = Arc::clone(&model.template_dependencies);
    for id in affected {
        let record = model.record(*id)?;
        for field in schema.full_fields(record.actual_type()) {
            let key = TemplateField {
                record: *id,
                field: field.name.clone(),
            };
            let old = dependencies.by_reader.get(&key).map(AsRef::as_ref);
            let new = reads.get(&key);
            if old == new
                || old.is_none_or(BTreeSet::is_empty) && new.is_none_or(BTreeSet::is_empty)
            {
                continue;
            }
            Arc::make_mut(&mut dependencies).replace(key, new.cloned().unwrap_or_default());
        }
    }
    Some(dependencies)
}

// 将直接变化字段转换为模板依赖起点，不扩大无关字段的失效范围。
fn template_seeds(
    schema: BuildSchema<'_>,
    previous: &CfdDataModel,
    changed: &[CfdRecordHandle],
    changed_fields: Option<&BTreeMap<CfdRecordHandle, BTreeSet<FieldName>>>,
) -> Option<Vec<TemplateField>> {
    let mut seeds = Vec::new();
    for id in changed {
        let record = previous.record(*id)?;
        if let Some(fields) = changed_fields.and_then(|changes| changes.get(id)) {
            for field in fields {
                schema.field(record.actual_type(), field)?;
                seeds.push(TemplateField {
                    record: *id,
                    field: field.clone(),
                });
            }
        } else {
            seeds.extend(
                schema
                    .full_fields(record.actual_type())
                    .map(|field| TemplateField {
                        record: *id,
                        field: field.name.clone(),
                    }),
            );
        }
    }
    Some(seeds)
}

// 身份与预算是复用已有句柄的前提，结构变化仍交给完整构建。
fn can_reuse_model(
    builder: &CfdModelBuilder<'_>,
    previous: &CfdDataModel,
    changed: &[CfdRecordHandle],
) -> Option<Vec<std::ops::Range<usize>>> {
    // 这些类别改变全局身份、诊断或预算语义，必须重新执行完整编译流程。
    if !previous.incremental_ready
        || previous.projection.is_some()
        || previous.schema_compilation != builder.schema.compilation_id()
        || previous.structural_limits != builder.structural_limits
        || builder.records.len() != previous.record_count()
        || changed
            .iter()
            .any(|id| id.index() >= previous.record_count())
    {
        return None;
    }
    let ranges = builder.records.changed_ranges(&previous.input_manifest);
    for range in &ranges {
        for index in range.clone() {
            let input = &builder.records[index];
            let record = previous.record(CfdRecordHandle::new(index))?;
            if input.id != record.id() || input.actual_type != record.actual_type() {
                return None;
            }
        }
    }
    Some(ranges)
}

struct ResolvedIncremental {
    records: BTreeMap<CfdRecordHandle, CfdRecord>,
    reads: TemplateReads,
    resolved_root_fields: usize,
    reused_literal_fields: usize,
}

// 所有受影响记录与维度先求值成功，再发布到新代次，失败时不修改旧快照。
fn resolve_replacements(
    builder: &CfdModelBuilder<'_>,
    previous: &CfdDataModel,
    schema: BuildSchema<'_>,
    drafts: &LazyDrafts<'_, '_>,
    affected: &BTreeSet<CfdRecordHandle>,
) -> Option<ResolvedIncremental> {
    let mut replacements = BTreeMap::<CfdRecordHandle, CfdRecord>::new();
    let mut diagnostics = Vec::new();
    let reads;
    let resolved_root_fields;
    let reused_literal_fields;
    {
        let mut resolver = ValueResolver::new(
            &schema,
            drafts,
            &previous.record_by_domain_id,
            &mut diagnostics,
            builder.structural_limits,
        );
        if !builder.dimension_variants.is_empty() {
            resolver.enable_literal_costs();
        }
        for id in affected {
            let old = previous.record(*id)?;
            let fields = resolver.resolve_record_fields(*id, None)?;
            replacements.insert(
                *id,
                CfdRecord {
                    id: old.id.clone(),
                    object: Arc::new(CfdObject::new(old.actual_type_name().clone(), fields)),
                    origin: builder.records[id.index()].origin.clone(),
                    dimension_fields: Arc::default(),
                },
            );
        }
        // 依赖在所有维度上下文取并集；闭包内记录重新求值每个已有变体，未影响记录保持共享。
        for (dimension, variants) in &builder.dimension_variants {
            for variant in variants {
                for id in affected {
                    let mut values = resolver
                        .resolve_record_fields(*id, Some((dimension.clone(), variant.clone())))?;
                    let record = replacements.get_mut(id)?;
                    for field in schema.full_fields(record.actual_type()) {
                        if field
                            .dimension
                            .as_ref()
                            .is_none_or(|binding| &binding.dimension != dimension)
                        {
                            continue;
                        }
                        let Some(value) = values.remove(&field.name) else {
                            continue;
                        };
                        let value = Arc::new(value);
                        let fields = Arc::make_mut(&mut record.dimension_fields);
                        let target = fields.entry(field.name.clone()).or_insert_with(|| {
                            CfdDimensionFieldValues {
                                dimension: dimension.clone(),
                                variants: BTreeMap::new(),
                                effective_variants: BTreeMap::new(),
                            }
                        });
                        if let Some(overlay) = drafts.dimensions.get(&(
                            *id,
                            field.name.clone(),
                            dimension.clone(),
                            variant.clone(),
                        )) {
                            target.variants.insert(
                                variant.clone(),
                                CfdDimensionValue {
                                    value: value.clone(),
                                    origin: overlay.origin.clone(),
                                },
                            );
                        }
                        target.effective_variants.insert(variant.clone(), value);
                    }
                }
            }
        }
        let reference_diagnostics = resolver.take_reference_diagnostics();
        reads = resolver.take_reads();
        resolved_root_fields = resolver.resolved_root_fields;
        reused_literal_fields = resolver.reused_literal_fields;
        // 任意新诊断交由全量构建统一归并，包括预算、循环、维度及可编辑错误，避免漏旧标签。
        if !reference_diagnostics.is_empty() {
            return None;
        }
    }
    if !diagnostics.is_empty() || !drafts.diagnostics.borrow().is_empty() {
        return None;
    }
    Some(ResolvedIncremental {
        records: replacements,
        reads,
        resolved_root_fields,
        reused_literal_fields,
    })
}

fn apply_replacements(
    model: &mut CfdDataModel,
    builder: &CfdModelBuilder<'_>,
    drafts: &LazyDrafts<'_, '_>,
    replacements: BTreeMap<CfdRecordHandle, CfdRecord>,
    input_ranges: &[std::ops::Range<usize>],
) -> Vec<CfdRecordHandle> {
    let records = Arc::make_mut(&mut model.records);
    let mut changed_records = Vec::new();
    for (id, replacement) in replacements {
        let old = &records[id.index()];
        if !old.object.same_stored_value(&replacement.object)
            || !same_dimension_values(old, &replacement)
        {
            changed_records.push(id);
            records[id.index()] = Arc::new(replacement);
        } else if old.origin != replacement.origin {
            let mut header = old.as_ref().clone();
            header.origin = replacement.origin;
            records[id.index()] = Arc::new(header);
        }
    }
    for index in input_ranges.iter().cloned().flatten() {
        let input = &builder.records[index];
        if records[index].origin != input.origin {
            // 文件前部长度变化只更新来源头部，object 与 dimension_fields 都保持共享。
            Arc::make_mut(&mut records[index]).origin = input.origin.clone();
        }
    }
    for key @ (host, field, _, variant) in &drafts.dimensions.changed {
        let Some(entry) = drafts.dimensions.get(key) else {
            continue;
        };
        let record = &records[host.index()];
        let origin_changed = record
            .dimension_fields
            .get(field)
            .and_then(|values| values.variants.get(variant))
            .is_some_and(|value| value.origin != entry.origin);
        if origin_changed {
            // 维度来源同样与值分离，位置刷新只复制相关字段的索引和共享值指针。
            let record = Arc::make_mut(&mut records[host.index()]);
            if let Some(value) = Arc::make_mut(&mut record.dimension_fields)
                .get_mut(field)
                .and_then(|values| values.variants.get_mut(variant))
            {
                value.origin = entry.origin.clone();
            }
        }
    }
    changed_records
}

fn same_dimension_values(left: &CfdRecord, right: &CfdRecord) -> bool {
    left.dimension_fields.len() == right.dimension_fields.len()
        && left.dimension_fields.iter().all(|(name, left)| {
            right.dimension_fields.get(name).is_some_and(|right| {
                left.dimension == right.dimension
                    && left.effective_variants.len() == right.effective_variants.len()
                    && left.effective_variants.iter().all(|(variant, left)| {
                        right
                            .effective_variants
                            .get(variant)
                            .is_some_and(|right| left.same_stored_value(right))
                    })
                    && left.variants.len() == right.variants.len()
                    && left.variants.iter().all(|(variant, left)| {
                        right
                            .variants
                            .get(variant)
                            .is_some_and(|right| left.value.same_stored_value(&right.value))
                    })
            })
        })
}

struct SemanticContext<'a>(&'a CfdDataModel);

impl CfdValueSemanticContext for SemanticContext<'_> {
    fn record_by_domain_id(
        &self,
        inheritance_root: &TypeName,
        id: &str,
    ) -> Option<CfdRecordHandle> {
        self.0.record_by_domain_id(inheritance_root, id)
    }

    fn record_actual_type(&self, id: CfdRecordHandle) -> Option<&str> {
        self.0.record(id).map(CfdRecord::actual_type)
    }
}
