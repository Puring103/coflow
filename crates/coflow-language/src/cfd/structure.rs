//! 成功解析的 CFD AST 与解析器结构节点预算使用同一计数口径。

use super::{CfdAst, CfdBitExpr, CfdBitExprKind, CfdField, CfdRecord, CfdValue};

impl CfdAst {
    /// Returns the structural node charge of a successfully parsed source.
    #[must_use]
    pub fn structural_node_count(&self) -> u64 {
        self.records.iter().map(record_nodes).sum::<u64>()
            + self
                .dimensions
                .iter()
                .map(|dimension| 1 + dimension.records.iter().map(record_nodes).sum::<u64>())
                .sum::<u64>()
    }
}

impl CfdValue {
    /// Returns the parser's structural node charge for this value subtree.
    #[must_use]
    pub fn structural_node_count(&self) -> u64 {
        // parse_value 为每个值收费一次；对象还包含 parse_block 的节点，
        // 位运算只额外计算二元运算符，叶操作数与括号不另收费。
        1 + match self {
            Self::OptionSome(inner, _) => inner.structural_node_count(),
            Self::Block(block) => 1 + field_nodes(&block.fields),
            Self::Array(items, _) => items.iter().map(Self::structural_node_count).sum(),
            Self::BitExpr(expression) => binary_nodes(expression),
            Self::Scalar(_, _)
            | Self::QuotedString(_, _)
            | Self::FormattedString(_)
            | Self::OptionNone(_)
            | Self::Ref(_) => 0,
        }
    }
}

fn record_nodes(record: &CfdRecord) -> u64 {
    // 记录与其根 block 各收费一次；分组语法没有额外节点收费。
    2 + field_nodes(&record.fields)
}

fn field_nodes(fields: &[CfdField]) -> u64 {
    fields
        .iter()
        .map(|field| 1 + field.value.structural_node_count())
        .sum()
}

fn binary_nodes(expression: &CfdBitExpr) -> u64 {
    match &expression.kind {
        CfdBitExprKind::Value(_) => 0,
        CfdBitExprKind::Binary { lhs, rhs, .. } => 1 + binary_nodes(lhs) + binary_nodes(rhs),
    }
}

#[cfg(test)]
mod tests {
    use crate::cfd::{parse_cfd, parse_cfd_with_options, CfdParseOptions};
    use crate::limits::StructuralLimits;

    #[test]
    fn ast_node_counts_match_the_parser_at_the_exact_budget_boundary() {
        for source in [
            "",
            "one: Row {}",
            "Row { one {} two: Child { value: 1 } }",
            "locale[en] { one: Row { title: \"hello\" } }",
            r#"one: Row { plain: 1, text: "x", formatted: f"{self.name}", none: None,
                some: Some(Some(None)), object: Node { value: 2 },
                array: [1, None, Some(Node {})], dict: { "a": true },
                reference: &Row::two, bits: (A | B) & (C ^ D) }"#,
        ] {
            let (ast, diagnostics) = parse_cfd(source);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            let nodes = ast.structural_node_count();
            let options = CfdParseOptions {
                structural_limits: StructuralLimits::new(256, nodes, 10_000_000),
            };
            let (_, diagnostics) = parse_cfd_with_options(source, options);
            assert!(
                diagnostics.is_empty(),
                "exact boundary {nodes}: {diagnostics:?}"
            );
            if nodes > 0 {
                let (_, diagnostics) = parse_cfd_with_options(
                    source,
                    CfdParseOptions {
                        structural_limits: StructuralLimits::new(256, nodes - 1, 10_000_000),
                    },
                );
                assert!(
                    diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.message.contains("nodes limit")),
                    "one node below {nodes}: {diagnostics:?}"
                );
            }
        }
    }
}
