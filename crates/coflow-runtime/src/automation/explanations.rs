//! 错误码说明按实际枚举穷举匹配，不从展示文档或消息文本猜测错误身份。
use super::{Failure, Result};
use crate::CfdErrorCode;
use coflow_language::diagnostics::CftErrorCode;
use serde::Serialize;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "测试的前置步骤失败应直接报告失败。")]
    #[test]
    fn every_language_and_model_code_has_an_exact_explanation_and_public_index_entry() {
        let reference =
            include_str!("../../../../website/docs/docs/reference/09-diagnostics/02-codes.md");
        let codes = coflow_language::diagnostics::CftErrorCode::ALL
            .iter()
            .map(|code| code.as_str())
            .chain(crate::CfdErrorCode::ALL.iter().map(|code| code.as_str()))
            .chain(
                crate::cfd_loader::CfdTextErrorCode::ALL
                    .iter()
                    .map(|code| code.as_str()),
            )
            .chain(super::RUNTIME_CODES.iter().map(|entry| entry.code));
        let mut seen = std::collections::BTreeSet::new();
        for code in codes {
            assert!(seen.insert(code), "duplicate code {code}");
            let explanation = super::explain(code).unwrap();
            assert_eq!(explanation.code, code);
            assert!(!explanation.trigger.is_empty() && !explanation.repair.is_empty());
            assert!(
                reference.contains(&format!("| `{code}` | {} |", explanation.meaning)),
                "{code}"
            );
        }
        for unknown in ["", "CFT", "DATA-00", "CFT-SCHEMA-006-extra", "UNKNOWN-999"] {
            assert!(super::explain(unknown).is_err(), "{unknown}");
        }
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiagnosticExplanation {
    pub code: String,
    pub stage: String,
    pub meaning: String,
    pub trigger: String,
    pub repair: String,
    pub queries: Vec<String>,
}

pub fn explain(code: &str) -> Result<DiagnosticExplanation> {
    let (stage, info, queries) = if let Some(code) = CftErrorCode::ALL
        .iter()
        .copied()
        .find(|entry| entry.as_str() == code)
    {
        (
            code.stage().to_string(),
            cft_info(code),
            vec!["schema check --explain", "schema inspect", "schema symbols"],
        )
    } else if let Some(code) = CfdErrorCode::ALL
        .iter()
        .copied()
        .find(|entry| entry.as_str() == code)
    {
        (
            code.stage().to_string(),
            cfd_info(code),
            vec!["check --explain", "schema inspect", "data get"],
        )
    } else if let Some(code) = crate::cfd_loader::CfdTextErrorCode::ALL
        .iter()
        .copied()
        .find(|entry| entry.as_str() == code)
    {
        (
            "CFD".into(),
            cfd_text_info(code),
            vec!["source read", "schema inspect", "check --explain"],
        )
    } else if let Some(entry) = RUNTIME_CODES.iter().find(|entry| entry.code == code) {
        (
            entry.stage.into(),
            (entry.meaning, entry.trigger, entry.repair),
            vec!["describe", "project inspect", "project files"],
        )
    } else {
        return Err(Failure::missing(format!(
            "unknown diagnostic code `{code}`"
        )));
    };
    Ok(DiagnosticExplanation {
        code: code.into(),
        stage,
        meaning: info.0.into(),
        trigger: info.1.into(),
        repair: info.2.into(),
        queries: queries.into_iter().map(String::from).collect(),
    })
}

pub(crate) fn for_diagnostic(diagnostic: &crate::Diagnostic) -> Option<serde_json::Value> {
    let mut explanation = super::json(explain(&diagnostic.code).ok()?).ok()?;
    // 目录说明描述错误类别；当前消息保留实际失败的类型、字段与原因。
    explanation["message"] = serde_json::json!(diagnostic.message);
    Some(explanation)
}

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
const fn cft_info(code: CftErrorCode) -> (&'static str, &'static str, &'static str) {
    use CftErrorCode as C;
    match code {
        C::UnexpectedCharacter => (
            "非法字符",
            "源码出现语言不接受的字符",
            "移除非法字符或改为合法标识符、符号。",
        ),
        C::InvalidStringEscape => (
            "字符串转义无效",
            "字符串中的转义序列不合法",
            "按字符串转义规则改写该序列。",
        ),
        C::UnterminatedString => (
            "字符串未结束",
            "字符串缺少结束引号",
            "补齐结束引号并检查转义。",
        ),
        C::InvalidIntLiteral => (
            "整数字面量无效",
            "整数格式或范围不合法",
            "使用合法的有符号 64 位整数字面量。",
        ),
        C::InvalidFloatLiteral => (
            "浮点数字面量无效",
            "浮点数格式或范围不合法",
            "使用合法的有限浮点数字面量。",
        ),
        C::UnexpectedToken => (
            "意外的语法单元",
            "当前位置的符号不符合语法",
            "按诊断位置调整符号与语法结构。",
        ),
        C::UnexpectedEof => (
            "源码提前结束",
            "语法结构尚未完整就到达文件末尾",
            "补齐缺失的值、分隔符或结束符。",
        ),
        C::ExpectedIdentifier => (
            "缺少标识符",
            "声明或路径需要名称的位置不是标识符",
            "填入合法标识符。",
        ),
        C::ExpectedToken => (
            "缺少指定符号",
            "语法要求的符号缺失",
            "根据诊断消息补齐指定符号。",
        ),
        C::InvalidTopLevelItem => (
            "顶层声明无效",
            "文件顶层出现不支持的声明形式",
            "改为受支持的 CFT 声明。",
        ),
        C::InvalidChainComparison => (
            "链式比较无效",
            "比较运算采用了不支持的链式写法",
            "拆成独立比较并使用布尔运算连接。",
        ),
        C::CheckBlockMustBeLast => (
            "check 块位置无效",
            "类型的 check 块后仍有字段声明",
            "把字段放在 check 块之前。",
        ),
        C::InvalidAnnotationSyntax => (
            "注解语法无效",
            "注解名称或参数结构不符合语法",
            "按注解语法调整名称和参数。",
        ),
        C::InvalidCheckStatement => (
            "检查语句无效或规则为空",
            "check 块为空，或其中的语句结构不合法",
            "为规则添加有效断言，并按诊断位置修正对应检查语句。",
        ),
        C::DuplicateCheckBlock => (
            "同一继承链中的规则名称重复",
            "当前类型或其继承链存在同名 check 规则",
            "为冲突规则指定不同名称，保留各规则的声明顺序。",
        ),
        C::SyntaxStructureLimitExceeded => (
            "语法结构超出限制",
            "源码的嵌套或结构规模超过语言限制",
            "减少嵌套深度或拆分源码结构。",
        ),
        C::DuplicateModule => (
            "模块重复",
            "Schema 输入产生重复模块身份",
            "调整 Schema 输入范围以消除重复。",
        ),
        C::DuplicateGlobalName => (
            "全局名称重复",
            "多个声明使用相同全局名称",
            "为重复声明使用不同名称。",
        ),
        C::DuplicateFieldName => (
            "字段名称重复",
            "同一类型多次声明相同字段",
            "合并或重命名重复字段。",
        ),
        C::DuplicateEnumVariant => (
            "枚举成员重复",
            "同一枚举多次声明相同成员名称",
            "删除或重命名重复成员。",
        ),
        C::DuplicateEnumValue => (
            "枚举数值重复",
            "枚举成员使用了重复数值",
            "为成员使用不重复的合法数值。",
        ),
        C::UnknownNamedType => (
            "类型未声明",
            "声明中的类型名称无法解析",
            "声明该类型或改为已有类型。",
        ),
        C::ParentMustBeType => (
            "继承目标不是类型",
            "父名称指向不能作为父类型的声明",
            "改为允许继承的类型声明。",
        ),
        C::UnknownConst => (
            "常量未声明",
            "常量名称无法解析",
            "声明该常量或改为已有常量。",
        ),
        C::InheritanceCycle => (
            "类型继承成环",
            "父类型关系包含循环",
            "移除使继承关系成环的边。",
        ),
        C::InheritSealedType => (
            "继承 sealed 类型",
            "类型继承了禁止继承的父类型",
            "选择可继承的父类型或调整声明。",
        ),
        C::DuplicateInheritedField => (
            "继承字段重名",
            "字段与继承得到的字段名称冲突",
            "保留一个声明或使用不同字段名。",
        ),
        C::ConflictingTypeModifiers => (
            "类型修饰符冲突",
            "类型使用了不能同时成立的修饰符",
            "按类型规则调整修饰符组合。",
        ),
        C::InvalidDictKeyType => (
            "字典键类型无效",
            "字典声明使用了不支持的键类型",
            "使用语言允许的字典键类型。",
        ),
        C::InvalidDefaultExpression => (
            "默认值表达式无效",
            "字段默认值不能按默认值语法解释",
            "按字段类型改写默认值表达式。",
        ),
        C::DefaultTypeMismatch => (
            "默认值类型不匹配",
            "默认值无法赋给字段声明类型",
            "调整默认值或字段类型。",
        ),
        C::DefaultReferencesField => (
            "默认值引用字段",
            "默认值依赖了不允许引用的字段",
            "使用常量或合法默认值表达式。",
        ),
        C::InvalidEnumValueSequence => (
            "枚举数值序列无效",
            "枚举的显式或自动数值序列不合法",
            "调整成员数值和声明顺序。",
        ),
        C::InvalidFlagEnumValue => (
            "flag 枚举数值无效",
            "flag 成员的数值不符合位标志规则",
            "按 flag 枚举规则设置成员数值。",
        ),
        C::UnknownAnnotation => (
            "注解未定义",
            "声明使用了不支持的注解名称",
            "使用受支持的注解名称。",
        ),
        C::DuplicateAnnotation => (
            "注解重复",
            "同一目标重复使用不允许重复的注解",
            "保留一个对应注解。",
        ),
        C::AnnotationWithoutTarget => (
            "注解没有目标",
            "注解之后没有可绑定的声明",
            "把注解放在适用声明之前。",
        ),
        C::InvalidAnnotationTarget => (
            "注解目标无效",
            "注解用于不支持的声明类别",
            "把注解用于允许的目标类别。",
        ),
        C::InvalidAnnotationArgument => (
            "注解参数无效",
            "注解参数数量或值不符合要求",
            "按该注解的参数要求修改。",
        ),
        C::InvalidAnnotatedFieldType => (
            "注解字段类型无效",
            "被注解字段的类型不满足注解约束",
            "调整字段类型或注解。",
        ),
        C::StructRequiresSealedType => (
            "struct 缺少 sealed",
            "struct 声明不满足 sealed 要求",
            "按 struct 规则补齐 sealed 声明。",
        ),
        C::IdAsEnumRequiresEmptyEnum => (
            "idAsEnum 目标枚举非空",
            "记录键映射目标枚举已声明成员",
            "使用符合 idAsEnum 规则的空枚举。",
        ),
        C::EnumVariantOnNonEnum => (
            "枚举成员目标错误",
            "成员访问的目标不是枚举",
            "使用正确的枚举类型名称。",
        ),
        C::UnknownEnumVariant => (
            "枚举成员不存在",
            "枚举没有指定名称的成员",
            "选择已声明的枚举成员。",
        ),
        C::InvalidConstValue => (
            "常量值无效",
            "常量表达式不能产生合法常量值",
            "按常量类型和表达式规则改写。",
        ),
        C::ReservedIdentifier => (
            "使用了保留标识符",
            "声明名称属于语言保留名称",
            "使用非保留的合法标识符。",
        ),
        C::LocalizedOnInvalidTarget => (
            "localized 目标无效",
            "本地化注解用于不支持的目标",
            "按本地化注解规则调整目标。",
        ),
        C::LocalizedBucketNotIdentifier => (
            "本地化名称无效",
            "本地化名称不是合法标识符",
            "使用合法的本地化名称。",
        ),
        C::DimensionOnInvalidTarget => (
            "dimension 目标无效",
            "维度注解用于不支持的目标",
            "按维度注解规则调整目标。",
        ),
        C::DimensionNameNotIdentifier => (
            "维度名称无效",
            "维度名称不是合法标识符",
            "使用合法的维度名称。",
        ),
        C::SingletonOnAbstractType => (
            "抽象类型不能 singleton",
            "singleton 与抽象类型同时使用",
            "调整为符合 singleton 规则的具体类型。",
        ),
        C::SingletonIdAsEnumConflict => (
            "singleton 与 idAsEnum 冲突",
            "类型同时使用了不允许组合的身份注解",
            "按记录身份规则调整注解。",
        ),
        C::SchemaStructureLimitExceeded => (
            "Schema 结构超出限制",
            "声明规模或依赖结构超过语言限制",
            "减少声明规模或依赖嵌套。",
        ),
        C::DuplicateTopLevelCheck => (
            "项目级检查重名",
            "多个项目级检查使用相同名称",
            "为项目级检查使用不同名称。",
        ),
        C::TypeAliasCycle => ("类型别名成环", "别名展开形成循环", "移除循环的别名依赖。"),
        C::RequiredObjectCycle => (
            "必填对象依赖成环",
            "必填对象字段形成不能终止的对象结构",
            "调整对象关系使其可终止。",
        ),
        C::DefaultMaterializationCycle => (
            "默认值物化成环",
            "默认对象的展开依赖形成循环",
            "移除循环默认值依赖。",
        ),
        C::UnknownValueName => (
            "值名称未定义",
            "检查表达式中的值名称无法解析",
            "使用有效的字段、常量或绑定名称。",
        ),
        C::UnknownField => (
            "字段未定义",
            "检查表达式访问了不存在的字段",
            "根据目标类型选择有效字段。",
        ),
        C::TypeUnknownEnumVariant => (
            "检查中的枚举成员不存在",
            "检查表达式引用了未声明的枚举成员",
            "选择该枚举的合法成员。",
        ),
        C::TypeEnumVariantOnNonEnum => (
            "检查中的枚举目标错误",
            "检查表达式对非枚举使用成员访问",
            "使用正确的枚举类型。",
        ),
        C::OperatorTypeMismatch => (
            "运算符类型不匹配",
            "操作数类型不支持该运算符",
            "调整操作数类型或运算符。",
        ),
        C::ComparisonTypeMismatch => (
            "比较类型不匹配",
            "比较两侧的类型不能进行该比较",
            "使用可比较且匹配的值类型。",
        ),
        C::ConditionMustBeBool => (
            "检查条件不是 bool",
            "检查条件没有产生布尔值",
            "改为产生 bool 的条件表达式。",
        ),
        C::UnknownFunction => (
            "检查函数未定义",
            "表达式调用了不支持的函数",
            "使用受支持的检查函数。",
        ),
        C::FunctionArityMismatch => (
            "函数参数数量错误",
            "调用参数数量不符合函数签名",
            "按函数签名补齐或移除参数。",
        ),
        C::FunctionArgTypeMismatch => (
            "函数参数类型错误",
            "调用参数类型不符合函数签名",
            "调整参数值或类型。",
        ),
        C::FieldAccessOnNonObject => (
            "非对象不能访问字段",
            "字段路径的当前值不是对象类型",
            "根据声明类型调整字段路径。",
        ),
        C::IndexOnNonIndexable => (
            "目标不能使用下标",
            "下标访问用于不支持的类型",
            "使用支持下标的集合类型。",
        ),
        C::IndexTypeMismatch => (
            "下标类型不匹配",
            "下标值不符合数组索引或字典键类型",
            "按目标集合的索引类型修改。",
        ),
        C::InvalidIsPredicate => (
            "类型判断无效",
            "is 判断使用了无效目标或类型",
            "按类型判断规则修改目标。",
        ),
        C::QuantifierRequiresCollection => (
            "量词目标不是集合",
            "量词遍历目标不是支持的集合",
            "使用合法集合或记录集合。",
        ),
        C::UniqueUnsupportedElementType => (
            "unique 元素类型无效",
            "unique 的元素类型不受支持",
            "选择受支持的元素类型或投影。",
        ),
        C::BitwiseRequiresIntOrFlagEnum => (
            "位运算类型无效",
            "位运算用于非整数、非 flag 枚举",
            "使用 int 或兼容的 flag 枚举值。",
        ),
        C::ShiftRequiresInt => ("位移要求整数", "位移操作数不是 int", "使用整数操作数。"),
        C::RegexPatternMustBeLiteral => (
            "正则模式不是字面量",
            "正则模式未使用要求的字面量形式",
            "使用合法的字符串模式字面量。",
        ),
        C::InvalidRegexPattern => (
            "正则模式无效",
            "模式不能编译为合法正则表达式",
            "修正正则语法。",
        ),
        C::InvalidQuantifierBindings => (
            "量词绑定无效",
            "量词绑定的数量或形式不符合遍历目标",
            "按集合或记录集合规则调整绑定。",
        ),
        C::InvalidRecordSetQuery => (
            "记录集合查询无效",
            "记录集合查询的目标或形式不合法",
            "使用合法的记录类型和查询形式。",
        ),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
const fn cfd_info(code: CfdErrorCode) -> (&'static str, &'static str, &'static str) {
    use CfdErrorCode as C;
    match code {
        C::UnknownType => (
            "记录类型未定义",
            "记录使用了无法解析的类型",
            "声明该类型或改为已有记录类型。",
        ),
        C::AbstractRecordType => (
            "记录类型为抽象类型",
            "记录直接使用了抽象类型",
            "选择可赋给该类型的具体记录类型。",
        ),
        C::MissingObjectType => (
            "对象类型缺失",
            "对象没有可确定的类型身份",
            "提供可确定的合法对象类型。",
        ),
        C::ObjectTypeMismatch => (
            "对象类型不匹配",
            "对象实际类型不能赋给声明类型",
            "选择可赋给字段类型的具体对象类型。",
        ),
        C::UnknownField => (
            "数据字段未声明",
            "记录或对象包含未声明字段",
            "删除该赋值或补齐相应字段声明。",
        ),
        C::MissingRequiredField => (
            "必填字段缺失",
            "没有显式赋值且没有可用默认值",
            "提供该字段的合法值。",
        ),
        C::TypeMismatch => (
            "数据值类型不匹配",
            "数据值不能赋给字段声明类型",
            "按字段声明类型提供值。",
        ),
        C::InvalidEnumVariant => (
            "枚举值无效",
            "数据枚举值不符合目标枚举",
            "使用合法枚举成员或允许的 flag 组合。",
        ),
        C::DuplicateDictKey => (
            "字典键重复",
            "同一字典包含语义相同的键",
            "合并或移除重复键条目。",
        ),
        C::MissingIdField => (
            "记录身份信息缺失",
            "记录缺少要求的身份信息",
            "按记录身份规则补齐身份。",
        ),
        C::InvalidRecordId => (
            "记录键无效",
            "记录键不符合身份规则",
            "使用合法且符合目标类型规则的记录键。",
        ),
        C::DuplicateId => (
            "记录键重复",
            "同一身份范围内存在重复记录键",
            "为记录使用不重复的键。",
        ),
        C::DuplicatePolymorphicId => (
            "多态记录键重复",
            "多态记录身份范围内出现键冲突",
            "调整冲突记录的键。",
        ),
        C::SingletonRecordCountInvalid => (
            "singleton 记录数量无效",
            "singleton 类型的记录数量不符合要求",
            "按 singleton 身份规则调整记录数量。",
        ),
        C::SingletonKeyMissingOrInvalid => (
            "singleton 记录键无效",
            "singleton 记录没有使用规定身份",
            "使用该 singleton 的规定记录键。",
        ),
        C::SingletonKeyCollision => (
            "singleton 记录键冲突",
            "singleton 身份与其他记录冲突",
            "消除冲突的记录身份。",
        ),
        C::ValueDependencyCycle => (
            "值依赖成环",
            "记录值或插值依赖形成循环",
            "调整依赖路径以消除循环。",
        ),
        C::DataStructureLimitExceeded => (
            "数据结构超出限制",
            "数据规模或值嵌套超过结构限制",
            "减少结构规模或嵌套深度。",
        ),
        C::RefTargetNotFound => (
            "引用目标不存在",
            "记录引用不能找到目标记录",
            "创建目标或修正引用键。",
        ),
        C::RefTargetTypeMismatch => (
            "引用目标类型不匹配",
            "引用目标不能赋给声明的引用类型",
            "选择符合引用类型的目标记录。",
        ),
        C::CheckFailed => (
            "业务规则未通过",
            "业务检查条件不成立",
            "检查规则和诊断定位的业务值。",
        ),
        C::CheckEvalTypeError => (
            "规则求值类型错误",
            "求值时遇到不符合操作要求的类型",
            "检查字段值、表达式和规则依赖。",
        ),
        C::CheckIndexOutOfBounds => (
            "规则数组下标越界",
            "求值访问了不存在的数组位置",
            "调整下标或在规则中检查集合范围。",
        ),
        C::CheckMissingDictKey => (
            "规则字典键不存在",
            "求值读取了字典中不存在的键",
            "补齐所需键或先检查键是否存在。",
        ),
        C::CheckEmptyMinMax => (
            "空集合不能求极值",
            "min 或 max 的目标集合为空",
            "为集合提供值或先检查集合非空。",
        ),
        C::CheckComparisonFailed => (
            "规则比较未通过",
            "规则中的比较结果为假",
            "检查比较两侧的业务值与预期关系。",
        ),
        C::CheckBoolExpectedTrue => (
            "规则布尔条件未通过",
            "要求为 true 的条件结果为 false",
            "检查条件依赖值与业务要求。",
        ),
        C::CheckNegationFailed => (
            "规则否定条件未通过",
            "否定条件结果未满足规则",
            "检查被否定条件的业务值。",
        ),
        C::CheckAndFailed => (
            "规则合取条件未通过",
            "合取中的条件未全部成立",
            "检查失败的子条件与关联业务值。",
        ),
        C::CheckOrFailed => (
            "规则析取条件未通过",
            "析取中的条件均未满足要求",
            "检查各子条件与关联业务值。",
        ),
        C::CheckTypePredicateFailed => (
            "规则类型判断未通过",
            "值的实际类型不满足类型判断",
            "检查对象实际类型或类型条件。",
        ),
        C::CheckContainsFailed => (
            "规则包含条件未通过",
            "目标集合或字符串不包含要求值",
            "检查集合内容、目标值和包含条件。",
        ),
        C::CheckUniqueFailed => (
            "规则唯一性条件未通过",
            "要求唯一的值出现重复",
            "修改重复业务值或唯一性投影。",
        ),
        C::CheckMatchesFailed => (
            "规则模式匹配未通过",
            "字符串不满足要求的正则模式",
            "调整字符串内容或业务模式。",
        ),
        C::CheckAnyQuantifierFailed => (
            "any 规则未通过",
            "遍历结果没有满足条件的元素",
            "检查集合内容和量词条件。",
        ),
        C::CheckNoneQuantifierFailed => (
            "none 规则未通过",
            "遍历中存在满足禁止条件的元素",
            "检查命中元素和禁止条件。",
        ),
        C::CheckAllQuantifierFailed => (
            "all 规则未通过",
            "遍历中存在不满足条件的元素",
            "检查失败元素和量词条件。",
        ),
        C::CheckBudgetExceeded => (
            "规则执行预算耗尽",
            "规则执行工作量达到预算上限",
            "减少规则工作量或调整执行计划；重新检查完成状态。",
        ),
    }
}

const fn cfd_text_info(
    code: crate::cfd_loader::CfdTextErrorCode,
) -> (&'static str, &'static str, &'static str) {
    use crate::cfd_loader::CfdTextErrorCode as C;
    match code {
        C::Syntax => (
            "CFD 源码语法无效",
            "CFD 源码不能完整解析",
            "根据源码位置修正语法后重新查询。",
        ),
        C::UnknownType => (
            "CFD 记录或对象类型未定义",
            "源码类型标记无法在 Schema 中找到",
            "使用已声明的具体类型。",
        ),
        C::AbstractObjectType => (
            "CFD 对象使用抽象类型",
            "对象实际类型是不能直接构造的抽象类型",
            "使用可赋给字段类型的具体对象类型。",
        ),
        C::ObjectTypeMismatch => (
            "CFD 对象类型不匹配",
            "对象类型标记不能赋给字段声明类型",
            "选择符合字段声明的实际对象类型。",
        ),
        C::UnknownField => (
            "CFD 赋值字段未声明",
            "源码赋值使用了类型中不存在的字段",
            "查询声明并修正字段名称。",
        ),
        C::DuplicateField => (
            "CFD 字段重复赋值",
            "同一对象或记录多次赋值同一字段",
            "合并或删除重复赋值。",
        ),
        C::ReservedIdField => (
            "CFD 保留身份字段被赋值",
            "源码试图将记录身份作为普通字段赋值",
            "通过记录键或记录改名修改身份。",
        ),
        C::TypeMismatch => (
            "CFD 表达式类型不匹配",
            "源码表达式不能按字段声明类型加载",
            "按字段类型修正表达式。",
        ),
        C::InvalidEnumVariant => (
            "CFD 枚举表达式无效",
            "枚举成员或位组合不符合目标枚举",
            "使用合法成员或允许的 flag 组合。",
        ),
        C::ReferenceNeedsMarker => (
            "CFD 引用缺少标记",
            "引用表达式没有使用引用标记",
            "使用 &key 或 &Type::key 表达引用。",
        ),
    }
}

struct RuntimeCode {
    code: &'static str,
    stage: &'static str,
    meaning: &'static str,
    trigger: &'static str,
    repair: &'static str,
}

// 项目与命令执行的错误身份由精确目录匹配，诊断消息只承载具体失败上下文。
const RUNTIME_CODES: &[RuntimeCode] = &[
    RuntimeCode {
        code: "PROJECT-CONFIG-NOT-FOUND",
        stage: "PROJECT",
        meaning: "项目配置未找到",
        trigger: "指定目录及父目录没有可用项目配置",
        repair: "用 --project 指定项目目录或配置文件。",
    },
    RuntimeCode {
        code: "PROJECT-CONFIG-AMBIGUOUS",
        stage: "PROJECT",
        meaning: "项目配置有歧义",
        trigger: "项目目录同时存在多个受支持的配置文件名",
        repair: "明确保留要使用的配置文件或指定配置文件路径。",
    },
    RuntimeCode {
        code: "PROJECT-CONFIG-PATH",
        stage: "PROJECT",
        meaning: "项目配置路径无效",
        trigger: "配置路径不存在、类型不正确或无法解析",
        repair: "检查 --project 路径及文件访问条件。",
    },
    RuntimeCode {
        code: "PROJECT-CONFIG-READ",
        stage: "PROJECT",
        meaning: "项目配置读取失败",
        trigger: "配置文件不能读取",
        repair: "检查路径、访问权限和文件状态。",
    },
    RuntimeCode {
        code: "PROJECT-CONFIG-PARSE",
        stage: "PROJECT",
        meaning: "项目配置解析失败",
        trigger: "coflow.yaml 不符合配置语法或字段类型",
        repair: "按项目配置参考修正诊断位置的内容。",
    },
    RuntimeCode {
        code: "PROJECT-CONFIG-WRITE",
        stage: "PROJECT",
        meaning: "项目配置写入失败",
        trigger: "更新配置文件时写入失败",
        repair: "检查目标路径和写入条件。",
    },
    RuntimeCode {
        code: "PROJECT-CONFIG-OVERLAP",
        stage: "PROJECT",
        meaning: "项目输入范围重叠",
        trigger: "Schema 与数据输入包含或覆盖同一来源",
        repair: "调整输入路径以分离 Schema 与数据范围。",
    },
    RuntimeCode {
        code: "PROJECT-SCHEMA-PATH",
        stage: "PROJECT",
        meaning: "Schema 输入路径无效",
        trigger: "Schema 输入不存在或路径形式不合法",
        repair: "检查 Schema 输入路径和文件类别。",
    },
    RuntimeCode {
        code: "PROJECT-SCHEMA-READ",
        stage: "PROJECT",
        meaning: "Schema 输入读取失败",
        trigger: "Schema 文件或目录无法读取",
        repair: "检查输入路径及访问条件。",
    },
    RuntimeCode {
        code: "PROJECT-CWD",
        stage: "PROJECT",
        meaning: "当前目录无法读取",
        trigger: "项目定位无法获得调用目录",
        repair: "从可访问的目录调用或明确指定项目路径。",
    },
    RuntimeCode {
        code: "PROJECT-INIT-IO",
        stage: "PROJECT",
        meaning: "项目初始化失败",
        trigger: "创建项目文件时发生读写错误",
        repair: "检查目标目录及文件创建条件。",
    },
    RuntimeCode {
        code: "PROJECT-FILE-CREATE",
        stage: "PROJECT",
        meaning: "项目文件创建失败",
        trigger: "项目文件不能创建",
        repair: "检查目标是否已存在及目录写入条件。",
    },
    RuntimeCode {
        code: "PROJECT-FILE-DELETE",
        stage: "PROJECT",
        meaning: "项目文件删除失败",
        trigger: "项目文件不能删除",
        repair: "检查目标文件及删除条件。",
    },
    RuntimeCode {
        code: "PROJECT-FILE-RESTORE",
        stage: "PROJECT",
        meaning: "项目文件恢复失败",
        trigger: "文件操作失败后的恢复未完成",
        repair: "根据诊断路径核实实际内容并完成恢复。",
    },
    RuntimeCode {
        code: "CFD-READ",
        stage: "CFD",
        meaning: "CFD 写入来源读取失败",
        trigger: "编辑所需的 CFD 文件或来源结构不能读取",
        repair: "检查来源文件、记录位置和诊断消息。",
    },
    RuntimeCode {
        code: "CFD-WRITE",
        stage: "CFD",
        meaning: "CFD 源码修改失败",
        trigger: "请求不能安全地应用到 CFD 来源结构",
        repair: "检查字段路径、记录来源和请求操作。",
    },
    RuntimeCode {
        code: "CFD-SOURCE",
        stage: "CFD",
        meaning: "CFD 输入来源无效",
        trigger: "输入来源不能作为有效的 CFD 数据来源",
        repair: "检查数据输入配置与文件类别。",
    },
    RuntimeCode {
        code: "CLI-REQUEST",
        stage: "CLI",
        meaning: "命令请求无效",
        trigger: "参数或结构化请求不符合命令协议",
        repair: "使用 describe 查询命令要求并修正请求。",
    },
    RuntimeCode {
        code: "CLI-TARGET",
        stage: "CLI",
        meaning: "查询或修改目标不存在",
        trigger: "类型、记录、字段或其他请求目标不能找到",
        repair: "重新查询目标身份并使用已有目标。",
    },
    RuntimeCode {
        code: "CLI-CONFLICT",
        stage: "CLI",
        meaning: "输入或前置条件冲突",
        trigger: "输入已变化或请求的预期条件未满足",
        repair: "重新读取当前内容并重新准备请求。",
    },
    RuntimeCode {
        code: "CLI-IO",
        stage: "CLI",
        meaning: "请求载体读写失败",
        trigger: "请求文件、标准输入或其他文件操作失败",
        repair: "检查诊断中的路径与读写条件。",
    },
    RuntimeCode {
        code: "CLI-PUBLISH",
        stage: "CLI",
        meaning: "候选发布失败",
        trigger: "候选文件发布或失败恢复没有完成",
        repair:
            "根据 written_files、restored_files 和 restoration_failed 核实文件状态后重新准备请求。",
    },
    RuntimeCode {
        code: "CLI-CANDIDATE",
        stage: "CLI",
        meaning: "候选项目处理失败",
        trigger: "候选项目构建、路径映射或来源识别失败",
        repair: "检查项目输入路径、候选配置和诊断消息。",
    },
    RuntimeCode {
        code: "CLI-FIELD",
        stage: "CLI",
        meaning: "字段投影无效",
        trigger: "所选类型范围内字段路径无法解析",
        repair: "查询 Schema 并选择范围内有效字段。",
    },
    RuntimeCode {
        code: "CLI-PATH",
        stage: "CLI",
        meaning: "值路径无效",
        trigger: "请求的字段或集合路径不符合目标结构",
        repair: "按字段类型修正字段名、索引或字典键。",
    },
    RuntimeCode {
        code: "CLI-SORT",
        stage: "CLI",
        meaning: "排序字段无效",
        trigger: "字段无法作为所选范围的排序键",
        repair: "选择有效且类型一致的可排序字段。",
    },
    RuntimeCode {
        code: "CLI-FILTER",
        stage: "CLI",
        meaning: "筛选条件无效",
        trigger: "筛选表达式的字段或值类型不合法",
        repair: "按 --where 语法和字段声明类型修正条件。",
    },
    RuntimeCode {
        code: "CLI-VALUE",
        stage: "CLI",
        meaning: "写入值无效",
        trigger: "CFD 表达式或模型值不符合目标类型",
        repair: "按目标字段类型提供有效写入值。",
    },
    RuntimeCode {
        code: "ARTIFACT-IO",
        stage: "ARTIFACT",
        meaning: "代码产物读写失败",
        trigger: "生成目标或暂存文件不能读取、创建或写入",
        repair: "检查目标路径与文件权限，并依据发布和恢复结果重试。",
    },
    RuntimeCode {
        code: "ARTIFACT-001",
        stage: "ARTIFACT",
        meaning: "代码产物处理失败",
        trigger: "生成目录、稳定映射或文件发布不符合要求",
        repair: "检查生成目标、输入范围及发布与恢复结果。",
    },
    RuntimeCode {
        code: "CHECK-BUDGET",
        stage: "CHECK",
        meaning: "规则规划预算耗尽",
        trigger: "规则任务数量超过规划预算",
        repair: "减少规则任务范围或调整规则结构后重新检查。",
    },
    RuntimeCode {
        code: "CLI-LSP",
        stage: "CLI",
        meaning: "语言服务启动失败",
        trigger: "语言服务器运行或传输失败",
        repair: "检查项目路径、输入输出通道与诊断消息。",
    },
    RuntimeCode {
        code: "DIM-CONFIG-002",
        stage: "PROJECT",
        meaning: "维度目录配置无效",
        trigger: "维度输出目录与项目输入或其他维度目录冲突",
        repair: "按维度目录要求调整项目配置。",
    },
    RuntimeCode {
        code: "GIT-DIFF",
        stage: "GIT",
        meaning: "Git 差异读取失败",
        trigger: "Git 仓库、基准提交或历史文件不能读取",
        repair: "检查仓库状态与基准身份，候选比较可使用 diff --input。",
    },
    RuntimeCode {
        code: "GIT-EXTERNAL-SOURCE",
        stage: "GIT",
        meaning: "历史外部输入不可还原",
        trigger: "历史配置引用了指定仓库之外的输入",
        repair: "核对列出的历史输入路径与不完整范围。",
    },
    RuntimeCode {
        code: "PROJECT-001",
        stage: "PROJECT",
        meaning: "项目配置或生成目标无效",
        trigger: "项目输入、生成目标或稳定枚举映射不符合要求",
        repair: "根据配置定位修正输入与生成目标。",
    },
    RuntimeCode {
        code: "RUNTIME-INTERNAL",
        stage: "RUNTIME",
        meaning: "运行时内部状态无效",
        trigger: "项目数据处理遇到内部不一致",
        repair: "保留诊断与项目输入以定位运行时问题。",
    },
    RuntimeCode {
        code: "SCHEMA-STDIN-PATH",
        stage: "SCHEMA",
        meaning: "标准输入声明路径无效",
        trigger: "标准输入 Schema 路径不能映射为有效模块",
        repair: "使用合法的项目相对 CFT 路径。",
    },
    RuntimeCode {
        code: "MUTATION-COORDINATE",
        stage: "MUTATION",
        meaning: "记录坐标无效",
        trigger: "类型或记录键不符合身份规则",
        repair: "使用查询返回的合法记录身份。",
    },
    RuntimeCode {
        code: "MUTATION-DEFAULT",
        stage: "MUTATION",
        meaning: "默认值物化失败",
        trigger: "声明默认值不能转换为待编辑值",
        repair: "检查字段默认值及嵌套类型声明。",
    },
    RuntimeCode {
        code: "MUTATION-DIMENSION",
        stage: "MUTATION",
        meaning: "维度修改目标无效",
        trigger: "记录、字段绑定或变体身份不符合维度要求",
        repair: "核对记录身份、维度绑定和变体名称。",
    },
    RuntimeCode {
        code: "MUTATION-DIMENSION-DESTINATION",
        stage: "WRITE",
        meaning: "维度目标文件无效",
        trigger: "新覆盖无法按项目目录策略确定合法目标文件",
        repair: "配置 dimensions 的目录，项目外主体可显式指定目标文件。",
    },
    RuntimeCode {
        code: "MUTATION-DIMENSION-PATH",
        stage: "MUTATION",
        meaning: "维度值路径无效",
        trigger: "嵌套覆盖路径不符合字段类型",
        repair: "按覆盖字段声明与实际对象类型调整路径。",
    },
    RuntimeCode {
        code: "MUTATION-DIMENSION-STALE",
        stage: "MUTATION",
        meaning: "维度旧值条件冲突",
        trigger: "当前显式覆盖不符合请求的预期状态",
        repair: "重新读取覆盖值并重新准备请求。",
    },
    RuntimeCode {
        code: "MUTATION-FILE",
        stage: "MUTATION",
        meaning: "修改文件无效",
        trigger: "修改目标不属于允许的项目来源",
        repair: "使用项目选中的 CFD 来源文件。",
    },
    RuntimeCode {
        code: "MUTATION-FILE-GUARD",
        stage: "MUTATION",
        meaning: "来源文件条件冲突",
        trigger: "来源指纹与请求要求不一致",
        repair: "重新读取来源内容并更新前置条件。",
    },
    RuntimeCode {
        code: "MUTATION-INSERT",
        stage: "MUTATION",
        meaning: "记录创建请求无效",
        trigger: "待创建记录的类型、键或来源不符合要求",
        repair: "使用具体记录类型和合法来源文件。",
    },
    RuntimeCode {
        code: "MUTATION-INSERT-CONFLICT",
        stage: "MUTATION",
        meaning: "创建记录身份冲突",
        trigger: "待创建身份与候选记录集合冲突",
        repair: "查询现有身份并选择不冲突的记录键。",
    },
    RuntimeCode {
        code: "MUTATION-PATH",
        stage: "MUTATION",
        meaning: "修改路径无效",
        trigger: "字段或集合路径不能定位待修改值",
        repair: "核对字段、下标与类型化字典键。",
    },
    RuntimeCode {
        code: "MUTATION-RENAME",
        stage: "MUTATION",
        meaning: "记录改名请求无效",
        trigger: "改名目标或新身份不符合要求",
        repair: "使用合法的记录身份与新记录键。",
    },
    RuntimeCode {
        code: "MUTATION-RENAME-CONFLICT",
        stage: "MUTATION",
        meaning: "改名记录身份冲突",
        trigger: "新记录键与已有记录或批量请求冲突",
        repair: "选择不冲突的新记录键。",
    },
    RuntimeCode {
        code: "MUTATION-REORDER",
        stage: "MUTATION",
        meaning: "记录排序请求无效",
        trigger: "排序目标或移动位置不符合要求",
        repair: "核对记录身份与目标顺序位置。",
    },
    RuntimeCode {
        code: "MUTATION-REORDER-BATCH",
        stage: "MUTATION",
        meaning: "批量排序组合无效",
        trigger: "批量修改包含不能合并的记录排序操作",
        repair: "按来源容器整理排序操作。",
    },
    RuntimeCode {
        code: "MUTATION-REORDER-CONTAINER",
        stage: "MUTATION",
        meaning: "记录排序容器不一致",
        trigger: "排序记录不属于同一可写来源容器",
        repair: "在同一 CFD 文件内执行排序。",
    },
    RuntimeCode {
        code: "MUTATION-SHAPE",
        stage: "MUTATION",
        meaning: "修改值结构无效",
        trigger: "值结构不符合待修改字段形状",
        repair: "按声明类型提供对象、集合或基础值。",
    },
    RuntimeCode {
        code: "MUTATION-TRANSFER",
        stage: "MUTATION",
        meaning: "记录迁移请求无效",
        trigger: "迁移记录或目标位置不符合要求",
        repair: "核对来源记录与目标文件。",
    },
    RuntimeCode {
        code: "MUTATION-TRANSFER-FILE",
        stage: "MUTATION",
        meaning: "迁移目标文件无效",
        trigger: "迁移目标不能作为合法 CFD 文件",
        repair: "选择项目允许的 CFD 目标路径。",
    },
    RuntimeCode {
        code: "MUTATION-TXN-INVARIANT",
        stage: "MUTATION",
        meaning: "修改事务状态不一致",
        trigger: "候选执行计划或来源索引存在内部不一致",
        repair: "保留诊断与请求以定位事务问题。",
    },
    RuntimeCode {
        code: "MUTATION-TYPE",
        stage: "MUTATION",
        meaning: "修改值类型无效",
        trigger: "目标类型或实际值类型不符合修改要求",
        repair: "核对字段声明与对象实际类型。",
    },
    RuntimeCode {
        code: "MUTATION-VALUE",
        stage: "MUTATION",
        meaning: "修改值无效",
        trigger: "提交值不满足字段声明及引用要求",
        repair: "根据字段类型和诊断提供合法值。",
    },
    RuntimeCode {
        code: "WRITE-AMBIGUOUS-SOURCE",
        stage: "WRITE",
        meaning: "写入来源有歧义",
        trigger: "多个来源对应同一目标文件或记录",
        repair: "消除来源歧义后重新执行。",
    },
    RuntimeCode {
        code: "WRITE-NO-SOURCE",
        stage: "WRITE",
        meaning: "写入来源不存在",
        trigger: "待写目标没有可定位来源",
        repair: "使用项目中已有来源或创建新源文件。",
    },
    RuntimeCode {
        code: "WRITE-NOT-FOUND",
        stage: "WRITE",
        meaning: "写入记录不存在",
        trigger: "来源会话中没有目标记录",
        repair: "重新查询记录身份后提交修改。",
    },
    RuntimeCode {
        code: "WRITE-RENAME",
        stage: "WRITE",
        meaning: "记录键写入值无效",
        trigger: "记录键修改没有提供合法字符串",
        repair: "提供合法的新记录键。",
    },
    RuntimeCode {
        code: "WRITE-REORDER-CONTAINER",
        stage: "WRITE",
        meaning: "排序来源容器不一致",
        trigger: "排序记录不属于同一可写 CFD 文件",
        repair: "在同一来源文件内调整记录顺序。",
    },
    RuntimeCode {
        code: "WRITE-REORDER-INDEX",
        stage: "WRITE",
        meaning: "排序位置越界",
        trigger: "目标顺序位置超出来源容器长度",
        repair: "使用来源容器中的合法位置。",
    },
    RuntimeCode {
        code: "WRITE-REORDER-ORIGIN",
        stage: "WRITE",
        meaning: "排序来源不可写",
        trigger: "排序记录没有可写来源",
        repair: "使用来自 CFD 文件的记录。",
    },
    RuntimeCode {
        code: "WRITE-REORDER-TYPE",
        stage: "WRITE",
        meaning: "排序来源类型无效",
        trigger: "来源写入器不支持所选排序操作",
        repair: "核对记录来源与支持的操作。",
    },
    RuntimeCode {
        code: "WRITE-SHAPE",
        stage: "WRITE",
        meaning: "写入数据结构无效",
        trigger: "来源中的字段或值结构不能安全修改",
        repair: "核对源码结构与修改请求。",
    },
    RuntimeCode {
        code: "WRITE-TRANSFER-FILE",
        stage: "WRITE",
        meaning: "记录迁移文件无效",
        trigger: "迁移来源或目标文件不符合要求",
        repair: "使用有效的 CFD 文件并核对来源范围。",
    },
    RuntimeCode {
        code: "WRITE-TRANSFER-INDEX",
        stage: "WRITE",
        meaning: "记录迁移位置越界",
        trigger: "迁移插入位置超过目标文件的记录数量",
        repair: "使用目标文件中的合法插入位置。",
    },
    RuntimeCode {
        code: "WRITE-TRANSFER-ORIGIN",
        stage: "WRITE",
        meaning: "记录迁移来源不可写",
        trigger: "待迁移记录没有可写源码来源",
        repair: "使用具有 CFD 源码来源的记录。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-CONFIG",
        stage: "CLI",
        meaning: "更新服务配置无效",
        trigger: "更新服务参数缺失或配置不能读取",
        repair: "检查更新服务配置与版本信息。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-CONFIRM",
        stage: "CLI",
        meaning: "更新确认读取失败",
        trigger: "交互确认不能从输入通道读取",
        repair: "使用可交互终端或明确指定 --yes。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-INSTALL",
        stage: "CLI",
        meaning: "CLI 更新安装失败",
        trigger: "更新产物下载、替换或安装失败",
        repair: "检查网络、安装路径及写入权限。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-OUTPUT",
        stage: "CLI",
        meaning: "更新提示输出失败",
        trigger: "更新确认提示不能写入终端",
        repair: "检查终端输入输出通道。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-PLATFORM",
        stage: "CLI",
        meaning: "更新平台不受支持",
        trigger: "当前操作系统或架构没有对应更新产物",
        repair: "使用支持平台的发布产物。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-RELEASE",
        stage: "CLI",
        meaning: "更新版本查询失败",
        trigger: "发布信息不能获取或缺少对应产物",
        repair: "检查网络与发布服务返回信息。",
    },
    RuntimeCode {
        code: "SELF-UPDATE-VERSION",
        stage: "CLI",
        meaning: "更新版本信息无效",
        trigger: "当前或远端版本不能按版本规则解析",
        repair: "检查当前安装与发布版本标识。",
    },
    RuntimeCode {
        code: "SKILL-HOME",
        stage: "CLI",
        meaning: "技能安装目录无法确定",
        trigger: "全局安装所需用户目录不可用",
        repair: "配置合法的用户或代理配置目录。",
    },
    RuntimeCode {
        code: "SKILL-DIR-CREATE",
        stage: "CLI",
        meaning: "技能目录创建失败",
        trigger: "目标技能目录无法创建",
        repair: "检查目标路径及目录写入权限。",
    },
    RuntimeCode {
        code: "SKILL-REMOVE",
        stage: "CLI",
        meaning: "技能移除失败",
        trigger: "已安装的技能文件无法移除",
        repair: "检查目标文件权限和占用状态。",
    },
    RuntimeCode {
        code: "SKILL-BUNDLE",
        stage: "CLI",
        meaning: "技能包无效",
        trigger: "分发技能包缺少文件或结构不符合要求",
        repair: "检查当前 CLI 分发包完整性。",
    },
    RuntimeCode {
        code: "SKILL-SWAP",
        stage: "CLI",
        meaning: "技能包替换失败",
        trigger: "准备完成的技能包无法替换到目标目录",
        repair: "检查安装目录权限与文件状态。",
    },
    RuntimeCode {
        code: "SKILL-CLEANUP",
        stage: "CLI",
        meaning: "技能临时文件清理失败",
        trigger: "安装后的临时目录或备份无法清理",
        repair: "根据诊断路径清理残留文件。",
    },
    RuntimeCode {
        code: "SKILL-FILE-WRITE",
        stage: "CLI",
        meaning: "技能文件写入失败",
        trigger: "技能文件无法写入目标目录",
        repair: "检查目录写入权限及磁盘状态。",
    },
    RuntimeCode {
        code: "SKILL-MANIFEST",
        stage: "CLI",
        meaning: "技能安装清单无效",
        trigger: "安装清单不能读取、解析或满足当前协议",
        repair: "检查清单路径与安装完整性。",
    },
    RuntimeCode {
        code: "SKILL-DIR-READ",
        stage: "CLI",
        meaning: "技能目录读取失败",
        trigger: "安装目录不能枚举或读取",
        repair: "检查技能目录访问权限。",
    },
    RuntimeCode {
        code: "SKILL-DIR-REMOVE",
        stage: "CLI",
        meaning: "技能目录删除失败",
        trigger: "卸载目标目录不能删除",
        repair: "检查目标目录权限和占用状态。",
    },
    RuntimeCode {
        code: "WRITE-CONFLICT",
        stage: "WRITE",
        meaning: "写入来源内容冲突",
        trigger: "准备修改后来源文件被外部修改",
        repair: "重新读取来源并重新准备修改。",
    },
];
