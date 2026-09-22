//! 寄存器标量与代数句柄的紧凑表示，不依赖解释器或 Host。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Slot {
    Empty,
    Unit,
    None,
    Bool(bool),
    Int(i32),
    Float(f32),
    Handle(HandleId),
}
/// 三个 16 位字使引用与 i32/f32 共用 8 字节槽，既不使用 NaN 装箱，也不截断浮点位模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HandleId([u16; 3]);
impl HandleId {
    pub(crate) fn get(self) -> u64 {
        u64::from(self.0[0]) | u64::from(self.0[1]) << 16 | u64::from(self.0[2]) << 32
    }
}
impl Slot {
    pub(crate) const MAX_HANDLE: u64 = (1u64 << 48) - 1;
    pub(crate) const MAX_HEAP_HANDLE: u64 = (1u64 << 44) - 1;
    /// 标量身份只编码内容，不分配堆节点；保留 None、NaN 位模式和负零的区别。
    pub(crate) fn scalar_id(self) -> Option<u64> {
        let (tag, bits) = match self {
            Self::None | Self::Unit => (1, 0),
            Self::Bool(value) => (2, u64::from(value)),
            Self::Int(value) => (3, u64::from(value as u32)),
            Self::Float(value) => (4, u64::from(value.to_bits())),
            _ => return None,
        };
        Some((tag << 44) | bits)
    }
    pub(crate) fn from_scalar_id(id: u64) -> Option<Self> {
        let bits = id & Self::MAX_HEAP_HANDLE;
        if bits > u64::from(u32::MAX) {
            return None;
        }
        match id >> 44 {
            1 if bits == 0 => Some(Self::None),
            2 if bits <= 1 => Some(Self::Bool(bits != 0)),
            3 => Some(Self::Int(bits as u32 as i32)),
            4 => Some(Self::Float(f32::from_bits(bits as u32))),
            _ => None,
        }
    }
    pub(crate) fn handle(id: u64) -> Self {
        assert!(id <= Self::MAX_HANDLE, "执行引用必须在发布前验证");
        Self::Handle(HandleId([
            id as u16,
            (id as u64 >> 16) as u16,
            (id as u64 >> 32) as u16,
        ]))
    }
}
const _: () = assert!(size_of::<Slot>() == 8);
