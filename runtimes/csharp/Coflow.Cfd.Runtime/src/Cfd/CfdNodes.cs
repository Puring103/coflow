namespace CoflowRuntime.Generated;

using System.ComponentModel;

internal sealed class CfdDocument
{
    public CfdDocument(
        string path,
        IReadOnlyList<CfdRecordNode> records)
    {
        Path = path;
        Records = records;
    }

    public string Path { get; }
    public IReadOnlyList<CfdRecordNode> Records { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CfdRecordNode
{
    internal CfdRecordNode(
        string key,
        string declaredType,
        IReadOnlyList<CfdFieldNode> fields,
        CfdSpan span,
        string? groupType = null)
    {
        Key = key;
        DeclaredType = declaredType;
        Fields = fields;
        Span = span;
        GroupType = groupType;
    }

    public string Key { get; }
    public string DeclaredType { get; }
    public IReadOnlyList<CfdFieldNode> Fields { get; }
    public CfdSpan Span { get; }
    public string? GroupType { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CfdFieldNode
{
    internal CfdFieldNode(string name, CfdValueNode value, CfdSpan span)
    {
        Name = name;
        Value = value;
        Span = span;
    }

    public string Name { get; }
    public CfdValueNode Value { get; }
    public CfdSpan Span { get; }
}

internal sealed class CfdEntryNode
{
    public CfdEntryNode(CfdValueNode key, CfdValueNode value, CfdSpan span)
    {
        Key = key;
        Value = value;
        Span = span;
    }

    public CfdValueNode Key { get; }
    public CfdValueNode Value { get; }
    public CfdSpan Span { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public abstract class CfdValueNode
{
    internal CfdValueNode(CfdSpan span) => Span = span;
    public CfdSpan Span { get; }
}

internal sealed class CfdInvalidValue : CfdValueNode
{
    internal CfdInvalidValue(CfdSpan span) : base(span) { }
}

internal sealed class CfdNoneValue : CfdValueNode
{
    public CfdNoneValue(CfdSpan span) : base(span) { }
}

internal sealed class CfdSomeValue : CfdValueNode
{
    public CfdSomeValue(CfdValueNode value, CfdSpan span) : base(span) => Value = value;
    public CfdValueNode Value { get; }
}

internal sealed class CfdOkValue : CfdValueNode
{
    public CfdOkValue(CfdValueNode value, CfdSpan span) : base(span) => Value = value;
    public CfdValueNode Value { get; }
}

internal sealed class CfdErrValue : CfdValueNode
{
    public CfdErrValue(CfdValueNode value, CfdSpan span) : base(span) => Value = value;
    public CfdValueNode Value { get; }
}

internal sealed class CfdScalarValue : CfdValueNode
{
    public CfdScalarValue(string value, CfdSpan span) : base(span) => Value = value;
    public string Value { get; }
}

internal sealed class CfdConstantValue : CfdValueNode
{
    internal CfdConstantValue(CoflowConstant constant, CfdSpan span) : base(span) =>
        Constant = constant;

    internal CoflowConstant Constant { get; }
}

internal sealed class CfdStringValue : CfdValueNode
{
    public CfdStringValue(string value, CfdSpan span) : base(span) => Value = value;
    public string Value { get; }
}

internal sealed class CfdFormattedStringValue : CfdValueNode
{
    public CfdFormattedStringValue(string source, IReadOnlyList<CfdFormatSegment> segments, CfdSpan span)
        : base(span)
    {
        Source = source;
        Segments = segments;
    }

    public string Source { get; }
    public IReadOnlyList<CfdFormatSegment> Segments { get; }
}

internal sealed class CfdFunctionValue : CfdValueNode
{
    public CfdFunctionValue(string source, CfdSpan span) : base(span) => Source = source;
    public string Source { get; }
}

internal abstract record CfdFormatSegment;

internal sealed record CfdFormatText(string Text) : CfdFormatSegment;

internal sealed record CfdFormatReference(
    string? TypeName,
    string? Key,
    IReadOnlyList<string> Path) : CfdFormatSegment;

internal sealed class CfdBitExpressionValue : CfdValueNode
{
    public CfdBitExpressionValue(CfdBitExpression expression, CfdSpan span) : base(span) => Expression = expression;
    public CfdBitExpression Expression { get; }
}

internal sealed class CfdBitExpression
{
    private CfdBitExpression(CfdBitExpressionKind kind, CfdSpan span)
    {
        Kind = kind;
        Span = span;
    }

    public CfdBitExpressionKind Kind { get; }
    public CfdSpan Span { get; }

    public static CfdBitExpression Value(string value, CfdSpan span) =>
        new(new CfdBitExpressionKind.Value(value), span);

    public static CfdBitExpression Binary(
        CfdBitOperator operation,
        CfdBitExpression left,
        CfdBitExpression right,
        CfdSpan span) =>
        new(new CfdBitExpressionKind.Binary(operation, left, right), span);

    internal CfdBitExpression WithSpan(CfdSpan span) => new(Kind, span);
}

internal abstract record CfdBitExpressionKind
{
    private CfdBitExpressionKind() { }

    public sealed record Value(string Text) : CfdBitExpressionKind;

    public sealed record Binary(
        CfdBitOperator Operator,
        CfdBitExpression Left,
        CfdBitExpression Right) : CfdBitExpressionKind;
}

internal enum CfdBitOperator
{
    Or,
    Xor,
    And,
}

internal sealed class CfdReferenceValue : CfdValueNode
{
    public CfdReferenceValue(string? typeName, string key, CfdSpan span) : base(span)
    {
        TypeName = typeName;
        Key = key;
    }

    public string? TypeName { get; }
    public string Key { get; }
}

internal sealed class CfdObjectValue : CfdValueNode
{
    public CfdObjectValue(string? declaredType, IReadOnlyList<CfdFieldNode> fields, CfdSpan span) : base(span)
    {
        DeclaredType = declaredType;
        Fields = fields;
    }

    public string? DeclaredType { get; }
    public IReadOnlyList<CfdFieldNode> Fields { get; }
}

internal sealed class CfdArrayValue : CfdValueNode
{
    public CfdArrayValue(IReadOnlyList<CfdValueNode> items, CfdSpan span) : base(span) => Items = items;
    public IReadOnlyList<CfdValueNode> Items { get; }
}

internal sealed class CfdDictionaryValue : CfdValueNode
{
    public CfdDictionaryValue(IReadOnlyList<CfdEntryNode> entries, CfdSpan span) : base(span) => Entries = entries;
    public IReadOnlyList<CfdEntryNode> Entries { get; }
}
