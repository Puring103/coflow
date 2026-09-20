//! 连续数值负载不建立逐元素堆节点；引用数组沿用稳定身份，读取统一返回 ValueId。
use super::ValueId;
use crate::{schema::CftValueType, vm::executor::Slot};

#[derive(Debug, Clone)]
enum Storage {
    Int(Vec<i32>), Float(Vec<f32>), Bool(Vec<u8>),
    OptionalNumber { float: bool, values: Vec<OptionalNumber> },
    OptionalBool(Vec<[u8; 2]>),
    Nones(usize),
    FixedReferences(Vec<u32>),
    References(Vec<ValueId>),
}
/// 有效标记与完整 32 位载荷分离；紧凑行没有填充，也不借用 NaN 或零表示 None。
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct OptionalNumber { bits: u32, present: u8 }
const _: () = assert!(size_of::<OptionalNumber>() == 5);
#[derive(Debug, Clone)]
pub struct ArrayValue(Storage);
impl ArrayValue {
    pub(super) fn empty(ty: &CftValueType) -> Self {
        if let CftValueType::Option(inner) = ty {
            return Self(match inner.as_ref() {
                CftValueType::Int => Storage::OptionalNumber { float: false, values: Vec::new() },
                CftValueType::Float => Storage::OptionalNumber { float: true, values: Vec::new() },
                CftValueType::Bool => Storage::OptionalBool(Vec::new()),
                _ => Storage::References(Vec::new()),
            });
        }
        Self(match ty { CftValueType::Int => Storage::Int(Vec::new()), CftValueType::Float => Storage::Float(Vec::new()), CftValueType::Bool => Storage::Bool(Vec::new()), _ => Storage::References(Vec::new()) })
    }
    pub fn len(&self) -> usize { match &self.0 { Storage::Int(v) => v.len(), Storage::Float(v) => v.len(), Storage::Bool(v) => v.len(), Storage::OptionalNumber { values, .. } => values.len(), Storage::OptionalBool(values) => values.len(), Storage::Nones(len) => *len, Storage::FixedReferences(values) => values.len(), Storage::References(v) => v.len() } }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub(super) fn capacity(&self) -> usize { match &self.0 { Storage::Int(v) => v.capacity(), Storage::Float(v) => v.capacity(), Storage::Bool(v) => v.capacity(), Storage::OptionalNumber { values, .. } => values.capacity(), Storage::OptionalBool(values) => values.capacity(), Storage::Nones(_) => usize::MAX, Storage::FixedReferences(values) => values.capacity(), Storage::References(v) => v.capacity() } }
    pub(super) fn element_bytes(&self) -> usize { match self.0 { Storage::Int(_) | Storage::Float(_) => 4, Storage::Bool(_) => 1, Storage::OptionalNumber { .. } => 5, Storage::OptionalBool(_) => 2, Storage::Nones(_) => 0, Storage::FixedReferences(_) => 4, Storage::References(_) => 8 } }
    pub(super) fn heap_bytes(&self) -> usize { self.capacity() * self.element_bytes() }
    pub(super) fn references(&self) -> &[ValueId] { if let Storage::References(values) = &self.0 { values } else { &[] } }
    pub fn get(&self, index: usize) -> Option<ValueId> {
        match &self.0 {
            Storage::Int(values) => Slot::Int(*values.get(index)?).scalar_id(),
            Storage::Float(values) => Slot::Float(*values.get(index)?).scalar_id(),
            Storage::Bool(values) => Slot::Bool(*values.get(index)? != 0).scalar_id(),
            Storage::OptionalNumber { float, values } => {
                let value = values.get(index)?;
                if value.present == 0 { Slot::None.scalar_id() }
                else if *float { Slot::Float(f32::from_bits(value.bits)).scalar_id() }
                else { Slot::Int(value.bits as i32).scalar_id() }
            }
            Storage::OptionalBool(values) => { let [present, value] = *values.get(index)?; if present == 0 { Slot::None.scalar_id() } else { Slot::Bool(value != 0).scalar_id() } }
            Storage::Nones(len) => if index < *len { Slot::None.scalar_id() } else { None },
            Storage::FixedReferences(values) => { let id = *values.get(index)?; if id == u32::MAX { Slot::None.scalar_id() } else { Some(u64::from(id)) } }
            Storage::References(values) => values.get(index).copied(),
        }
    }
    pub fn iter(&self) -> ArrayIter<'_> { ArrayIter { array: self, range: 0..self.len() } }
    pub(super) fn reserve(&mut self, additional: usize) -> Result<(), String> {
        if matches!(self.0, Storage::FixedReferences(_)) { return Err("固定引用缓冲不可修改".into()); }
        if let Storage::Nones(len) = &self.0 { return len.checked_add(additional).map(|_| ()).ok_or_else(|| "数组长度溢出".into()); }
        match &mut self.0 { Storage::Int(v) => v.try_reserve_exact(additional), Storage::Float(v) => v.try_reserve_exact(additional), Storage::Bool(v) => v.try_reserve_exact(additional), Storage::OptionalNumber { values, .. } => values.try_reserve_exact(additional), Storage::OptionalBool(values) => values.try_reserve_exact(additional), Storage::Nones(_) | Storage::FixedReferences(_) => unreachable!(), Storage::References(v) => v.try_reserve_exact(additional) }.map_err(|_| "数组缓冲分配失败".into())
    }
    pub(super) fn push(&mut self, value: ValueId) -> Result<(), String> {
        match (&mut self.0, Slot::from_scalar_id(value)) {
            (Storage::Int(v), Some(Slot::Int(value))) => v.push(value),
            (Storage::Float(v), Some(Slot::Float(value))) => v.push(value),
            (Storage::Bool(v), Some(Slot::Bool(value))) => v.push(u8::from(value)),
            (Storage::OptionalNumber { float: _, values }, Some(Slot::None)) => values.push(OptionalNumber { bits: 0, present: 0 }),
            (Storage::OptionalNumber { float: false, values }, Some(Slot::Int(value))) => values.push(OptionalNumber { bits: value as u32, present: 1 }),
            (Storage::OptionalNumber { float: true, values }, Some(Slot::Float(value))) => values.push(OptionalNumber { bits: value.to_bits(), present: 1 }),
            (Storage::OptionalBool(values), Some(Slot::None)) => values.push([0, 0]),
            (Storage::OptionalBool(values), Some(Slot::Bool(value))) => values.push([1, u8::from(value)]),
            (Storage::Nones(len), Some(Slot::None)) => *len = len.checked_add(1).ok_or("数组长度溢出")?,
            (Storage::References(v), _) => v.push(value),
            _ => return Err("数组元素类型不匹配".into()),
        }
        Ok(())
    }
    pub(super) fn set(&mut self, index: usize, value: ValueId) -> Result<(), String> {
        if index >= self.len() { return Err("数组索引越界".into()); }
        match (&mut self.0, Slot::from_scalar_id(value)) {
            (Storage::Int(v), Some(Slot::Int(value))) => v[index] = value,
            (Storage::Float(v), Some(Slot::Float(value))) => v[index] = value,
            (Storage::Bool(v), Some(Slot::Bool(value))) => v[index] = u8::from(value),
            (Storage::OptionalNumber { float: _, values }, Some(Slot::None)) => values[index] = OptionalNumber { bits: 0, present: 0 },
            (Storage::OptionalNumber { float: false, values }, Some(Slot::Int(value))) => values[index] = OptionalNumber { bits: value as u32, present: 1 },
            (Storage::OptionalNumber { float: true, values }, Some(Slot::Float(value))) => values[index] = OptionalNumber { bits: value.to_bits(), present: 1 },
            (Storage::OptionalBool(values), Some(Slot::None)) => values[index] = [0, 0],
            (Storage::OptionalBool(values), Some(Slot::Bool(value))) => values[index] = [1, u8::from(value)],
            (Storage::Nones(_), Some(Slot::None)) => {},
            (Storage::References(v), _) => v[index] = value,
            _ => return Err("数组元素类型不匹配".into()),
        }
        Ok(())
    }
    pub(super) fn remove(&mut self, index: usize) {
        match &mut self.0 { Storage::Int(v) => { v.remove(index); }, Storage::Float(v) => { v.remove(index); }, Storage::Bool(v) => { v.remove(index); }, Storage::OptionalNumber { values, .. } => { values.remove(index); }, Storage::OptionalBool(values) => { values.remove(index); }, Storage::Nones(len) => { *len -= 1; }, Storage::FixedReferences(_) => unreachable!("固定引用缓冲不可修改"), Storage::References(v) => { v.remove(index); } }
    }
    fn packed_type(values: &[ValueId]) -> Option<CftValueType> {
        let mut ty = None;
        let mut optional = false;
        for value in values {
            let current = match Slot::from_scalar_id(*value)? {
                Slot::None => { optional = true; continue; }
                Slot::Int(_) => CftValueType::Int, Slot::Float(_) => CftValueType::Float,
                Slot::Bool(_) => CftValueType::Bool, _ => return None,
            };
            if ty.as_ref().is_some_and(|ty| *ty != current) { return None; }
            ty = Some(current);
        }
        ty.map(|ty| if optional { CftValueType::Option(Box::new(ty)) } else { ty })
    }
    pub(super) fn packing_bytes(values: &[ValueId]) -> usize {
        Self::packed_type(values).map_or(0, |ty| values.len().saturating_mul(Self::empty(&ty).element_bytes()))
    }
    pub(super) fn pack_fixed(values: Vec<ValueId>, count: u64) -> Result<Self, String> {
        if count > u64::from(u32::MAX) { return Err("固定引用数组身份数量超限".into()); }
        let packed = Self::pack(values)?;
        let Storage::References(values) = &packed.0 else { return Ok(packed); };
        if !values.iter().all(|id| *id < count || Slot::from_scalar_id(*id) == Some(Slot::None)) { return Ok(packed); }
        let mut references = Vec::new();
        references.try_reserve_exact(values.len()).map_err(|_| "固定引用数组分配失败")?;
        for id in values {
            // 固定区最多 u32::MAX 个节点，最大有效索引为 u32::MAX - 1。
            references.push(if Slot::from_scalar_id(*id) == Some(Slot::None) { u32::MAX } else { u32::try_from(*id).map_err(|_| "固定引用数组越界")? });
        }
        Ok(Self(Storage::FixedReferences(references)))
    }
    pub(super) fn pack(values: Vec<ValueId>) -> Result<Self, String> {
        let Some(ty) = Self::packed_type(&values) else {
            if values.iter().all(|id| Slot::from_scalar_id(*id) == Some(Slot::None)) { return Ok(Self(Storage::Nones(values.len()))); }
            return Ok(Self(Storage::References(values)));
        };
        let mut result = Self::empty(&ty); result.reserve(values.len())?;
        for value in values { result.push(value)?; }
        Ok(result)
    }
}
impl From<Vec<ValueId>> for ArrayValue { fn from(values: Vec<ValueId>) -> Self { Self(Storage::References(values)) } }
impl FromIterator<ValueId> for ArrayValue { fn from_iter<T: IntoIterator<Item = ValueId>>(iter: T) -> Self { Self::from(iter.into_iter().collect::<Vec<_>>()) } }
#[derive(Debug)]
pub struct ArrayIter<'a> { array: &'a ArrayValue, range: std::ops::Range<usize> }
impl Iterator for ArrayIter<'_> {
    type Item = ValueId;
    fn next(&mut self) -> Option<ValueId> { self.array.get(self.range.next()?) }
    fn size_hint(&self) -> (usize, Option<usize>) { self.range.size_hint() }
}
impl DoubleEndedIterator for ArrayIter<'_> { fn next_back(&mut self) -> Option<ValueId> { self.array.get(self.range.next_back()?) } }
impl ExactSizeIterator for ArrayIter<'_> {}
impl<'a> IntoIterator for &'a ArrayValue {
    type Item = ValueId;
    type IntoIter = ArrayIter<'a>;
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(value: Slot) -> ValueId { value.scalar_id().unwrap() }

    #[test]
    fn packed_numbers_preserve_bits_and_do_not_expose_gc_edges() {
        let bits = [0, 0x8000_0000, 0x7fc0_0042, 0xff80_0000];
        let input = bits.map(|bits| id(Slot::Float(f32::from_bits(bits))));
        let array = ArrayValue::pack(input.to_vec()).unwrap();
        assert_eq!(array.iter().collect::<Vec<_>>(), input);
        assert_eq!(array.heap_bytes(), bits.len() * 4);
        assert!(array.references().is_empty());
        let integers = [i32::MIN, 0, i32::MAX, 0].map(|v| id(Slot::Int(v)));
        let array = ArrayValue::pack(integers.to_vec()).unwrap();
        assert_eq!(array.iter().rev().collect::<Vec<_>>(), integers.into_iter().rev().collect::<Vec<_>>());
        assert_eq!(array.element_bytes(), 4);
        assert!(array.references().is_empty());
    }

    #[test]
    fn optional_numbers_keep_presence_and_bits_without_gc_edges() {
        for values in [
            vec![id(Slot::None), id(Slot::Int(0)), id(Slot::Int(i32::MIN)), id(Slot::None)],
            vec![id(Slot::None), id(Slot::Float(-0.0)), id(Slot::Float(f32::from_bits(0x7fc0_0042))), id(Slot::None)],
            vec![id(Slot::None), id(Slot::Bool(false)), id(Slot::Bool(true)), id(Slot::None)],
        ] {
            let mut array = ArrayValue::pack(values.clone()).unwrap();
            assert_eq!(array.iter().collect::<Vec<_>>(), values);
            assert!(array.references().is_empty());
            assert_eq!(array.element_bytes(), if matches!(Slot::from_scalar_id(values[1]), Some(Slot::Bool(_))) { 2 } else { 5 });
            array.set(0, values[2]).unwrap(); array.set(2, id(Slot::None)).unwrap(); array.remove(1);
            assert_eq!(array.iter().collect::<Vec<_>>(), [values[2], id(Slot::None), id(Slot::None)]);
        }
        let none = ArrayValue::pack(vec![id(Slot::None); 1000]).unwrap();
        assert_eq!(none.len(), 1000); assert_eq!(none.heap_bytes(), 0); assert!(none.references().is_empty());
        assert_eq!(none.get(999), Some(id(Slot::None))); assert_eq!(none.get(1000), None);
    }

    #[test]
    fn typed_builder_updates_preserve_source_and_reject_wrong_types() {
        let mut source = ArrayValue::empty(&CftValueType::Bool);
        source.reserve(3).unwrap();
        source.push(id(Slot::Bool(false))).unwrap();
        source.push(id(Slot::Bool(true))).unwrap();
        let mut edited = source.clone();
        edited.set(0, id(Slot::Bool(true))).unwrap();
        edited.remove(1);
        assert!(edited.push(id(Slot::Int(1))).is_err());
        assert!(edited.set(1, id(Slot::Bool(false))).is_err());
        assert_eq!(source.iter().collect::<Vec<_>>(), [id(Slot::Bool(false)), id(Slot::Bool(true))]);
        assert_eq!(edited.iter().collect::<Vec<_>>(), [id(Slot::Bool(true))]);
        assert_eq!(source.heap_bytes(), 3);
        assert!(source.references().is_empty());
    }

    #[test]
    fn none_and_references_keep_order_and_identity() {
        let values = vec![id(Slot::None), id(Slot::Int(0)), 7, 7];
        let array = ArrayValue::pack(values.clone()).unwrap();
        assert_eq!(array.iter().collect::<Vec<_>>(), values);
        assert_eq!(array.references(), values);
        assert_eq!(array.get(values.len()), None);
        assert!(ArrayValue::pack(vec![]).unwrap().is_empty());
    }
}
