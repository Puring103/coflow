//! 字段批次按源码依赖分组：独立补丁只生成一次文本，重叠修改保持请求顺序。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use coflow_language::cfd::{parse_cfd_with_options, CfdAst, CfdParseOptions};

use super::patch::{apply_patch, field_node_change, field_patch, find_record, FieldPatch};
use super::{diag, ensure_parse_ok, CfdWriter};
use crate::api::{DiagnosticSet, WriteBatchFailure, WriteCellRequest, WriteOutcome};
use crate::data_model::RecordOrigin;

impl CfdWriter {
    #[cfg(test)]
    pub(crate) fn field_batch_parse_count(&self) -> usize {
        self.field_batch_parses
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn write_field_batch(
        &self,
        requests: &[WriteCellRequest<'_>],
    ) -> Result<Vec<WriteOutcome>, WriteBatchFailure> {
        self.write_field_batch_with_options(requests, CfdParseOptions::default())
    }

    pub(super) fn write_field_batch_with_options(
        &self,
        requests: &[WriteCellRequest<'_>],
        options: CfdParseOptions,
    ) -> Result<Vec<WriteOutcome>, WriteBatchFailure> {
        let Some(first) = requests.first() else {
            return Ok(Vec::new());
        };
        let path = request_path(first).map_err(|diagnostics| failure(0, diagnostics))?;
        let mut source = self
            .read_source(path)
            .map_err(|diagnostics| failure(0, diagnostics))?;
        let mut ast = self
            .parse_field_source(path, &source, options)
            .map_err(|diagnostics| failure(0, diagnostics))?;
        let mut start = 0;
        while start < requests.len() {
            let (patches, end) = plan_group(path, &source, &ast, requests, start)?;
            let candidate = patches.apply(&source);
            let next_ast = match self.parse_field_source(path, &candidate, options) {
                Ok(ast) => ast,
                Err(diagnostics) => {
                    // 只在候选语法失败时顺序定位首个错误，保留原操作下标和诊断位置。
                    // 成功批次不支付逐操作解析成本。
                    return Err(self.locate_batch_failure(
                        path,
                        source,
                        ast,
                        requests,
                        start..end,
                        diagnostics,
                        options,
                    ));
                }
            };
            if let Some(index) = first_node_budget_overflow(
                &ast,
                &next_ast,
                &requests[start..end],
                options.structural_limits.max_nodes,
            )
            .map_err(|diagnostics| failure(start, diagnostics))?
            {
                // 最终源码可以在后续收缩后重新低于上限，但中间超限仍必须失败。
                // 只对越界前缀重建源码，让真实解析器给出该步骤的原始诊断位置。
                let index = start + index;
                let (prefix, _) = plan_group(path, &source, &ast, &requests[..=index], start)?;
                let prefix_source = prefix.apply(&source);
                self.parse_field_source(path, &prefix_source, options)
                    .map_err(|diagnostics| failure(index, diagnostics))?;
            }
            source = candidate;
            ast = next_ast;
            start = end;
        }
        // 复用刚验证的 AST 立即降低，AST 随文件释放，避免跨文件批次积累大量语法树。
        let preloaded =
            crate::cfd_loader::CfdLoader::load_parsed_partial(first.schema, path, &source, &ast);
        drop(ast);
        self.stage_preloaded_source(path, source, Some(preloaded))
            .map_err(|diagnostics| failure(requests.len() - 1, diagnostics))?;
        Ok(vec![WriteOutcome::default(); requests.len()])
    }

    fn parse_field_source(
        &self,
        path: &Path,
        source: &str,
        options: CfdParseOptions,
    ) -> Result<CfdAst, DiagnosticSet> {
        #[cfg(test)]
        self.field_batch_parses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (ast, diagnostics) = parse_cfd_with_options(source, options);
        ensure_parse_ok(path, source, &diagnostics)?;
        Ok(ast)
    }

    fn locate_batch_failure(
        &self,
        path: &Path,
        mut source: String,
        mut ast: CfdAst,
        requests: &[WriteCellRequest<'_>],
        range: std::ops::Range<usize>,
        diagnostics: DiagnosticSet,
        options: CfdParseOptions,
    ) -> WriteBatchFailure {
        let last = range.end - 1;
        for index in range {
            source = match apply_patch(&source, &ast, &requests[index]) {
                Ok(source) => source,
                Err(diagnostics) => return failure(index, diagnostics),
            };
            ast = match self.parse_field_source(path, &source, options) {
                Ok(ast) => ast,
                Err(diagnostics) => return failure(index, diagnostics),
            };
        }
        failure(last, diagnostics)
    }
}

fn first_node_budget_overflow(
    before: &CfdAst,
    after: &CfdAst,
    requests: &[WriteCellRequest<'_>],
    max_nodes: u64,
) -> Result<Option<usize>, DiagnosticSet> {
    if requests.len() <= 1 {
        return Ok(None);
    }
    let mut nodes = before.structural_node_count();
    // 纯字段补丁保持记录身份和顺序；索引只保存借用，避免每个请求遍历整文件。
    let mut records = HashMap::with_capacity(before.records.len());
    for (old, new) in before.records.iter().zip(&after.records) {
        records
            .entry((old.type_name.as_str(), old.id.as_str()))
            .or_insert((old, new));
    }
    for (index, request) in requests.iter().enumerate() {
        let Some(&(old, new)) = records.get(&(request.actual_type, request.record_id)) else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "written record is absent from the parsed batch candidate",
            )));
        };
        let (removed, added) = field_node_change(old, new, request)?;
        nodes = nodes - removed + added;
        if nodes > max_nodes {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn request_path<'a>(request: &'a WriteCellRequest<'_>) -> Result<&'a Path, DiagnosticSet> {
    match request.origin {
        RecordOrigin::File { path, .. } => Ok(path),
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "cfd writer requires a File origin",
        ))),
    }
}

fn failure(index: usize, diagnostics: DiagnosticSet) -> WriteBatchFailure {
    WriteBatchFailure { index, diagnostics }
}

fn plan_group(
    path: &Path,
    source: &str,
    ast: &CfdAst,
    requests: &[WriteCellRequest<'_>],
    start: usize,
) -> Result<(PatchGroup, usize), WriteBatchFailure> {
    let mut records = HashMap::new();
    if requests.len() - start > 1 {
        records.reserve(ast.records.len());
        for record in &ast.records {
            // 与单次写入一致：若原文有重复身份，定位第一个记录。
            records
                .entry((record.type_name.as_str(), record.id.as_str()))
                .or_insert(record);
        }
    }
    let mut patches = PatchGroup::default();
    let mut end = start;
    for (index, request) in requests.iter().enumerate().skip(start) {
        let patch = (|| {
            if request_path(request)? != path {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "cfd field batch must target one source file",
                )));
            }
            // 高频单字段写入只顺序定位一次，不为整个文件额外构建哈希索引。
            let record = if requests.len() - start == 1 {
                find_record(ast, request.actual_type, request.record_id)
            } else {
                records
                    .get(&(request.actual_type, request.record_id))
                    .copied()
            }
            .ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!(
                        "record `{}.{}` not found in AST",
                        request.actual_type, request.record_id,
                    ),
                ))
            })?;
            field_patch(source, record, request)
        })();
        match patch {
            Ok(patch) => {
                let independent_syntax = patch.independent_syntax;
                if !independent_syntax && end > start {
                    break;
                }
                if !patches.insert(source, patch) {
                    break;
                }
                end = index + 1;
                if !independent_syntax {
                    break;
                }
            }
            // 先应用前面的修改，再重新定位当前请求；父对象替换可能改变路径可达性。
            Err(_) if end > start => break,
            Err(diagnostics) => return Err(failure(index, diagnostics)),
        }
    }
    Ok((patches, end))
}

#[derive(Default)]
struct PatchGroup {
    patches: BTreeMap<usize, PendingPatch>,
    newline_changes: BTreeMap<usize, usize>,
}

struct PendingPatch {
    patch: FieldPatch,
    inserted_fields: BTreeSet<String>,
}

impl PatchGroup {
    fn insert(&mut self, source: &str, patch: FieldPatch) -> bool {
        if let Some(line_start) = patch.formatting_start {
            if self
                .newline_changes
                .range(..patch.span.start)
                .next_back()
                .is_some_and(|(_, end)| *end >= line_start)
            {
                // 前序跨行修改会改变同一行后续复杂值的缩进，必须在新源码上重新渲染。
                return false;
            }
        }
        if let Some(existing) = self.patches.get_mut(&patch.span.start) {
            if let (Some(_), Some(current)) = (&existing.patch.insertion, &patch.insertion) {
                if existing.patch.span == patch.span
                    && existing.inserted_fields.insert(current.field.clone())
                {
                    // 同一对象末尾的不同缺省字段按请求顺序追加，共用原有逗号与注释。
                    if !existing.patch.replacement.ends_with('\n') {
                        existing.patch.replacement.push('\n');
                    }
                    existing
                        .patch
                        .replacement
                        .push_str(&patch.replacement[current.fragment_start..]);
                    return true;
                }
            }
            return false;
        }
        if self
            .patches
            .range(..patch.span.start)
            .next_back()
            .is_some_and(|(_, previous)| previous.patch.span.end > patch.span.start)
            || self
                .patches
                .range(patch.span.start..)
                .next()
                .is_some_and(|(start, _)| *start < patch.span.end)
        {
            return false;
        }
        if source[patch.span.start..patch.span.end].contains('\n')
            || patch.replacement.contains('\n')
        {
            self.newline_changes
                .insert(patch.span.start, patch.span.end);
        }
        let inserted_fields = patch
            .insertion
            .as_ref()
            .map(|insertion| BTreeSet::from([insertion.field.clone()]))
            .unwrap_or_default();
        self.patches.insert(
            patch.span.start,
            PendingPatch {
                patch,
                inserted_fields,
            },
        );
        true
    }

    fn apply(self, source: &str) -> String {
        let capacity = self.patches.values().fold(source.len(), |size, entry| {
            size - (entry.patch.span.end - entry.patch.span.start) + entry.patch.replacement.len()
        });
        let mut result = String::with_capacity(capacity);
        let mut cursor = 0;
        for entry in self.patches.into_values() {
            result.push_str(&source[cursor..entry.patch.span.start]);
            result.push_str(&entry.patch.replacement);
            cursor = entry.patch.span.end;
        }
        result.push_str(&source[cursor..]);
        result
    }
}
