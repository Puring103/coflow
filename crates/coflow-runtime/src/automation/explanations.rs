//! 错误码说明按实际枚举穷举匹配，不从展示文档或消息文本猜测错误身份。
use coflow_language::diagnostics::CftErrorCode;
use crate::CfdErrorCode;
use super::{Failure, Result};
use serde::Serialize;

#[cfg(test)]
mod tests {
    #[test]
    fn every_language_and_model_code_has_an_exact_explanation_and_public_index_entry() {
        let reference = include_str!("../../../../website/docs/docs/reference/09-diagnostics/02-codes.md");
        let codes = coflow_language::diagnostics::CftErrorCode::ALL.iter().map(|code| code.as_str())
            .chain(crate::CfdErrorCode::ALL.iter().map(|code| code.as_str()))
            .chain(super::RUNTIME_CODES.iter().map(|entry| entry.code));
        let mut seen = std::collections::BTreeSet::new();
        for code in codes {
            assert!(seen.insert(code), "duplicate code {code}");
            let explanation = super::explain(code).unwrap();
            assert_eq!(explanation.code, code);
            assert!(!explanation.trigger.is_empty() && !explanation.repair.is_empty());
            assert!(reference.contains(&format!("| `{code}` | {} |", explanation.meaning)), "{code}");
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
    let (stage, info, queries) = if let Some(code) = CftErrorCode::ALL.iter().copied().find(|entry| entry.as_str() == code) {
        (code.stage().to_string(), cft_info(code), vec!["schema check --explain", "schema inspect", "schema symbols"])
    } else if let Some(code) = CfdErrorCode::ALL.iter().copied().find(|entry| entry.as_str() == code) {
        (code.stage().to_string(), cfd_info(code), vec!["check --explain", "schema inspect", "data get"])
    } else if let Some(entry) = RUNTIME_CODES.iter().find(|entry| entry.code == code) {
        (entry.stage.into(), (entry.meaning, entry.trigger, entry.repair), vec!["describe", "project inspect", "project files"])
    } else { return Err(Failure::missing(format!("unknown diagnostic code `{code}`"))); };
    Ok(DiagnosticExplanation { code: code.into(), stage, meaning: info.0.into(), trigger: info.1.into(), repair: info.2.into(),
        queries: queries.into_iter().map(String::from).collect() })
}

fn cft_info(code: CftErrorCode) -> (&'static str, &'static str, &'static str) {
    use CftErrorCode as C;
    match code {
        C::UnexpectedCharacter => ("非法字符", "源码出现语言不接受的字符", "移除非法字符或改为合法标识符、符号。"),
        C::InvalidStringEscape => ("字符串转义无效", "字符串中的转义序列不合法", "按字符串转义规则改写该序列。"),
        C::UnterminatedString => ("字符串未结束", "字符串缺少结束引号", "补齐结束引号并检查转义。"),
        C::InvalidIntLiteral => ("整数字面量无效", "整数格式或范围不合法", "使用合法的有符号 64 位整数字面量。"),
        C::InvalidFloatLiteral => ("浮点数字面量无效", "浮点数格式或范围不合法", "使用合法的有限浮点数字面量。"),
        C::UnexpectedToken => ("意外的语法单元", "当前位置的符号不符合语法", "按诊断位置调整符号与语法结构。"),
        C::UnexpectedEof => ("源码提前结束", "语法结构尚未完整就到达文件末尾", "补齐缺失的值、分隔符或结束符。"),
        C::ExpectedIdentifier => ("缺少标识符", "声明或路径需要名称的位置不是标识符", "填入合法标识符。"),
        C::ExpectedToken => ("缺少指定符号", "语法要求的符号缺失", "根据诊断消息补齐指定符号。"),
        C::InvalidTopLevelItem => ("顶层声明无效", "文件顶层出现不支持的声明形式", "改为受支持的 CFT 声明。"),
        C::InvalidChainComparison => ("链式比较无效", "比较运算采用了不支持的链式写法", "拆成独立比较并使用布尔运算连接。"),
        C::CheckBlockMustBeLast => ("check 块位置无效", "类型的 check 块后仍有字段声明", "把字段放在 check 块之前。"),
        C::InvalidAnnotationSyntax => ("注解语法无效", "注解名称或参数结构不符合语法", "按注解语法调整名称和参数。"),
        C::InvalidCheckStatement => ("检查语句无效", "check 块中的语句结构不合法", "按检查语句语法改写该语句。"),
        C::DuplicateCheckBlock => ("检查块重复", "同一类型声明了多个 check 块", "把检查语句合并到一个 check 块。"),
        C::SyntaxStructureLimitExceeded => ("语法结构超出限制", "源码的嵌套或结构规模超过语言限制", "减少嵌套深度或拆分源码结构。"),
        C::DuplicateModule => ("模块重复", "Schema 输入产生重复模块身份", "调整 Schema 输入范围以消除重复。"),
        C::DuplicateGlobalName => ("全局名称重复", "多个声明使用相同全局名称", "为重复声明使用不同名称。"),
        C::DuplicateFieldName => ("字段名称重复", "同一类型多次声明相同字段", "合并或重命名重复字段。"),
        C::DuplicateEnumVariant => ("枚举成员重复", "同一枚举多次声明相同成员名称", "删除或重命名重复成员。"),
        C::DuplicateEnumValue => ("枚举数值重复", "枚举成员使用了重复数值", "为成员使用不重复的合法数值。"),
        C::UnknownNamedType => ("类型未声明", "声明中的类型名称无法解析", "声明该类型或改为已有类型。"),
        C::ParentMustBeType => ("继承目标不是类型", "父名称指向不能作为父类型的声明", "改为允许继承的类型声明。"),
        C::UnknownConst => ("常量未声明", "常量名称无法解析", "声明该常量或改为已有常量。"),
        C::InheritanceCycle => ("类型继承成环", "父类型关系包含循环", "移除使继承关系成环的边。"),
        C::InheritSealedType => ("继承 sealed 类型", "类型继承了禁止继承的父类型", "选择可继承的父类型或调整声明。"),
        C::DuplicateInheritedField => ("继承字段重名", "字段与继承得到的字段名称冲突", "保留一个声明或使用不同字段名。"),
        C::ConflictingTypeModifiers => ("类型修饰符冲突", "类型使用了不能同时成立的修饰符", "按类型规则调整修饰符组合。"),
        C::InvalidDictKeyType => ("字典键类型无效", "字典声明使用了不支持的键类型", "使用语言允许的字典键类型。"),
        C::InvalidDefaultExpression => ("默认值表达式无效", "字段默认值不能按默认值语法解释", "按字段类型改写默认值表达式。"),
        C::DefaultTypeMismatch => ("默认值类型不匹配", "默认值无法赋给字段声明类型", "调整默认值或字段类型。"),
        C::DefaultReferencesField => ("默认值引用字段", "默认值依赖了不允许引用的字段", "使用常量或合法默认值表达式。"),
        C::InvalidEnumValueSequence => ("枚举数值序列无效", "枚举的显式或自动数值序列不合法", "调整成员数值和声明顺序。"),
        C::InvalidFlagEnumValue => ("flag 枚举数值无效", "flag 成员的数值不符合位标志规则", "按 flag 枚举规则设置成员数值。"),
        C::UnknownAnnotation => ("注解未定义", "声明使用了不支持的注解名称", "使用受支持的注解名称。"),
        C::DuplicateAnnotation => ("注解重复", "同一目标重复使用不允许重复的注解", "保留一个对应注解。"),
        C::AnnotationWithoutTarget => ("注解没有目标", "注解之后没有可绑定的声明", "把注解放在适用声明之前。"),
        C::InvalidAnnotationTarget => ("注解目标无效", "注解用于不支持的声明类别", "把注解用于允许的目标类别。"),
        C::InvalidAnnotationArgument => ("注解参数无效", "注解参数数量或值不符合要求", "按该注解的参数要求修改。"),
        C::InvalidAnnotatedFieldType => ("注解字段类型无效", "被注解字段的类型不满足注解约束", "调整字段类型或注解。"),
        C::StructRequiresSealedType => ("struct 缺少 sealed", "struct 声明不满足 sealed 要求", "按 struct 规则补齐 sealed 声明。"),
        C::IdAsEnumRequiresEmptyEnum => ("idAsEnum 目标枚举非空", "记录键映射目标枚举已声明成员", "使用符合 idAsEnum 规则的空枚举。"),
        C::EnumVariantOnNonEnum => ("枚举成员目标错误", "成员访问的目标不是枚举", "使用正确的枚举类型名称。"),
        C::UnknownEnumVariant => ("枚举成员不存在", "枚举没有指定名称的成员", "选择已声明的枚举成员。"),
        C::InvalidConstValue => ("常量值无效", "常量表达式不能产生合法常量值", "按常量类型和表达式规则改写。"),
        C::ReservedIdentifier => ("使用了保留标识符", "声明名称属于语言保留名称", "使用非保留的合法标识符。"),
        C::LocalizedOnInvalidTarget => ("localized 目标无效", "本地化注解用于不支持的目标", "按本地化注解规则调整目标。"),
        C::LocalizedBucketNotIdentifier => ("本地化名称无效", "本地化名称不是合法标识符", "使用合法的本地化名称。"),
        C::DimensionOnInvalidTarget => ("dimension 目标无效", "维度注解用于不支持的目标", "按维度注解规则调整目标。"),
        C::DimensionNameNotIdentifier => ("维度名称无效", "维度名称不是合法标识符", "使用合法的维度名称。"),
        C::SingletonOnAbstractType => ("抽象类型不能 singleton", "singleton 与抽象类型同时使用", "调整为符合 singleton 规则的具体类型。"),
        C::SingletonIdAsEnumConflict => ("singleton 与 idAsEnum 冲突", "类型同时使用了不允许组合的身份注解", "按记录身份规则调整注解。"),
        C::SchemaStructureLimitExceeded => ("Schema 结构超出限制", "声明规模或依赖结构超过语言限制", "减少声明规模或依赖嵌套。"),
        C::DuplicateTopLevelCheck => ("项目级检查重名", "多个项目级检查使用相同名称", "为项目级检查使用不同名称。"),
        C::TypeAliasCycle => ("类型别名成环", "别名展开形成循环", "移除循环的别名依赖。"),
        C::RequiredObjectCycle => ("必填对象依赖成环", "必填对象字段形成不能终止的对象结构", "调整对象关系使其可终止。"),
        C::DefaultMaterializationCycle => ("默认值物化成环", "默认对象的展开依赖形成循环", "移除循环默认值依赖。"),
        C::UnknownValueName => ("值名称未定义", "检查表达式中的值名称无法解析", "使用有效的字段、常量或绑定名称。"),
        C::UnknownField => ("字段未定义", "检查表达式访问了不存在的字段", "根据目标类型选择有效字段。"),
        C::TypeUnknownEnumVariant => ("检查中的枚举成员不存在", "检查表达式引用了未声明的枚举成员", "选择该枚举的合法成员。"),
        C::TypeEnumVariantOnNonEnum => ("检查中的枚举目标错误", "检查表达式对非枚举使用成员访问", "使用正确的枚举类型。"),
        C::OperatorTypeMismatch => ("运算符类型不匹配", "操作数类型不支持该运算符", "调整操作数类型或运算符。"),
        C::ComparisonTypeMismatch => ("比较类型不匹配", "比较两侧的类型不能进行该比较", "使用可比较且匹配的值类型。"),
        C::ConditionMustBeBool => ("检查条件不是 bool", "检查条件没有产生布尔值", "改为产生 bool 的条件表达式。"),
        C::UnknownFunction => ("检查函数未定义", "表达式调用了不支持的函数", "使用受支持的检查函数。"),
        C::FunctionArityMismatch => ("函数参数数量错误", "调用参数数量不符合函数签名", "按函数签名补齐或移除参数。"),
        C::FunctionArgTypeMismatch => ("函数参数类型错误", "调用参数类型不符合函数签名", "调整参数值或类型。"),
        C::FieldAccessOnNonObject => ("非对象不能访问字段", "字段路径的当前值不是对象类型", "根据声明类型调整字段路径。"),
        C::IndexOnNonIndexable => ("目标不能使用下标", "下标访问用于不支持的类型", "使用支持下标的集合类型。"),
        C::IndexTypeMismatch => ("下标类型不匹配", "下标值不符合数组索引或字典键类型", "按目标集合的索引类型修改。"),
        C::InvalidIsPredicate => ("类型判断无效", "is 判断使用了无效目标或类型", "按类型判断规则修改目标。"),
        C::QuantifierRequiresCollection => ("量词目标不是集合", "量词遍历目标不是支持的集合", "使用合法集合或记录集合。"),
        C::UniqueUnsupportedElementType => ("unique 元素类型无效", "unique 的元素类型不受支持", "选择受支持的元素类型或投影。"),
        C::BitwiseRequiresIntOrFlagEnum => ("位运算类型无效", "位运算用于非整数、非 flag 枚举", "使用 int 或兼容的 flag 枚举值。"),
        C::ShiftRequiresInt => ("位移要求整数", "位移操作数不是 int", "使用整数操作数。"),
        C::RegexPatternMustBeLiteral => ("正则模式不是字面量", "正则模式未使用要求的字面量形式", "使用合法的字符串模式字面量。"),
        C::InvalidRegexPattern => ("正则模式无效", "模式不能编译为合法正则表达式", "修正正则语法。"),
        C::InvalidQuantifierBindings => ("量词绑定无效", "量词绑定的数量或形式不符合遍历目标", "按集合或记录集合规则调整绑定。"),
        C::InvalidRecordSetQuery => ("记录集合查询无效", "记录集合查询的目标或形式不合法", "使用合法的记录类型和查询形式。"),
    }
}

fn cfd_info(code: CfdErrorCode) -> (&'static str, &'static str, &'static str) {
    use CfdErrorCode as C;
    match code {
        C::UnknownType => ("记录类型未定义", "记录使用了无法解析的类型", "声明该类型或改为已有记录类型。"),
        C::AbstractRecordType => ("记录类型为抽象类型", "记录直接使用了抽象类型", "选择可赋给该类型的具体记录类型。"),
        C::MissingObjectType => ("对象类型缺失", "对象没有可确定的类型身份", "提供可确定的合法对象类型。"),
        C::ObjectTypeMismatch => ("对象类型不匹配", "对象实际类型不能赋给声明类型", "选择可赋给字段类型的具体对象类型。"),
        C::UnknownField => ("数据字段未声明", "记录或对象包含未声明字段", "删除该赋值或补齐相应字段声明。"),
        C::MissingRequiredField => ("必填字段缺失", "没有显式赋值且没有可用默认值", "提供该字段的合法值。"),
        C::TypeMismatch => ("数据值类型不匹配", "数据值不能赋给字段声明类型", "按字段声明类型提供值。"),
        C::InvalidEnumVariant => ("枚举值无效", "数据枚举值不符合目标枚举", "使用合法枚举成员或允许的 flag 组合。"),
        C::DuplicateDictKey => ("字典键重复", "同一字典包含语义相同的键", "合并或移除重复键条目。"),
        C::MissingIdField => ("记录身份信息缺失", "记录缺少要求的身份信息", "按记录身份规则补齐身份。"),
        C::InvalidRecordKey => ("记录键无效", "记录键不符合身份规则", "使用合法且符合目标类型规则的记录键。"),
        C::DuplicateId => ("记录键重复", "同一身份范围内存在重复记录键", "为记录使用不重复的键。"),
        C::DuplicatePolymorphicId => ("多态记录键重复", "多态记录身份范围内出现键冲突", "调整冲突记录的键。"),
        C::SingletonRecordCountInvalid => ("singleton 记录数量无效", "singleton 类型的记录数量不符合要求", "按 singleton 身份规则调整记录数量。"),
        C::SingletonKeyMissingOrInvalid => ("singleton 记录键无效", "singleton 记录没有使用规定身份", "使用该 singleton 的规定记录键。"),
        C::SingletonKeyCollision => ("singleton 记录键冲突", "singleton 身份与其他记录冲突", "消除冲突的记录身份。"),
        C::ValueDependencyCycle => ("值依赖成环", "记录值或插值依赖形成循环", "调整依赖路径以消除循环。"),
        C::DataStructureLimitExceeded => ("数据结构超出限制", "数据规模或值嵌套超过结构限制", "减少结构规模或嵌套深度。"),
        C::RefTargetNotFound => ("引用目标不存在", "记录引用不能找到目标记录", "创建目标或修正引用键。"),
        C::RefTargetTypeMismatch => ("引用目标类型不匹配", "引用目标不能赋给声明的引用类型", "选择符合引用类型的目标记录。"),
        C::CheckFailed => ("业务规则未通过", "业务检查条件不成立", "检查规则和诊断定位的业务值。"),
        C::CheckEvalTypeError => ("规则求值类型错误", "求值时遇到不符合操作要求的类型", "检查字段值、表达式和规则依赖。"),
        C::CheckIndexOutOfBounds => ("规则数组下标越界", "求值访问了不存在的数组位置", "调整下标或在规则中检查集合范围。"),
        C::CheckMissingDictKey => ("规则字典键不存在", "求值读取了字典中不存在的键", "补齐所需键或先检查键是否存在。"),
        C::CheckEmptyMinMax => ("空集合不能求极值", "min 或 max 的目标集合为空", "为集合提供值或先检查集合非空。"),
        C::CheckComparisonFailed => ("规则比较未通过", "规则中的比较结果为假", "检查比较两侧的业务值与预期关系。"),
        C::CheckBoolExpectedTrue => ("规则布尔条件未通过", "要求为 true 的条件结果为 false", "检查条件依赖值与业务要求。"),
        C::CheckNegationFailed => ("规则否定条件未通过", "否定条件结果未满足规则", "检查被否定条件的业务值。"),
        C::CheckAndFailed => ("规则合取条件未通过", "合取中的条件未全部成立", "检查失败的子条件与关联业务值。"),
        C::CheckOrFailed => ("规则析取条件未通过", "析取中的条件均未满足要求", "检查各子条件与关联业务值。"),
        C::CheckTypePredicateFailed => ("规则类型判断未通过", "值的实际类型不满足类型判断", "检查对象实际类型或类型条件。"),
        C::CheckContainsFailed => ("规则包含条件未通过", "目标集合或字符串不包含要求值", "检查集合内容、目标值和包含条件。"),
        C::CheckUniqueFailed => ("规则唯一性条件未通过", "要求唯一的值出现重复", "修改重复业务值或唯一性投影。"),
        C::CheckMatchesFailed => ("规则模式匹配未通过", "字符串不满足要求的正则模式", "调整字符串内容或业务模式。"),
        C::CheckAnyQuantifierFailed => ("any 规则未通过", "遍历结果没有满足条件的元素", "检查集合内容和量词条件。"),
        C::CheckNoneQuantifierFailed => ("none 规则未通过", "遍历中存在满足禁止条件的元素", "检查命中元素和禁止条件。"),
        C::CheckAllQuantifierFailed => ("all 规则未通过", "遍历中存在不满足条件的元素", "检查失败元素和量词条件。"),
        C::CheckBudgetExceeded => ("规则执行预算耗尽", "规则执行工作量达到预算上限", "减少规则工作量或调整执行计划；重新检查完成状态。"),
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
    RuntimeCode { code: "PROJECT-CONFIG-NOT-FOUND", stage: "PROJECT", meaning: "项目配置未找到", trigger: "指定目录及父目录没有可用项目配置", repair: "用 --project 指定项目目录或配置文件。" },
    RuntimeCode { code: "PROJECT-CONFIG-AMBIGUOUS", stage: "PROJECT", meaning: "项目配置有歧义", trigger: "项目目录同时存在多个受支持的配置文件名", repair: "明确保留要使用的配置文件或指定配置文件路径。" },
    RuntimeCode { code: "PROJECT-CONFIG-PATH", stage: "PROJECT", meaning: "项目配置路径无效", trigger: "配置路径不存在、类型不正确或无法解析", repair: "检查 --project 路径及文件访问条件。" },
    RuntimeCode { code: "PROJECT-CONFIG-READ", stage: "PROJECT", meaning: "项目配置读取失败", trigger: "配置文件不能读取", repair: "检查路径、访问权限和文件状态。" },
    RuntimeCode { code: "PROJECT-CONFIG-PARSE", stage: "PROJECT", meaning: "项目配置解析失败", trigger: "coflow.yaml 不符合配置语法或字段类型", repair: "按项目配置参考修正诊断位置的内容。" },
    RuntimeCode { code: "PROJECT-CONFIG-WRITE", stage: "PROJECT", meaning: "项目配置写入失败", trigger: "更新配置文件时写入失败", repair: "检查目标路径和写入条件。" },
    RuntimeCode { code: "PROJECT-CONFIG-OVERLAP", stage: "PROJECT", meaning: "项目输入范围重叠", trigger: "Schema 与数据输入包含或覆盖同一来源", repair: "调整输入路径以分离 Schema 与数据范围。" },
    RuntimeCode { code: "PROJECT-SCHEMA-PATH", stage: "PROJECT", meaning: "Schema 输入路径无效", trigger: "Schema 输入不存在或路径形式不合法", repair: "检查 Schema 输入路径和文件类别。" },
    RuntimeCode { code: "PROJECT-SCHEMA-READ", stage: "PROJECT", meaning: "Schema 输入读取失败", trigger: "Schema 文件或目录无法读取", repair: "检查输入路径及访问条件。" },
    RuntimeCode { code: "PROJECT-CWD", stage: "PROJECT", meaning: "当前目录无法读取", trigger: "项目定位无法获得调用目录", repair: "从可访问的目录调用或明确指定项目路径。" },
    RuntimeCode { code: "PROJECT-INIT-IO", stage: "PROJECT", meaning: "项目初始化失败", trigger: "创建项目文件时发生读写错误", repair: "检查目标目录及文件创建条件。" },
    RuntimeCode { code: "PROJECT-FILE-CREATE", stage: "PROJECT", meaning: "项目文件创建失败", trigger: "项目文件不能创建", repair: "检查目标是否已存在及目录写入条件。" },
    RuntimeCode { code: "PROJECT-FILE-DELETE", stage: "PROJECT", meaning: "项目文件删除失败", trigger: "项目文件不能删除", repair: "检查目标文件及删除条件。" },
    RuntimeCode { code: "PROJECT-FILE-RESTORE", stage: "PROJECT", meaning: "项目文件恢复失败", trigger: "文件操作失败后的恢复未完成", repair: "根据诊断路径核实实际内容并完成恢复。" },
    RuntimeCode { code: "CFD-READ", stage: "CFD", meaning: "CFD 写入来源读取失败", trigger: "编辑所需的 CFD 文件或来源结构不能读取", repair: "检查来源文件、记录位置和诊断消息。" },
    RuntimeCode { code: "CFD-WRITE", stage: "CFD", meaning: "CFD 源码修改失败", trigger: "请求不能安全地应用到 CFD 来源结构", repair: "检查字段路径、记录来源和请求操作。" },
    RuntimeCode { code: "CFD-SOURCE", stage: "CFD", meaning: "CFD 输入来源无效", trigger: "输入来源不能作为有效的 CFD 数据来源", repair: "检查数据输入配置与文件类别。" },
    RuntimeCode { code: "CLI-REQUEST", stage: "CLI", meaning: "命令请求无效", trigger: "参数或结构化请求不符合命令协议", repair: "使用 describe 查询命令要求并修正请求。" },
    RuntimeCode { code: "CLI-TARGET", stage: "CLI", meaning: "查询或修改目标不存在", trigger: "类型、记录、字段或其他请求目标不能找到", repair: "重新查询目标身份并使用已有目标。" },
    RuntimeCode { code: "CLI-CONFLICT", stage: "CLI", meaning: "输入或前置条件冲突", trigger: "输入已变化或请求的预期条件未满足", repair: "重新读取当前内容并重新准备请求。" },
    RuntimeCode { code: "CLI-IO", stage: "CLI", meaning: "请求载体读写失败", trigger: "请求文件、标准输入或其他文件操作失败", repair: "检查诊断中的路径与读写条件。" },
    RuntimeCode { code: "CLI-PUBLISH", stage: "CLI", meaning: "候选发布失败", trigger: "候选文件发布或失败恢复没有完成", repair: "根据 written_files、restored_files 和 restoration_failed 核实文件状态后重新准备请求。" },
    RuntimeCode { code: "CLI-CANDIDATE", stage: "CLI", meaning: "候选项目处理失败", trigger: "候选项目构建、路径映射或来源识别失败", repair: "检查项目输入路径、候选配置和诊断消息。" },
    RuntimeCode { code: "CLI-FIELD", stage: "CLI", meaning: "字段投影无效", trigger: "所选类型范围内字段路径无法解析", repair: "查询 Schema 并选择范围内有效字段。" },
    RuntimeCode { code: "CLI-PATH", stage: "CLI", meaning: "值路径无效", trigger: "请求的字段或集合路径不符合目标结构", repair: "按字段类型修正字段名、索引或字典键。" },
    RuntimeCode { code: "CLI-SORT", stage: "CLI", meaning: "排序字段无效", trigger: "字段无法作为所选范围的排序键", repair: "选择有效且类型一致的可排序字段。" },
    RuntimeCode { code: "CLI-FILTER", stage: "CLI", meaning: "筛选条件无效", trigger: "筛选表达式的字段或值类型不合法", repair: "按 --where 语法和字段声明类型修正条件。" },
    RuntimeCode { code: "CLI-VALUE", stage: "CLI", meaning: "写入值无效", trigger: "CFD 表达式或模型值不符合目标类型", repair: "按目标字段类型提供有效写入值。" },
];
