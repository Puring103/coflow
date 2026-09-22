//! 文本、数值、集合与维度内建语义，固定接收者使用借用视图。
use super::{ExecutionContext, CachedRegex, Heap, Stored, Value, Slot, CftValueType, ExecutionError, ExecutionHost, invalid, Comparison};
/// 浮点文本只接受语言定义的 ASCII 语法，不建立全局正则或初始化锁。
fn is_float_text(text: &str) -> bool {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if matches!(unsigned, "inf" | "NaN") { return true; }
    let bytes = unsigned.as_bytes();
    let mut pos = 0;
    fn digits(bytes: &[u8], pos: &mut usize) -> bool {
        let start = *pos;
        while bytes.get(*pos).is_some_and(u8::is_ascii_digit) { *pos += 1; }
        *pos > start
    }
    if !digits(bytes, &mut pos) { return false; }
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        if !digits(bytes, &mut pos) { return false; }
    }
    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        pos += 1;
        if matches!(bytes.get(pos), Some(b'+' | b'-')) { pos += 1; }
        if !digits(bytes, &mut pos) { return false; }
    }
    pos == bytes.len()
}

impl ExecutionContext<'_> {
    pub(super) fn regex_match(&self, pattern: &str, text: &str) -> Result<bool, ExecutionError> {
        use regex_automata::nfa::thompson::{pikevm::PikeVM, WhichCaptures, NFA};
        let mut regexes = self.runtime.vm.regexes.borrow_mut();
        if !regexes.contains_key(pattern) {
            let heap_bytes = self.runtime.vm.heap.borrow().total_bytes();
            let remaining = self.budget.remaining_heap_bytes().saturating_sub(heap_bytes);
            let syntax_bytes = pattern
                .len()
                .checked_mul(256)
                .and_then(|n| n.checked_add(4096))
                .ok_or_else(|| invalid("正则容量溢出"))?;
            let nfa_limit = remaining.saturating_sub(syntax_bytes) / 64;
            if nfa_limit < 1024 {
                return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
            }
            // 无捕获 Pike VM 没有随输入增长的 DFA 缓存；编译和 NFA 状态空间先保守预留。
            let compile_memory = self
                .budget
                .reserve_temporary(syntax_bytes + nfa_limit * 64, heap_bytes)?;
            self.budget.charge(pattern.len() as u64)?;
            let program = PikeVM::builder()
                .thompson(
                    NFA::config()
                        .which_captures(WhichCaptures::None)
                        .nfa_size_limit(Some(nfa_limit)),
                )
                .build(pattern)
                .map_err(|error| invalid(&format!("正则编译失败：{error}")))?;
            let required = program
                .get_nfa()
                .memory_usage()
                .checked_mul(16)
                .and_then(|n| n.checked_add(program.get_nfa().states().len().saturating_mul(64)))
                .and_then(|n| n.checked_add(pattern.len() + size_of::<CachedRegex>() + 1024))
                .and_then(|n| {
                    n.checked_add(Heap::table_growth::<(String, CachedRegex)>(
                        regexes.len(),
                        regexes.capacity(),
                    ))
                })
                .ok_or_else(|| invalid("正则容量溢出"))?;
            drop(compile_memory);
            let memory = self.budget.reserve_temporary(required, heap_bytes)?;
            let cache = program.create_cache();
            let mut key = String::new();
            key.try_reserve_exact(pattern.len())
                .map_err(|_| invalid("正则键分配失败"))?;
            key.push_str(pattern);
            regexes
                .try_reserve(1)
                .map_err(|_| invalid("正则索引分配失败"))?;
            regexes.insert(
                key,
                CachedRegex {
                    program,
                    cache,
                    _memory: memory,
                },
            );
        }
        let entry = regexes
            .get_mut(pattern)
            .ok_or_else(|| invalid("正则缓存缺失"))?;
        let work = (entry.program.get_nfa().states().len() as u64)
            .saturating_mul(text.len().saturating_add(1) as u64);
        self.budget.charge(work)?;
        Ok(entry.program.is_match(&mut entry.cache, text))
    }
    pub(super) fn builtin_value(
        &self,
        name: &str,
        receiver: Slot,
        args: &[Slot],
        result_type: &CftValueType,
    ) -> Result<Slot, ExecutionError> {
        if let Some(type_name) = name.strip_prefix("$enum::") {
            let Slot::Int(value) = receiver else {
                return Err(invalid("enum 构造需要 int"));
            };
            let value = u32::try_from(value).map_err(|_| invalid("enum 值不能为负"))?;
            let meta = self
                .runtime
                .contract
                .schema()
                .resolve_enum(type_name)
                .ok_or_else(|| invalid("未知 enum"))?;
            if meta.is_flag {
                let mask = meta
                    .variants
                    .iter()
                    .fold(0u32, |mask, variant| mask | variant.value as u32);
                if value & !mask != 0 {
                    return Err(invalid("flag 包含未知位"));
                }
            }
            return self.allocate(Value::Enum {
                type_name: self.copy_text(type_name)?,
                value,
            });
        }
        if matches!(name, "for" | "default" | "variants") {
            let Slot::Handle(id) = receiver else {
                return Err(invalid("维度方法需要记录"));
            };
        let id = id.get();
            if name == "default" {
                return self.read_slot(self.slot(self.runtime.dimension_default(id)?)?);
            }
            if name == "for" {
                let Some(argument) = args.first() else {
                    return Err(invalid("缺少变体名"));
                };
                let text = self.text(*argument)?;
                return self.read_slot(self.slot(self.runtime.dimension_variant(id, &text)?)?);
            }
            let value = self.value(Slot::handle(id))?;
            let Value::Dimension { variants, .. } = value.as_ref() else {
                return Err(invalid("维度方法需要维度值"));
            };
            let (mut values, _memory) = self.reserve_values(variants.len())?;
            for variant in variants.keys() {
                self.budget.charge(1)?;
                let key = self.allocate(Value::String(self.copy_text(variant)?))?;
                let value =
                    self.read_slot(self.slot(self.runtime.dimension_variant(id, variant)?)?)?;
                values.push((key, value));
            }
            return self.dictionary(values);
        }
        if let Some(type_name) = name.strip_prefix("$records::") {
            // 记录目录属于映像，直接借用遍历，避免先复制未计费的目录。
            let records = &self.runtime.records;
            self.budget.charge(records.len() as u64)?;
            let count = records
                .keys()
                .filter(|(actual, _)| {
                    self.runtime
                        .contract
                        .schema()
                        .is_assignable(actual, type_name)
                })
                .count();
            let (mut values, _memory) = self.reserve_values(count)?;
            for ((actual, _), id) in records.iter() {
                if self
                    .runtime
                    .contract
                    .schema()
                    .is_assignable(actual, type_name)
                {
                    values.push(Slot::handle(*id));
                }
            }
            return self.array(values);
        }
        let argument = |index: usize| {
            args.get(index)
                .copied()
                .ok_or_else(|| invalid("缺少内建参数"))
        };
        if name == "len" {
            return Ok(Slot::Int(
                i32::try_from(self.length(receiver)?).map_err(|_| invalid("长度超出 int"))?,
            ));
        }
        if name == "string" {
            return self.allocate(Value::String(self.text(receiver)?));
        }
        if name == "isSome" || name == "isNone" {
            return Ok(Slot::Bool(if name == "isSome" {
                receiver != Slot::None
            } else {
                receiver == Slot::None
            }));
        }
        let stored = self.read_value(receiver)?;
        match stored.view() {
            Stored::Scalar(Slot::Int(ref value)) => match name {
                "abs" => Ok(Slot::Int(
                    value.checked_abs().ok_or_else(|| invalid("绝对值溢出"))?,
                )),
                "float" => Ok(Slot::Float(*value as f32)),
                _ => Err(invalid("未知 int 内建")),
            },
            Stored::Scalar(Slot::Float(ref value)) => match name {
                "abs" => Ok(Slot::Float(value.abs())),
                "isFinite" => Ok(Slot::Bool(value.is_finite())),
                "int" => {
                    if value.is_finite() && *value >= -2147483648.0 && *value < 2147483648.0 {
                        Ok(Slot::Int(value.trunc() as i32))
                    } else {
                        Err(invalid("float 转 int 越界"))
                    }
                }
                "approxEqual" => {
                    let (Slot::Float(other), Slot::Float(epsilon)) = (argument(0)?, argument(1)?)
                    else {
                        return Err(invalid("需要 float 参数"));
                    };
                    if !epsilon.is_finite() || epsilon < 0.0 {
                        return Err(invalid("epsilon 必须有限且非负"));
                    }
                    Ok(Slot::Bool((*value - other).abs() <= epsilon))
                }
                _ => Err(invalid("未知 float 内建")),
            },
            Stored::String(value) => {
                self.budget.charge(value.len() as u64)?;
                if name == "isBlank" {
                    return Ok(Slot::Bool(value.chars().all(char::is_whitespace)));
                }
                if name == "parseInt" {
                    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
                    return Ok(
                        if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
                            value.parse::<i32>().map_or(Slot::None, Slot::Int)
                        } else {
                            Slot::None
                        },
                    );
                }
                if name == "parseFloat" {
                    return Ok(if is_float_text(value) {
                        value.parse::<f32>().map_or(Slot::None, Slot::Float)
                    } else {
                        Slot::None
                    });
                }
                let other = self.read_value(argument(0)?)?;
                let Stored::String(other) = other.view() else {
                    return Err(invalid("需要 string 参数"));
                };
                Ok(Slot::Bool(match name {
                    "contains" => value.contains(other),
                    "startsWith" => value.starts_with(other),
                    "endsWith" => value.ends_with(other),
                    "matches" => self.regex_match(other, value)?,
                    _ => return Err(invalid("未知 string 内建")),
                }))
            }
            Stored::Array(values) => {
                self.budget.charge(values.len() as u64)?;
                if name == "contains" {
                    for value in values {
                        if self.equal(self.slot(value)?, argument(0)?)? {
                            return Ok(Slot::Bool(true));
                        }
                    }
                    return Ok(Slot::Bool(false));
                }
                if name == "isUnique" {
                    let (mut seen, _set_memory) = self.temporary_key_set(values.len())?;
                    let (mut key_memory, _guards_memory) = self.reserve_values(values.len())?;
                    for value in values {
                        let (key, memory) = self.temporary_key(self.slot(value)?)?;
                        if !seen.insert(key) {
                            return Ok(Slot::Bool(false));
                        }
                        key_memory.push(memory);
                    }
                    return Ok(Slot::Bool(true));
                }
                if name == "isSorted" || name == "isStrictlySorted" {
                    for value in values {
                        if matches!(self.slot(value)?,Slot::Float(v)if v.is_nan()) {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    for (left, right) in values.iter().zip(values.iter().skip(1)) {
                        let order = self.compare(self.slot(left)?, self.slot(right)?)?;
                        if !(order == Some(Comparison::Less)
                            || (name == "isSorted" && order == Some(Comparison::Equal)))
                        {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    return Ok(Slot::Bool(true));
                }
                if matches!(name, "min" | "max" | "sum") {
                    if values.is_empty() {
                        return if name == "sum" {
                            Ok(if *result_type == CftValueType::Float {
                                Slot::Float(0.0)
                            } else {
                                Slot::Int(0)
                            })
                        } else {
                            Err(invalid("空数组没有极值"))
                        };
                    }
                    let mut result = self.slot(values.get(0).expect("非空数组"))?;
                    for id in values.iter().skip(1) {
                        let next = self.slot(id)?;
                        result = if name == "sum" {
                            match (result, next) {
                                (Slot::Int(a), Slot::Int(b)) => {
                                    Slot::Int(a.checked_add(b).ok_or_else(|| invalid("求和溢出"))?)
                                }
                                (Slot::Float(a), Slot::Float(b)) => Slot::Float(a + b),
                                _ => return Err(invalid("求和需要数值")),
                            }
                        } else if matches!(result,Slot::Float(v)if v.is_nan()) {
                            result
                        } else if matches!(next,Slot::Float(v)if v.is_nan()) {
                            next
                        } else {
                            let order = self.compare(result, next)?;
                            if (name == "min" && order == Some(Comparison::Greater))
                                || (name == "max" && order == Some(Comparison::Less))
                            {
                                next
                            } else {
                                result
                            }
                        };
                    }
                    return Ok(result);
                }
                if matches!(
                    name,
                    "intersects" | "isDisjoint" | "isSubsetOf" | "isSupersetOf"
                ) {
                    let other = self.read_value(argument(0)?)?;
                    let Stored::Array(other) = other.view() else {
                        return Err(invalid("需要数组"));
                    };
                    let (left, right) = if name == "isSupersetOf" {
                        (other, values)
                    } else {
                        (values, other)
                    };
                    let (mut right_keys, _set_memory) = self.temporary_key_set(right.len())?;
                    let (mut key_memory, _guards_memory) = self.reserve_values(right.len())?;
                    for value in right {
                        let (key, memory) = self.temporary_key(self.slot(value)?)?;
                        if right_keys.insert(key) {
                            key_memory.push(memory);
                        }
                    }
                    let right = right_keys;
                    for value in left {
                        let found = right.contains(&self.scalar_key(self.slot(value)?)?);
                        if matches!(name, "intersects" | "isDisjoint") && found {
                            return Ok(Slot::Bool(name == "intersects"));
                        }
                        if matches!(name, "isSubsetOf" | "isSupersetOf") && !found {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    return Ok(Slot::Bool(name != "intersects"));
                }
                Err(invalid("未知数组内建"))
            }
            Stored::Dict(values) => {
                self.budget.charge(values.len() as u64)?;
                if name == "keys" || name == "values" {
                    let (mut items, _memory) = self.reserve_values(values.len())?;
                    for (key, value) in values.values() {
                        items.push(self.slot(if name == "keys" { *key } else { *value })?);
                    }
                    return self.array(items);
                }
                if matches!(name, "contains" | "containsKey" | "containsValue") {
                    let argument = argument(0)?;
                    if name != "containsValue" {
                        return Ok(Slot::Bool(values.contains_key(&self.scalar_key(argument)?)));
                    }
                    for (_, (_, value)) in values {
                        if self.equal(self.slot(*value)?, argument)? {
                            return Ok(Slot::Bool(true));
                        }
                    }
                    return Ok(Slot::Bool(false));
                }
                Err(invalid("未知字典内建"))
            }
            _ => Err(invalid("该值不提供内建方法")),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn float_text_grammar_preserves_language_boundaries() {
        for text in ["0", "-0", "+12.5", "1e2", "-1.2E-3", "inf", "-inf", "+NaN"] {
            assert!(super::is_float_text(text), "{text}");
        }
        for text in ["", "+", ".5", "1.", "1.e2", "1e", "1e+", " 1", "1\n", "１２", "Infinity", "nan", "1_0"] {
            assert!(!super::is_float_text(text), "{text}");
        }
    }
}
