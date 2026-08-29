namespace CoflowRuntime.Generated;

/// <summary>Logical project path and UTF-16 source text for one CFD file.</summary>
internal readonly struct CfdSource : IEquatable<CfdSource>
{
    public CfdSource(string path, string text)
    {
        Path = path ?? throw new ArgumentNullException(nameof(path));
        Text = text ?? throw new ArgumentNullException(nameof(text));
    }

    public string Path { get; }
    public string Text { get; }

    public bool Equals(CfdSource other) => Path == other.Path && Text == other.Text;
    public override bool Equals(object? obj) => obj is CfdSource other && Equals(other);
    public override int GetHashCode() => HashCode.Combine(Path, Text);
}

internal interface ICfdTextLoader
{
    bool TryLoad(string logicalPath, out string? text);
}

internal sealed class DelegateCfdTextLoader : ICfdTextLoader
{
    private readonly Func<string, string?> _loader;

    public DelegateCfdTextLoader(Func<string, string?> loader) =>
        _loader = loader ?? throw new ArgumentNullException(nameof(loader));

    public bool TryLoad(string logicalPath, out string? text)
    {
        text = _loader(logicalPath);
        return text is not null;
    }
}
