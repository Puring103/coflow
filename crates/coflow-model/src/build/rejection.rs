use super::{
    BuildSchema, CfdModelBuilder, CfdTemplateRejections, RecordDraft, Validator, ValueResolver,
};
use crate::{CfdDiagnostics, CfdModelBuildStats, CfdPath, CfdRecordHandle};
use std::collections::BTreeSet;

/// 原逐轮构建中，成功的基础记录只会因实际读取的目标被剔除而改变结果。
/// 保持原输入句柄及升序求值顺序，每轮只重算这些读者，最后仍由普通构建生成完整模型。
#[expect(
    clippy::too_many_lines,
    reason = "拒绝闭包按原轮次顺序更新读者和身份，集中保留诊断、预算及剔除顺序。"
)]
pub(super) fn template_rejection_closure(builder: &CfdModelBuilder<'_>) -> CfdTemplateRejections {
    let mut result = CfdTemplateRejections {
        rejected_records: Vec::new(),
        diagnostics: CfdDiagnostics::new(Vec::new()),
        statistics: CfdModelBuildStats::default(),
    };
    let schema = BuildSchema::new(builder.schema);
    // singleton 的成员数量/键诊断会随每轮剔除变化，交由原有索引阶段确定顺序。
    if schema.singleton_types().next().is_some() {
        return result;
    }
    let mut errors = Vec::new();
    let mut drafts = Vec::<RecordDraft>::with_capacity(builder.records.len());
    let mut validator = Validator::new(&schema, &mut errors, builder.structural_limits);
    for index in 0..builder.records.len() {
        let input = &builder.records[index];
        result.statistics.validated_records += 1;
        if let Some(mut draft) = validator.validate_top_level_record(
            None,
            &input.id,
            &input.actual_type,
            &input.fields,
            Some(CfdRecordHandle::new(index)),
            CfdPath::root(),
        ) {
            draft.origin = input.origin.clone();
            result.statistics.origin_records_checked += 1;
            drafts.push(draft);
        }
    }
    // 可编辑诊断也不能略过：原算法会在阻断失败时把它们一同发布并参与剔除。
    if !errors.is_empty() || drafts.len() != builder.records.len() {
        return result;
    }
    let mut indexes = crate::indexes::build_indexes(schema, &drafts, &mut errors);
    result.statistics.identity_records_checked += drafts.len();
    if !errors.is_empty() {
        return result;
    }

    let mut active = vec![true; drafts.len()];
    let mut pending = (0..drafts.len()).collect::<BTreeSet<_>>();
    let mut reads = vec![BTreeSet::<usize>::new(); drafts.len()];
    let mut readers = vec![BTreeSet::<usize>::new(); drafts.len()];
    while !pending.is_empty() {
        let mut blocking = Vec::new();
        let mut references = Vec::new();
        {
            // 每轮重新建立循环报告集合，与普通构建的重复循环去重边界相同。
            let mut resolver = ValueResolver::new(
                &schema,
                drafts.as_slice(),
                &indexes.record_by_domain_id,
                &mut blocking,
                builder.structural_limits,
            );
            resolver.enable_record_reads();
            for index in std::mem::take(&mut pending) {
                if !active[index] {
                    continue;
                }
                for target in std::mem::take(&mut reads[index]) {
                    readers[target].remove(&index);
                }
                // 不缓存失败或部分字段值；原根预算、字段顺序、内联/集合与模板语义完整执行。
                let _ = resolver.resolve_record_fields(CfdRecordHandle::new(index), None);
                references.extend(resolver.take_reference_diagnostics());
                for target in resolver.take_record_reads() {
                    reads[index].insert(target.index());
                    readers[target.index()].insert(index);
                }
                // 字段级读集不参与本算法，及时释放，避免每个拒绝轮重复持有历史轨迹。
                let _ = resolver.take_reads();
            }
            result.statistics.resolved_root_fields += resolver.resolved_root_fields;
            result.statistics.reused_literal_fields += resolver.reused_literal_fields;
        }
        if blocking.is_empty() {
            // 仅引用诊断不剔除记录；之后的完整构建负责引用验证和所有单维上下文。
            break;
        }
        // 普通编译器先发布阻断求值诊断，再追加可编辑引用诊断，必须保留这一顺序。
        blocking.extend(references);
        let rejected = blocking
            .iter()
            .flat_map(|diagnostic| diagnostic.primary.iter().chain(&diagnostic.related))
            .filter_map(|label| label.record.map(CfdRecordHandle::index))
            .filter(|&index| active[index])
            .collect::<BTreeSet<_>>();
        if rejected.is_empty() {
            // 无记录定位的全局错误必须由普通构建返回，不能提交先前的局部剔除。
            result.rejected_records.clear();
            result.diagnostics.diagnostics.clear();
            return result;
        }
        result.diagnostics.diagnostics.extend(blocking);
        for index in rejected {
            active[index] = false;
            result.rejected_records.push(CfdRecordHandle::new(index));
            let draft = &drafts[index];
            if let Some(root) = schema.inheritance_root(draft.actual_type.as_str()) {
                if let Some(domain) = indexes.record_by_domain_id.get_mut(root) {
                    domain.remove(draft.id.as_str());
                }
            }
            // 下一轮只访问被本轮删除身份影响的根，避免 N+(N-1)+... 的全表重建。
            pending.extend(
                readers[index]
                    .iter()
                    .copied()
                    .filter(|&reader| active[reader]),
            );
            for target in std::mem::take(&mut reads[index]) {
                readers[target].remove(&index);
            }
        }
    }
    result
}
