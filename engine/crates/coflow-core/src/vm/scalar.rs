//! 编译期折叠与运行期共用的纯标量语义。
use std::cmp::Ordering;
use super::{ExecutionError, error, slot::Slot};
pub(super) fn integer(value: Slot) -> Result<i32, ExecutionError> {
    if let Slot::Int(value) = value {
        Ok(value)
    } else {
        Err(error("需要 int"))
    }
}
pub(super) fn boolean(value: Slot) -> Result<bool, ExecutionError> {
    if let Slot::Bool(value) = value {
        Ok(value)
    } else {
        Err(error("需要 bool"))
    }
}
pub(super) fn int_binary(op: u8, left: i32, right: i32) -> Result<Slot, ExecutionError> {
    if (7..=12).contains(&op) {
        return Ok(Slot::Bool(match op {
            7 => left == right,
            8 => left != right,
            9 => left < right,
            10 => left <= right,
            11 => left > right,
            _ => left >= right,
        }));
    }
    let value = match op {
        0 => left.checked_add(right),
        1 => left.checked_sub(right),
        2 => left.checked_mul(right),
        4 => left.checked_div(right),
        5 => left.checked_rem(right),
        6 => u32::try_from(right)
            .ok()
            .and_then(|right| left.checked_pow(right)),
        13 => u32::try_from(right)
            .ok()
            .filter(|right| *right < 32)
            .map(|right| left.wrapping_shl(right)),
        14 => u32::try_from(right)
            .ok()
            .filter(|right| *right < 32)
            .map(|right| left >> right),
        15 => Some(left & right),
        16 => Some(left | right),
        17 => Some(left ^ right),
        _ => return Err(error("无效的 int 运算")),
    };
    value
        .map(Slot::Int)
        .ok_or_else(|| error("整数溢出、除零、负指数或非法移位"))
}

pub(super) fn float_binary(op: u8, left: f32, right: f32) -> Result<Slot, ExecutionError> {
    Ok(match op {
        0 => Slot::Float(left + right),
        1 => Slot::Float(left - right),
        2 => Slot::Float(left * right),
        3 => Slot::Float(left / right),
        6 => Slot::Float(left.powf(right)),
        7 => Slot::Bool(left == right),
        8 => Slot::Bool(left != right),
        9 => Slot::Bool(left < right),
        10 => Slot::Bool(left <= right),
        11 => Slot::Bool(left > right),
        12 => Slot::Bool(left >= right),
        _ => return Err(error("无效的 float 运算")),
    })
}
/// 标量快速路径：int/float 运算、标量相等与同型顺序比较在寄存器内完成，
/// 返回 None 表示需要 Host 语义（结构相等、连接或 flag 位运算）。
pub(crate) fn scalar_binary(
    op: u8,
    left: Slot,
    right: Slot,
) -> Option<Result<Slot, ExecutionError>> {
    // 相等：标量对在寄存器内比较，语义与 Host equal 的标量分支一致。
    if op == 7 || op == 8 {
        let equal = match (left, right) {
            (Slot::Bool(a), Slot::Bool(b)) => a == b,
            (Slot::Int(a), Slot::Int(b)) => a == b,
            (Slot::Float(a), Slot::Float(b)) => a == b,
            (Slot::Int(a), Slot::Float(b)) => a as f32 == b,
            (Slot::Float(a), Slot::Int(b)) => a == b as f32,
            (Slot::None, Slot::None) => true,
            _ => return None,
        };
        return Some(Ok(Slot::Bool(if op == 7 { equal } else { !equal })));
    }
    // 顺序比较：仅同型标量在寄存器内完成，与 Host compare 语义一致。
    if (9..=12).contains(&op) {
        let ordering = match (left, right) {
            (Slot::Int(a), Slot::Int(b)) => Some(a.cmp(&b)),
            (Slot::Float(a), Slot::Float(b)) => a.partial_cmp(&b),
            _ => return None,
        };
        return Some(Ok(Slot::Bool(match op {
            9 => ordering == Some(Ordering::Less),
            10 => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
            11 => ordering == Some(Ordering::Greater),
            _ => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
        })));
    }
    match (left, right) {
        (Slot::Int(a), Slot::Int(b)) => Some(
            match op {
                0 => a.checked_add(b),
                1 => a.checked_sub(b),
                2 => a.checked_mul(b),
                4 => a.checked_div(b),
                5 => a.checked_rem(b),
                6 => u32::try_from(b).ok().and_then(|b| a.checked_pow(b)),
                13 => u32::try_from(b)
                    .ok()
                    .filter(|b| *b < 32)
                    .map(|b| a.wrapping_shl(b)),
                14 => u32::try_from(b).ok().filter(|b| *b < 32).map(|b| a >> b),
                15 => Some(a & b),
                16 => Some(a | b),
                17 => Some(a ^ b),
                _ => return Some(Err(error("无效的 int 运算"))),
            }
            .map(Slot::Int)
            .ok_or_else(|| error("整数溢出、除零、负指数或非法移位")),
        ),
        (Slot::Float(a), Slot::Float(b)) => Some(Ok(Slot::Float(match op {
            0 => a + b,
            1 => a - b,
            2 => a * b,
            3 => a / b,
            6 => a.powf(b),
            _ => return Some(Err(error("无效的 float 运算"))),
        }))),
        _ => None,
    }
}

