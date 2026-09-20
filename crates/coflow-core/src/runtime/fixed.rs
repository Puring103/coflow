//! 固定区整体持有类型化负载；通用值适配与投影按需物化，热路径直接借用类型化区域。
use super::{ArrayValue, ScalarKey, Value, ValueId};
use crate::{schema::CftValueType, vm::executor::Slot};
use std::{borrow::Cow, collections::{BTreeMap, BTreeSet}, ops::Range, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TypeId(u32);
#[derive(Debug, Clone, Copy)]
pub(super) struct FieldNameId(u32);
#[derive(Debug, Clone, Copy)]
pub(super) struct LayoutId(u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct StringId(u32);
#[derive(Debug)]
struct Layout { ty: TypeId, fields: Vec<FieldSpec> }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarKind { Bool, Int, Float }
#[derive(Debug, Clone, Copy, Default)]
struct FieldEncoding { scalar: Option<ScalarKind>, indirect: bool, optional: bool }
impl FieldEncoding {
    fn observe(&mut self, id: ValueId) -> Result<(), String> {
        let kind = match Slot::from_scalar_id(id) {
            Some(Slot::None) => { self.optional = true; return Ok(()); }
            Some(Slot::Bool(_)) => ScalarKind::Bool,
            Some(Slot::Int(_)) => ScalarKind::Int,
            Some(Slot::Float(_)) => ScalarKind::Float,
            None => { u32::try_from(id).map_err(|_| "固定字段引用超限")?; self.indirect = true; return Ok(()); }
            _ => return Err("固定字段标量无效".into()),
        };
        if self.scalar.is_some_and(|previous| previous != kind) { return Err("同一固定字段存在不一致的标量类型".into()); }
        self.scalar = Some(kind); Ok(())
    }
    fn payload_bytes(self) -> usize {
        if self.scalar.is_none() && !self.indirect { 0 }
        else if self.scalar == Some(ScalarKind::Bool) && !self.indirect { 1 } else { 4 }
    }
    fn tagged(self) -> bool { self.payload_bytes() != 0 && (self.optional || (self.indirect && self.scalar.is_some())) }
    fn width(self) -> usize { self.payload_bytes() + usize::from(self.tagged()) }
    fn write(self, id: ValueId, bytes: &mut Vec<u8>) -> Result<(), String> {
        let (tag, payload) = match Slot::from_scalar_id(id) {
            Some(Slot::None) if self.optional => (0, 0),
            Some(Slot::Bool(value)) if self.scalar == Some(ScalarKind::Bool) => (1, u32::from(value)),
            Some(Slot::Int(value)) if self.scalar == Some(ScalarKind::Int) => (1, value as u32),
            Some(Slot::Float(value)) if self.scalar == Some(ScalarKind::Float) => (1, value.to_bits()),
            None if self.indirect => (2, u32::try_from(id).map_err(|_| "固定字段引用超限")?),
            _ => return Err("固定字段载荷与布局不一致".into()),
        };
        if self.tagged() { bytes.push(tag); }
        bytes.extend_from_slice(&payload.to_le_bytes()[..self.payload_bytes()]); Ok(())
    }
    fn read(self, bytes: &[u8]) -> Option<ValueId> {
        if self.payload_bytes() == 0 { return Slot::None.scalar_id(); }
        let (tag, bytes) = if self.tagged() { (*bytes.first()?, bytes.get(1..)?) } else { (if self.scalar.is_some() { 1 } else { 2 }, bytes) };
        if tag == 0 { return Slot::None.scalar_id(); }
        let mut payload = [0u8; 4];
        payload[..self.payload_bytes()].copy_from_slice(bytes.get(..self.payload_bytes())?);
        let payload = u32::from_le_bytes(payload);
        if tag == 2 { return Some(u64::from(payload)); }
        match self.scalar? { ScalarKind::Bool => Slot::Bool(payload != 0), ScalarKind::Int => Slot::Int(payload as i32), ScalarKind::Float => Slot::Float(f32::from_bits(payload)) }.scalar_id()
    }
}
/// 字段解释方式共享于布局；每行只保存紧凑载荷，Host/None 才需要额外标记。
#[derive(Debug)]
pub(super) struct FieldSpec { name: FieldNameId, offset: u32, encoding: FieldEncoding }
#[derive(Debug)]
struct Object { layout: LayoutId, key: Option<StringId>, fields: Range<usize>, bases: Vec<(TypeId, ValueId)> }
#[derive(Debug)]
struct Dimension { default: ValueId, variants: Vec<(StringId, ValueId, bool)> }
#[derive(Debug)]
struct Function { source: Arc<str>, owner: Option<ValueId>, host: Option<(StringId, StringId)> }
#[derive(Debug)]
struct Host { service: StringId, field: StringId, ty: CftValueType }
type Dictionary = indexmap::IndexMap<ScalarKey, (ValueId, ValueId)>;
// 与 ScalarKey 保持相同变体顺序和 Hash 语义，固定字符串查询不复制 UTF-8。
#[derive(Hash)]
enum Key<'a> { Bool(bool), Int(i32), String(&'a str), Enum { type_name: &'a str, value: u32 } }
impl indexmap::Equivalent<ScalarKey> for Key<'_> {
    fn equivalent(&self, other: &ScalarKey) -> bool {
        match (self, other) {
            (Self::Bool(a), ScalarKey::Bool(b)) => a == b,
            (Self::Int(a), ScalarKey::Int(b)) => a == b,
            (Self::String(a), ScalarKey::String(b)) => *a == b,
            (Self::Enum { type_name: a, value: av }, ScalarKey::Enum { type_name: b, value: bv }) => *a == b && av == bv,
            _ => false,
        }
    }
}

/// 固定与动态值共用只读借用视图，热路径不重建 Value、字段名或集合。
pub(super) enum Fields<'a> { Packed { fields: &'a [FieldSpec], bytes: &'a [u8] }, Named(&'a [(String, ValueId)]) }
impl Fields<'_> {
    pub(super) fn get(&self, index: usize) -> Option<ValueId> {
        match self { Self::Packed { fields, bytes } => { let field = fields.get(index)?; field.encoding.read(bytes.get(field.offset as usize..)?) }, Self::Named(values) => values.get(index).map(|(_, value)| *value) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_pools_share_layouts_and_strings_while_preserving_cycles_and_float_bits() {
        let object = |key: &str, friend| Value::Object { type_name: "Node".into(), key: Some(key.into()),
            fields: vec![("name".into(), 2), ("friend".into(), friend)], bases: Vec::new() };
        let (values, ids) = compact(vec![object("a", 1), object("b", 0), Value::String("same".into()),
            Value::String("same".into()), Value::Float(-0.0), Value::Float(f32::from_bits(0x7fc00001)),
            Value::Int(i32::MIN), Value::None, Value::Array(vec![2, 3, 7].into())]).unwrap();
        let values = FixedValues::new(values).unwrap();
        assert_eq!(values.layouts.len(), 1);
        assert_eq!(values.field_names.len(), 2);
        assert_eq!(values.cells[2], values.cells[3]);
        assert_eq!(values.field(0, 1), Some(1)); assert_eq!(values.field(1, 1), Some(0));
        assert_eq!(values.named_field(1, "name"), Some(2));
        assert!(matches!(values.scalar(ids[4]), Some(Slot::Float(value)) if value.to_bits() == (-0.0f32).to_bits()));
        assert!(matches!(values.scalar(ids[5]), Some(Slot::Float(value)) if value.to_bits() == 0x7fc00001));
        assert_eq!(values.scalar(ids[6]), Some(Slot::Int(i32::MIN)));
        assert!(matches!(values.view(ids[8]), Some(View::Array(values)) if values.iter().collect::<Vec<_>>() == vec![ids[2], ids[3], ids[7]]));
        assert!(matches!(values.get(0).as_deref(), Some(Value::Object { fields, .. }) if fields[1].1 == 1));
    }
    #[test]
    fn compact_publication_relocates_cycles_and_all_scalar_edges() {
        let values = vec![
            Value::Int(i32::MIN), Value::Float(-0.0), Value::None,
            Value::Object { type_name: "Node".into(), key: Some("one".into()),
                fields: vec![("self".into(), 3), ("number".into(), 0)], bases: vec![] },
            Value::Array(vec![0, 0].into()),
            Value::Dimension { default: 1, variants: BTreeMap::from([("none".into(), 2)]), explicit: BTreeSet::from(["none".into()]) },
            Value::Function { source: "fn() => 1".into(), owner: Some(3), host: None },
            Value::Template { source: "text".into(), owner: Some(3) },
            Value::Dict(indexmap::IndexMap::from([(ScalarKey::Int(i32::MIN), (0, 2))])),
        ];
        let (values, remap) = compact(values).unwrap();
        assert_eq!(values.len(), 6);
        let fixed = FixedValues::new(values).unwrap();
        assert_eq!(fixed.field(remap[3], 0), Some(remap[3]));
        assert_eq!(fixed.field(remap[3], 1), Some(remap[0]));
        assert!(matches!(fixed.view(remap[4]), Some(View::Array(array)) if array.references().is_empty() && array.heap_bytes() == 8));
        assert!(matches!(fixed.get(remap[5]).as_deref(), Some(Value::Dimension { default, variants, explicit })
            if *default == remap[1] && variants["none"] == remap[2] && explicit.contains("none")));
        assert!(matches!(fixed.get(remap[6]).as_deref(), Some(Value::Function { owner: Some(owner), .. }) if *owner == remap[3]));
        assert!(matches!(fixed.get(remap[7]).as_deref(), Some(Value::Template { owner: Some(owner), .. }) if *owner == remap[3]));
        assert_eq!(fixed.dictionary_index(remap[8], Slot::Int(i32::MIN)), Some(Some(remap[2])));
        assert!(matches!(fixed.scalar(remap[1]), Some(Slot::Float(value)) if value.to_bits() == 0x8000_0000));
        assert!(compact(vec![Value::Array(vec![u64::MAX].into())]).is_err());
    }

    #[test]
    fn object_rows_share_typed_layout_across_none_and_host_overrides() {
        let id = |slot: Slot| slot.scalar_id().unwrap();
        let row = |maybe, host| Value::Object { type_name: "Row".into(), key: None,
            fields: vec![("number".into(), id(Slot::Int(i32::MIN))), ("flag".into(), id(Slot::Bool(false))), ("maybe".into(), maybe), ("live".into(), host)], bases: vec![] };
        let values = FixedValues::new(vec![
            row(id(Slot::None), id(Slot::Int(7))),
            row(id(Slot::Float(f32::from_bits(0x7fc0_0042))), 3),
            row(id(Slot::Float(-0.0)), id(Slot::None)),
            Value::HostData { service: "Clock".into(), field: "value".into(), value_type: CftValueType::Int },
        ]).unwrap();
        assert_eq!(values.layouts.len(), 1);
        assert_eq!(values.field_bytes.len(), 3 * (4 + 1 + 5 + 5));
        assert_eq!(values.field(0, 2), Some(id(Slot::None)));
        assert_eq!(values.field(1, 2), Some(id(Slot::Float(f32::from_bits(0x7fc0_0042)))));
        assert_eq!(values.field(2, 2), Some(id(Slot::Float(-0.0))));
        assert_eq!(values.field(1, 3), Some(3)); assert!(values.is_host(3));
        assert_eq!(values.field(2, 3), Some(id(Slot::None)));
        assert_eq!(values.field(1, 0), Some(id(Slot::Int(i32::MIN))));
    }
    #[test]
    fn enum_payloads_share_descriptors_without_merging_records() {
        let enumeration = || Value::Enum { type_name: "Kind".into(), value: 7 };
        let row = |key: &str, enumeration| Value::Object { type_name: "Row".into(), key: Some(key.into()), fields: vec![("kind".into(), enumeration)], bases: vec![] };
        let (values, ids) = compact(vec![enumeration(), enumeration(), row("a", 0), row("b", 1)]).unwrap();
        assert_eq!(ids[0], ids[1]); assert_ne!(ids[2], ids[3]);
        let values = FixedValues::new(values).unwrap();
        assert_eq!(values.enums.len(), 1); assert_eq!(values.field_bytes.len(), 8);
        assert_eq!(values.field(ids[2], 0), values.field(ids[3], 0));
    }

    #[test]
    fn fixed_publication_rejects_out_of_image_references() {
        assert!(FixedValues::new(vec![Value::None]).is_err());
        assert!(FixedValues::new(vec![Value::Array(vec![1].into())]).is_err());
        assert!(FixedValues::new(vec![Value::Template { source: "".into(), owner: Some(1) }]).is_err());
        assert!(FixedValues::new(vec![Value::Dimension { default: 1, variants: BTreeMap::new(), explicit: BTreeSet::new() }]).is_err());
    }
    #[test]
    fn borrowed_fixed_dictionary_keys_match_owned_hashes() {
        let keys = [ScalarKey::Bool(true), ScalarKey::Int(-7), ScalarKey::String("键".into()), ScalarKey::Enum { type_name: "Kind".into(), value: 3 }];
        for key in keys {
            let value = match &key {
                ScalarKey::Bool(value) => Value::Bool(*value), ScalarKey::Int(value) => Value::Int(*value),
                ScalarKey::String(value) => Value::String(value.clone()),
                ScalarKey::Enum { type_name, value } => Value::Enum { type_name: type_name.clone(), value: *value },
            };
            let (values, ids) = compact(vec![value, Value::Int(42), Value::Dict(indexmap::IndexMap::from([(key, (0, 1))]))]).unwrap();
            let values = FixedValues::new(values).unwrap();
            let key = values.scalar(ids[0]).unwrap_or(Slot::handle(ids[0]));
            assert_eq!(values.dictionary_index(ids[2], key), Some(Some(ids[1])));
        }
    }
    #[test]
    #[ignore = "布局选择测量，使用 release 单独运行并保存原始输出"]
    fn fixed_layout_probe() {
        use std::{hint::black_box, time::Instant};
        const ROWS: usize = 65_536;
        const ROUNDS: usize = 128;
        let aos = (0..ROWS).map(|row| std::array::from_fn::<_, 8, _>(|field| (row * 8 + field) as u32)).collect::<Vec<_>>();
        let soa = std::array::from_fn::<_, 8, _>(|field| (0..ROWS).map(|row| (row * 8 + field) as u32).collect::<Vec<_>>());
        let cells = (0..ROWS * 8).map(|value| 2 | ((value as u64) << 4)).collect::<Vec<_>>();
        let references = (0..ROWS * 8).collect::<Vec<_>>();
        // 同一负载同时经过生产布局解码，避免只测理想连续数组而遗漏描述符成本。
        let fixed = FixedValues::new((0..ROWS).map(|row| Value::Object {
            type_name: "Row".into(), key: None, bases: vec![],
            fields: (0..8).map(|field| (format!("f{field}"), Slot::Int((row * 8 + field) as i32).scalar_id().unwrap())).collect(),
        }).collect()).unwrap();
        println!("production_regions={:?}", fixed.storage_sizes());
        let sample = |name: &str, all_fields: bool, read: &dyn Fn(usize, usize) -> u32| {
            let mut elapsed = Vec::new();
            for _ in 0..7 {
                let start = Instant::now(); let mut total = 0u64;
                for _ in 0..ROUNDS {
                    for row in 0..ROWS {
                        if all_fields { for field in 0..8 { total += u64::from(read(black_box(row), field)); } }
                        else { total += u64::from(read(black_box(row), 3)); }
                    }
                }
                let expected = if all_fields { (ROWS as u64 * 8 - 1) * ROWS as u64 * 8 / 2 }
                    else { (ROWS as u64 - 1) * ROWS as u64 * 4 + ROWS as u64 * 3 };
                assert_eq!(total, expected * ROUNDS as u64); black_box(total);
                elapsed.push(start.elapsed().as_nanos());
            }
            elapsed.sort_unstable();
            println!("layout={name},all_fields={all_fields},rows={ROWS},rounds={ROUNDS},samples_ns={elapsed:?}");
        };
        for all in [false, true] {
            sample("production_typed_rows", all, &|row, field| {
                let Slot::Int(value) = Slot::from_scalar_id(fixed.field(row as u64, field).unwrap()).unwrap() else { panic!("expected int") };
                value as u32
            });
            sample("aos_u32", all, &|row, field| aos[row][field]);
            sample("soa_u32", all, &|row, field| soa[field][row]);
            sample("tagged_reference_u64", all, &|row, field| (cells[references[row * 8 + field]] >> 4) as u32);
        }
        println!("payload_bytes:aos={},soa={},tagged_reference={}", ROWS * 8 * 4, ROWS * 8 * 4,
            cells.capacity() * size_of::<u64>() + references.capacity() * size_of::<usize>());
        let optional = ArrayValue::pack((0..ROWS).map(|row| if row % 2 == 0 { Slot::None } else { Slot::Int(row as i32) }.scalar_id().unwrap()).collect()).unwrap();
        let payload = (0..ROWS as u32).collect::<Vec<_>>();
        let bitmap = vec![0b10101010u8; ROWS / 8];
        for packed in [true, false] {
            let mut elapsed = Vec::new();
            for _ in 0..7 {
                let start = Instant::now(); let mut total = 0u64;
                for _ in 0..ROUNDS { for row in 0..ROWS {
                    let row = black_box(row);
                    total += if packed {
                        match Slot::from_scalar_id(optional.get(row).unwrap()).unwrap() { Slot::Int(value) => value as u64, Slot::None => 0, _ => unreachable!() }
                    } else if bitmap[row / 8] & (1 << (row % 8)) != 0 { payload[row] as u64 } else { 0 };
                } }
                assert_eq!(total, (ROWS as u64 / 2).pow(2) * ROUNDS as u64); black_box(total);
                elapsed.push(start.elapsed().as_nanos());
            }
            elapsed.sort_unstable();
            println!("optional_layout=presence_{packed},rows={ROWS},rounds={ROUNDS},samples_ns={elapsed:?},payload_bytes={}", if packed { ROWS * 5 } else { ROWS * 4 + bitmap.len() });
        }

    }
}
pub(super) enum View<'a> {
    Scalar(Slot), String(&'a str), Object(Fields<'a>), Array(&'a ArrayValue), Dict(&'a Dictionary), Host, Other,
}
impl<'a> View<'a> {
    pub(super) fn dynamic(value: &'a Value) -> Self {
        match value {
            Value::None => Self::Scalar(Slot::None), Value::Bool(v) => Self::Scalar(Slot::Bool(*v)),
            Value::Int(v) => Self::Scalar(Slot::Int(*v)), Value::Float(v) => Self::Scalar(Slot::Float(*v)),
            Value::String(v) => Self::String(v), Value::Array(v) => Self::Array(v), Value::Dict(v) => Self::Dict(v),
            Value::Object { fields, .. } => Self::Object(Fields::Named(fields)), Value::HostData { .. } => Self::Host,
            _ => Self::Other,
        }
    }
}
#[derive(Debug, Default)]
pub(super) struct FixedValues {
    cells: Vec<u64>,
    utf8: String,
    strings: Vec<Range<usize>>,
    types: Vec<StringId>,
    field_names: Vec<StringId>,
    layouts: Vec<Layout>,
    objects: Vec<Object>,
    field_bytes: Vec<u8>,
    arrays: Vec<ArrayValue>,
    dictionaries: Vec<Dictionary>,
    enums: Vec<(TypeId, u32)>,
    dimensions: Vec<Dimension>,
    functions: Vec<Function>,
    templates: Vec<(Arc<str>, Option<ValueId>)>,
    hosts: Vec<Host>,
}
/// 发布前移除标量节点，所有边与外部索引一起重定位；记录和函数身份不做内容合并。
pub(super) fn compact(values: Vec<Value>) -> Result<(Vec<Value>, Vec<ValueId>), String> {
    let mut count = 0u64;
    let mut enum_ids = BTreeMap::new();
    let remap = values.iter().map(|value| {
        let scalar = match value { Value::None => Some(Slot::None), Value::Bool(v) => Some(Slot::Bool(*v)), Value::Int(v) => Some(Slot::Int(*v)), Value::Float(v) => Some(Slot::Float(*v)), _ => None };
        scalar.and_then(Slot::scalar_id).unwrap_or_else(|| {
            // enum 没有创建身份，同类型同值共享一份描述；记录和函数仍各自分配身份。
            if let Value::Enum { type_name, value } = value {
                return *enum_ids.entry((type_name.clone(), *value)).or_insert_with(|| { let id = count; count += 1; id });
            }
            let id = count; count += 1; id
        })
    }).collect::<Vec<_>>();
    let resolve = |id: ValueId| -> Result<ValueId, String> {
        if Slot::from_scalar_id(id).is_some() { return Ok(id); }
        remap.get(usize::try_from(id).map_err(|_| "固定引用超限")?).copied().ok_or_else(|| "固定引用越界".into())
    };
    let mut packed = Vec::with_capacity(count as usize);
    for (old, mut value) in values.into_iter().enumerate() {
        if Slot::from_scalar_id(remap[old]).is_some() || remap[old] < packed.len() as u64 { continue; }
        match &mut value {
            Value::Object { fields, bases, .. } => { for (_, id) in fields.iter_mut().chain(bases.iter_mut()) { *id = resolve(*id)?; } }
            Value::Array(items) => { *items = ArrayValue::pack_fixed(items.iter().map(resolve).collect::<Result<Vec<_>, _>>()?, count)?; }
            Value::Dict(items) => { for (key, value) in items.values_mut() { *key = resolve(*key)?; *value = resolve(*value)?; } }
            Value::Dimension { default, variants, .. } => { *default = resolve(*default)?; for value in variants.values_mut() { *value = resolve(*value)?; } }
            Value::Function { owner, .. } | Value::Template { owner, .. } => { *owner = owner.map(resolve).transpose()?; }
            _ => {},
        }
        packed.push(value);
    }
    Ok((packed, remap))
}
impl FixedValues {
    pub(super) fn new(values: Vec<Value>) -> Result<Self, String> {
        let count = values.len();
        if count > u32::MAX as usize { return Err("固定值身份数量超限".into()); }
        // 对同一实际类型和字段集合先合并载荷形态，避免 None 或 Host 覆盖制造逐行布局。
        let mut shapes = BTreeMap::<(String, Vec<String>), Vec<FieldEncoding>>::new();
        for value in &values {
            if let Value::Object { type_name, fields, .. } = value {
                let shape = (type_name.clone(), fields.iter().map(|(name, _)| name.clone()).collect());
                let encodings = shapes.entry(shape).or_insert_with(|| vec![FieldEncoding::default(); fields.len()]);
                for (encoding, (_, id)) in encodings.iter_mut().zip(fields) { encoding.observe(*id)?; }
            }
        }
        let mut result = Self::default();
        let mut strings = BTreeMap::<String, StringId>::new();
        let mut types = BTreeMap::<StringId, TypeId>::new();
        let mut layouts = BTreeMap::<(TypeId, Vec<StringId>), LayoutId>::new();
        fn intern(result: &mut FixedValues, strings: &mut BTreeMap<String, StringId>, text: String) -> Result<StringId, String> {
            if let Some(id) = strings.get(&text) { return Ok(*id); }
            let id = StringId(u32::try_from(result.strings.len()).map_err(|_| "固定字符串数量超限")?);
            let start = result.utf8.len(); result.utf8.push_str(&text);
            result.strings.push(start..result.utf8.len()); strings.insert(text, id); Ok(id)
        }
        fn ty(result: &mut FixedValues, strings: &mut BTreeMap<String, StringId>, types: &mut BTreeMap<StringId, TypeId>, name: String) -> Result<TypeId, String> {
            let name = intern(result, strings, name)?;
            if let Some(id) = types.get(&name) { return Ok(*id); }
            let id = TypeId(u32::try_from(result.types.len()).map_err(|_| "固定类型数量超限")?);
            result.types.push(name); types.insert(name, id); Ok(id)
        }
        for value in values {
            let (tag, payload): (u64, u64) = match value {
                Value::None | Value::Bool(_) | Value::Int(_) | Value::Float(_) => return Err("固定区发布前必须移除标量节点".into()),
                Value::String(text) => (4, intern(&mut result, &mut strings, text)?.0 as u64),
                Value::Enum { type_name, value } => {
                    let ty = ty(&mut result, &mut strings, &mut types, type_name)?;
                    let index = result.enums.len(); result.enums.push((ty, value)); (5, index as u64)
                }
                Value::Object { type_name, key, fields, bases } => {
                    let shape = (type_name.clone(), fields.iter().map(|(name, _)| name.clone()).collect());
                    let encodings = shapes.get(&shape).ok_or("固定字段布局缺失")?;
                    let object_type = ty(&mut result, &mut strings, &mut types, type_name)?;
                    let mut names = Vec::with_capacity(fields.len());
                    let start = result.field_bytes.len();
                    for ((name, value), encoding) in fields.into_iter().zip(encodings) {
                        names.push(intern(&mut result, &mut strings, name)?);
                        encoding.write(value, &mut result.field_bytes)?;
                    }
                    let fields = start..result.field_bytes.len();
                    let layout_key = (object_type, names.clone());
                    let layout = if let Some(layout) = layouts.get(&layout_key) { *layout } else {
                        let id = LayoutId(u32::try_from(result.layouts.len()).map_err(|_| "固定布局数量超限")?);
                        let mut fields = Vec::with_capacity(names.len());
                        let mut offset = 0u32;
                        for (name, encoding) in names.into_iter().zip(encodings) {
                            let field = FieldNameId(u32::try_from(result.field_names.len()).map_err(|_| "固定字段数量超限")?);
                            fields.push(FieldSpec { name: field, offset, encoding: *encoding });
                            offset = offset.checked_add(encoding.width() as u32).ok_or("固定对象布局超限")?;
                            result.field_names.push(name);
                        }
                        result.layouts.push(Layout { ty: object_type, fields }); layouts.insert(layout_key, id); id
                    };
                    let key = key.map(|key| intern(&mut result, &mut strings, key)).transpose()?;
                    let bases = bases.into_iter().map(|(name, value)| ty(&mut result, &mut strings, &mut types, name).map(|ty| (ty, value))).collect::<Result<_, _>>()?;
                    let index = result.objects.len(); result.objects.push(Object { layout, key, fields, bases }); (6, index as u64)
                }
                Value::Array(values) => {
                    let index = result.arrays.len(); result.arrays.push(values); (7, index as u64)
                }
                Value::Dict(values) => { let index = result.dictionaries.len(); result.dictionaries.push(values); (8, index as u64) }
                Value::Dimension { default, variants, explicit } => {
                    let mut stored = Vec::with_capacity(variants.len());
                    for (name, value) in variants { let present = explicit.contains(&name); stored.push((intern(&mut result, &mut strings, name)?, value, present)); }
                    let index = result.dimensions.len(); result.dimensions.push(Dimension { default, variants: stored }); (9, index as u64)
                }
                Value::Function { source, owner, host } => {
                    let host = host.map(|(service, field)| Ok::<_, String>((intern(&mut result, &mut strings, service)?, intern(&mut result, &mut strings, field)?))).transpose()?;
                    let index = result.functions.len(); result.functions.push(Function { source, owner, host }); (10, index as u64)
                }
                Value::Template { source, owner } => { let index = result.templates.len(); result.templates.push((source, owner)); (11, index as u64) }
                Value::HostData { service, field, value_type } => {
                    let service = intern(&mut result, &mut strings, service)?; let field = intern(&mut result, &mut strings, field)?;
                    let index = result.hosts.len(); result.hosts.push(Host { service, field, ty: value_type }); (12, index as u64)
                }
            };
            if payload > u64::MAX >> 4 { return Err("固定区过大".into()); }
            result.cells.push(tag | payload << 4);
        }
        // 所有边都指向同一映像，环允许存在；发布后读取无需借用动态堆。
        let invalid = result.objects.iter().any(|object| {
            let fields = &result.layouts[object.layout.0 as usize].fields;
            let bytes = &result.field_bytes[object.fields.clone()];
            fields.iter().any(|field| field.encoding.read(&bytes[field.offset as usize..]).is_none_or(|id| id >= count as u64 && Slot::from_scalar_id(id).is_none()))
        })
            || result.arrays.iter().any(|array| array.iter().any(|id| id >= count as u64 && Slot::from_scalar_id(id).is_none()))
            || result.objects.iter().any(|object| object.bases.iter().any(|(_, id)| *id >= count as u64))
            || result.dictionaries.iter().any(|values| values.values().any(|(key, value)| [key, value].iter().any(|id| **id >= count as u64 && Slot::from_scalar_id(**id).is_none())))
            || result.dimensions.iter().any(|dimension| (dimension.default >= count as u64 && Slot::from_scalar_id(dimension.default).is_none()) || dimension.variants.iter().any(|(_, id, _)| *id >= count as u64 && Slot::from_scalar_id(*id).is_none()))
            || result.functions.iter().any(|function| function.owner.is_some_and(|id| id >= count as u64))
            || result.templates.iter().any(|(_, owner)| owner.is_some_and(|id| id >= count as u64));
        if invalid { return Err("固定区引用越界".into()); }
        Ok(result)
    }
    #[cfg(test)]
    pub(super) fn storage_sizes(&self) -> Vec<(&'static str, usize)> {
        vec![
            ("identity_cells", self.cells.capacity() * size_of::<u64>()),
            ("utf8", self.utf8.capacity()),
            ("string_ranges", self.strings.capacity() * size_of::<Range<usize>>()),
            ("type_and_field_names", (self.types.capacity() + self.field_names.capacity()) * size_of::<StringId>()),
            ("layouts", self.layouts.capacity() * size_of::<Layout>() + self.layouts.iter().map(|layout| layout.fields.capacity() * size_of::<FieldSpec>()).sum::<usize>()),
            ("objects", self.objects.capacity() * size_of::<Object>() + self.objects.iter().map(|object| object.bases.capacity() * size_of::<(TypeId, ValueId)>()).sum::<usize>()),
            ("field_payloads", self.field_bytes.capacity()),
            ("arrays", self.arrays.capacity() * size_of::<ArrayValue>() + self.arrays.iter().map(ArrayValue::heap_bytes).sum::<usize>()),
            ("dictionaries", self.dictionaries.capacity() * size_of::<Dictionary>() + self.dictionaries.iter().map(|values| values.capacity() * (size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>() + 32) + values.keys().map(|key| match key { ScalarKey::String(text) => text.capacity(), ScalarKey::Enum { type_name, .. } => type_name.capacity(), _ => 0 }).sum::<usize>()).sum::<usize>()),
            ("dimensions", self.dimensions.capacity() * size_of::<Dimension>() + self.dimensions.iter().map(|dimension| dimension.variants.capacity() * size_of::<(StringId, ValueId, bool)>()).sum::<usize>()),
            ("callables_and_hosts", self.functions.capacity() * size_of::<Function>() + self.templates.capacity() * size_of::<(Arc<str>, Option<ValueId>)>() + self.hosts.capacity() * size_of::<Host>()),
            ("enums", self.enums.capacity() * size_of::<(TypeId, u32)>()),
        ]
    }
    fn text(&self, id: StringId) -> &str { &self.utf8[self.strings[id.0 as usize].clone()] }
    fn type_name(&self, id: TypeId) -> &str { self.text(self.types[id.0 as usize]) }
    fn cell(&self, id: ValueId) -> Option<(u64, usize)> { self.cells.get(usize::try_from(id).ok()?).map(|cell| (cell & 15, (cell >> 4) as usize)) }
    pub(super) fn len(&self) -> ValueId { self.cells.len() as ValueId }
    pub(super) fn scalar(&self, id: ValueId) -> Option<Slot> {
        Slot::from_scalar_id(id)
    }
    pub(super) fn view(&self, id: ValueId) -> Option<View<'_>> {
        if let Some(scalar) = self.scalar(id) { return Some(View::Scalar(scalar)); }
        let (tag, payload) = self.cell(id)?;
        Some(match tag {
            4 => View::String(self.text(StringId(payload as u32))),
            6 => { let object = &self.objects[payload]; View::Object(Fields::Packed { fields: &self.layouts[object.layout.0 as usize].fields, bytes: &self.field_bytes[object.fields.clone()] }) },
            7 => View::Array(&self.arrays[payload]),
            8 => View::Dict(&self.dictionaries[payload]), 12 => View::Host, _ => View::Other,
        })
    }
    pub(super) fn is_host(&self, id: ValueId) -> bool { matches!(self.cell(id), Some((12, _))) }
    pub(super) fn host(&self, id: ValueId) -> Option<(&str, &str, &CftValueType)> {
        let (12, index) = self.cell(id)? else { return None; }; let host = &self.hosts[index];
        Some((self.text(host.service), self.text(host.field), &host.ty))
    }
    pub(super) fn object_identity(&self, id: ValueId) -> Option<(&str, bool)> {
        let (6, index) = self.cell(id)? else { return None; };
        let object = &self.objects[index];
        Some((self.type_name(self.layouts[object.layout.0 as usize].ty), object.key.is_some()))
    }
    pub(super) fn object_type(&self, id: ValueId) -> Option<&str> {
        let (6, index) = self.cell(id)? else { return None; };
        Some(self.type_name(self.layouts[self.objects[index].layout.0 as usize].ty))
    }
    pub(super) fn field(&self, id: ValueId, slot: usize) -> Option<ValueId> {
        let View::Object(fields) = self.view(id)? else { return None; }; fields.get(slot)
    }
    pub(super) fn named_field(&self, id: ValueId, name: &str) -> Option<ValueId> {
        let (6, index) = self.cell(id)? else { return None; }; let object = &self.objects[index];
        let layout = &self.layouts[object.layout.0 as usize];
        let slot = layout.fields.iter().position(|field| self.text(self.field_names[field.name.0 as usize]) == name)?;
        self.field(id, slot)
    }
    pub(super) fn dictionary_index(&self, id: ValueId, key: Slot) -> Option<Option<ValueId>> {
        let (8, index) = self.cell(id)? else { return None; };
        let key = match key {
            Slot::Bool(value) => Key::Bool(value), Slot::Int(value) => Key::Int(value),
            Slot::Handle(id) => match self.cell(id.get())? {
                (4, index) => Key::String(self.text(StringId(index as u32))),
                (5, index) => { let (ty, value) = self.enums[index]; Key::Enum { type_name: self.type_name(ty), value } },
                _ => return None,
            },
            _ => return None,
        };
        Some(self.dictionaries[index].get(&key).map(|(_, value)| *value))
    }
    /// 通用只读适配的临时副本在创建前计费；固定热路径仍直接使用借用视图。
    pub(super) fn materialized_bytes(&self, id: ValueId) -> Option<usize> {
        if Slot::from_scalar_id(id).is_some() { return Some(0); }
        let (tag, payload) = self.cell(id)?;
        Some(match tag {
            4 => self.text(StringId(payload as u32)).len(),
            5 => self.type_name(self.enums[payload].0).len(),
            6 => {
                let object = &self.objects[payload]; let layout = &self.layouts[object.layout.0 as usize];
                self.type_name(layout.ty).len() + object.key.map_or(0, |key| self.text(key).len())
                    + layout.fields.len() * size_of::<(String, ValueId)>()
                    + layout.fields.iter().map(|field| self.text(self.field_names[field.name.0 as usize]).len()).sum::<usize>()
                    + object.bases.len() * size_of::<(String, ValueId)>()
                    + object.bases.iter().map(|(ty, _)| self.type_name(*ty).len()).sum::<usize>()
            }
            7 => self.arrays[payload].heap_bytes(),
            8 => {
                let values = &self.dictionaries[payload];
                values.len() * (size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>() + 32)
                    + values.keys().map(|key| match key { ScalarKey::String(text) => text.len(), ScalarKey::Enum { type_name, .. } => type_name.len(), _ => 0 }).sum::<usize>()
            }
            9 => self.dimensions[payload].variants.iter().map(|(name, _, explicit)| {
                // BTree 临时节点保守按每项独立节点计费，覆盖键和显式存在性集合。
                (self.text(*name).len() + 512) * if *explicit { 2 } else { 1 }
            }).sum(),
            10 => self.functions[payload].host.map_or(0, |(service, field)| self.text(service).len() + self.text(field).len()),
            11 => 0,
            12 => { let host = &self.hosts[payload]; self.text(host.service).len() + self.text(host.field).len() + 512 },
            _ => return None,
        })
    }
    pub(super) fn get(&self, id: ValueId) -> Option<Cow<'_, Value>> {
        if let Some(value) = super::inline_value(id) { return Some(Cow::Owned(value)); }
        let (tag, payload) = self.cell(id)?;
        Some(Cow::Owned(match tag {
            4 => Value::String(self.text(StringId(payload as u32)).into()),
            5 => { let (ty, value) = self.enums[payload]; Value::Enum { type_name: self.type_name(ty).into(), value } }
            6 => {
                let object = &self.objects[payload]; let layout = &self.layouts[object.layout.0 as usize];
                Value::Object { type_name: self.type_name(layout.ty).into(), key: object.key.map(|key| self.text(key).into()),
                    fields: layout.fields.iter().map(|field| (self.text(self.field_names[field.name.0 as usize]).into(), field.encoding.read(&self.field_bytes[object.fields.start + field.offset as usize..]).expect("固定字段发布前已验证"))).collect(),
                    bases: object.bases.iter().map(|(ty, id)| (self.type_name(*ty).into(), *id)).collect() }
            }
            7 => Value::Array(self.arrays[payload].clone()),
            8 => Value::Dict(self.dictionaries[payload].clone()),
            9 => { let dimension = &self.dimensions[payload]; Value::Dimension { default: dimension.default,
                variants: dimension.variants.iter().map(|(name, value, _)| (self.text(*name).into(), *value)).collect(),
                explicit: dimension.variants.iter().filter(|(_, _, present)| *present).map(|(name, _, _)| self.text(*name).into()).collect::<BTreeSet<_>>() } }
            10 => { let function = &self.functions[payload]; Value::Function { source: function.source.clone(), owner: function.owner, host: function.host.map(|(service, field)| (self.text(service).into(), self.text(field).into())) } }
            11 => { let (source, owner) = &self.templates[payload]; Value::Template { source: source.clone(), owner: *owner } }
            12 => { let host = &self.hosts[payload]; Value::HostData { service: self.text(host.service).into(), field: self.text(host.field).into(), value_type: host.ty.clone() } }
            _ => unreachable!("固定区标签在发布前构造"),
        }))
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = Cow<'_, Value>> { (0..self.len()).map(|id| self.get(id).expect("固定身份已验证")) }
}
