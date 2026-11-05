using System;
using System.Collections.Generic;

namespace Coflow.Loader
{
/// <summary>One optional layer; None and Some(None) retain distinct states.</summary>
public readonly struct Option<T> : IEquatable<Option<T>>
{
    // 装箱隔断递归值结构体的 CLR 布局；存在标记保留每层 Some/None。
    private readonly object? _value;
    private Option(T value) { _value = value; IsSome = true; }
    public bool IsSome { get; }
    public bool IsNone => !IsSome;
    public T Value => IsSome ? (T)_value! : throw new InvalidOperationException("Option is None.");
    public static Option<T> None => default;
    public static Option<T> Some(T value) => new Option<T>(value);
    // 每层单独保存存在状态，嵌套 Option 不使用可空值压平。
    public bool Equals(Option<T> other) => IsSome == other.IsSome && (!IsSome || EqualityComparer<T>.Default.Equals(Value, other.Value));
    public override bool Equals(object? other) => other is Option<T> value && Equals(value);
    public override int GetHashCode() => IsSome ? HashCode.Combine(true, Value) : 0;
    public static bool operator ==(Option<T> left, Option<T> right) => left.Equals(right);
    public static bool operator !=(Option<T> left, Option<T> right) => !left.Equals(right);
}
}
