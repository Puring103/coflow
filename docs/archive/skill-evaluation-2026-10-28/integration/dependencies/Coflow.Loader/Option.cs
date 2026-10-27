using System;
using System.Collections.Generic;

namespace Coflow.Loader
{
/// <summary>One optional layer; None and Some(None) retain distinct states.</summary>
public readonly struct Option<T> : IEquatable<Option<T>>
{
    // 强类型节点打断 Option<结构体自身> 的内存布局递归，并保留每层存在状态。
    private sealed class SomeValue
    {
        internal SomeValue(T value) { Value = value; }
        internal T Value { get; }
    }
    private readonly SomeValue? _some;
    private Option(T value) { _some = new SomeValue(value); }
    public bool IsSome => _some != null;
    public bool IsNone => _some == null;
    public T Value => _some != null ? _some.Value : throw new InvalidOperationException("Option is None.");
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
