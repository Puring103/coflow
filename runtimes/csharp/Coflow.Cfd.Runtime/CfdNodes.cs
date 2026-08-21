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
    public CfdRecordNode(string key, string declaredType, IReadOnlyList<CfdFieldNode> fields, CfdSpan span)
    {
        Key = key;
        DeclaredType = declaredType;
        Fields = fields;
        Span = span;
    }

    public string Key { get; }
    public string DeclaredType { get; }
    public IReadOnlyList<CfdFieldNode> Fields { get; }
    public CfdSpan Span { get; }
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
