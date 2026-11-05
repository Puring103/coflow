using System;

namespace Coflow.Loader
{
// 已绑定值仅服务加载构建：固定值共享，表达式在请求上下文中求值。
internal abstract class BoundValue
{
    internal string Path { get; set; } = "";
    internal CfdSpan Span { get; set; }
    internal abstract bool IsFixed { get; }
    internal abstract object? Evaluate(CfdReader reader);
    internal abstract object? Read(CfdReader reader);
    internal virtual CfdReader? Traverse(CfdReader reader) => null;
}
internal sealed class BoundValue<T> : BoundValue
{
    internal readonly T Fixed;
    private readonly object? _boxed;
    private readonly Func<CfdReader, T>? _evaluate;
    private readonly Func<CfdReader, CfdReader>? _traverse;
    internal BoundValue(T value, Func<CfdReader, CfdReader>? traverse = null) { Fixed = value; _boxed = value; _traverse = traverse; }
    internal BoundValue(Func<CfdReader, T> evaluate, Func<CfdReader, CfdReader>? traverse = null)
    { Fixed = default!; _evaluate = evaluate; _traverse = traverse; }
    internal override bool IsFixed => _evaluate == null;
    internal T Get(CfdReader reader)
    {
        if (_evaluate == null) return Fixed;
        return (T)reader.ReadDynamic(this)!;
    }
    internal override object? Read(CfdReader reader) => IsFixed ? _boxed : Get(reader);
    internal override object? Evaluate(CfdReader reader) => _evaluate!(reader);
    internal override CfdReader? Traverse(CfdReader reader) => _traverse?.Invoke(reader);
}
internal sealed class BindingScope
{
    internal BindingScope(CfdType type, string path) { Type = type; Path = path; }
    internal CfdType Type { get; }
    internal string Path { get; }
    internal CfdLoadException Error(CfdValueNode node, string code, string message) => LoadContext.Error(code, message, Path, node.Span);
}

}
