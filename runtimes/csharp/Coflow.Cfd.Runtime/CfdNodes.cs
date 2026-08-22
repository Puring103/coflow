namespace Coflow.Cfd.Runtime;

public readonly struct CfdSpan : IEquatable<CfdSpan>
{
    public CfdSpan(int startLine, int startColumn, int endLine, int endColumn)
    {
        StartLine = startLine;
        StartColumn = startColumn;
        EndLine = endLine;
        EndColumn = endColumn;
    }

    public int StartLine { get; }
    public int StartColumn { get; }
    public int EndLine { get; }
    public int EndColumn { get; }
    public bool Equals(CfdSpan other) => StartLine == other.StartLine && StartColumn == other.StartColumn && EndLine == other.EndLine && EndColumn == other.EndColumn;
    public override bool Equals(object? obj) => obj is CfdSpan other && Equals(other);
    public override int GetHashCode() => HashCode.Combine(StartLine, StartColumn, EndLine, EndColumn);
    public override string ToString() => $"{StartLine}:{StartColumn}-{EndLine}:{EndColumn}";
}

public sealed class CfdDocument
{
    public CfdDocument(string path, IReadOnlyList<CfdRecordNode> records)
    {
        Path = path;
        Records = records;
    }

    public string Path { get; }
    public IReadOnlyList<CfdRecordNode> Records { get; }
}

public sealed class CfdRecordNode
{
    public CfdRecordNode(
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

public sealed class CfdFieldNode
{
    public CfdFieldNode(string name, CfdValueNode value, CfdSpan span)
    {
        Name = name;
        Value = value;
        Span = span;
    }

    public string Name { get; }
    public CfdValueNode Value { get; }
    public CfdSpan Span { get; }
}

public sealed class CfdEntryNode
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

public abstract class CfdValueNode
{
    protected CfdValueNode(CfdSpan span) => Span = span;
    public CfdSpan Span { get; }
}

public sealed class CfdNullValue : CfdValueNode
{
    public CfdNullValue(CfdSpan span) : base(span) { }
}

public sealed class CfdScalarValue : CfdValueNode
{
    public CfdScalarValue(string value, CfdSpan span) : base(span) => Value = value;
    public string Value { get; }
}

public sealed class CfdStringValue : CfdValueNode
{
    public CfdStringValue(string value, CfdSpan span) : base(span) => Value = value;
    public string Value { get; }
}

public sealed class CfdFormattedStringValue : CfdValueNode
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

public abstract record CfdFormatSegment;

public sealed record CfdFormatText(string Text) : CfdFormatSegment;

public sealed record CfdFormatReference(
    string? TypeName,
    string? Key,
    IReadOnlyList<string> Path) : CfdFormatSegment;

public sealed class CfdBitExpressionValue : CfdValueNode
{
    public CfdBitExpressionValue(CfdBitExpression expression, CfdSpan span) : base(span) => Expression = expression;
    public CfdBitExpression Expression { get; }
}

public sealed class CfdBitExpression
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

public abstract record CfdBitExpressionKind
{
    private CfdBitExpressionKind() { }

    public sealed record Value(string Text) : CfdBitExpressionKind;

    public sealed record Binary(
        CfdBitOperator Operator,
        CfdBitExpression Left,
        CfdBitExpression Right) : CfdBitExpressionKind;
}

public enum CfdBitOperator
{
    Or,
    Xor,
    And,
}

public sealed class CfdReferenceValue : CfdValueNode
{
    public CfdReferenceValue(string key, CfdSpan span) : base(span) => Key = key;
    public string Key { get; }
}

public sealed class CfdObjectValue : CfdValueNode
{
    public CfdObjectValue(string? declaredType, IReadOnlyList<CfdFieldNode> fields, CfdSpan span) : base(span)
    {
        DeclaredType = declaredType;
        Fields = fields;
    }

    public string? DeclaredType { get; }
    public IReadOnlyList<CfdFieldNode> Fields { get; }
}

public sealed class CfdArrayValue : CfdValueNode
{
    public CfdArrayValue(IReadOnlyList<CfdValueNode> items, CfdSpan span) : base(span) => Items = items;
    public IReadOnlyList<CfdValueNode> Items { get; }
}

public sealed class CfdDictionaryValue : CfdValueNode
{
    public CfdDictionaryValue(IReadOnlyList<CfdEntryNode> entries, CfdSpan span) : base(span) => Entries = entries;
    public IReadOnlyList<CfdEntryNode> Entries { get; }
}
