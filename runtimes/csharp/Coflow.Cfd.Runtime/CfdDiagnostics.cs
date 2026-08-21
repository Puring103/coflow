namespace Coflow.Cfd.Runtime;

public sealed class CfdDiagnostic
{
    public CfdDiagnostic(string code, string message, string path, CfdSpan? span = null)
    {
        Code = code;
        Message = message;
        Path = path;
        Span = span;
    }

    public string Code { get; }
    public string Message { get; }
    public string Path { get; }
    public CfdSpan? Span { get; }
    public override string ToString() => $"{Code}: {Message} ({Path}{(Span is null ? string.Empty : $":{Span}")})";
}

public class CfdLoadException : Exception
{
    public CfdLoadException(IReadOnlyList<CfdDiagnostic> diagnostics)
        : base(diagnostics.Count == 0 ? "CFD load failed." : diagnostics[0].Message)
    {
        Diagnostics = diagnostics;
    }

    public IReadOnlyList<CfdDiagnostic> Diagnostics { get; }
    public IReadOnlyList<CfdDiagnostic> Errors => Diagnostics;
}

public sealed class CfdParseException : CfdLoadException
{
    public CfdParseException(IReadOnlyList<CfdDiagnostic> errors)
        : base(errors) { }

}

public sealed class CfdLoadOptions
{
    public int MaxDepth { get; init; } = 128;
    public int MaxNodes { get; init; } = 1_000_000;
    public int MaxRecords { get; init; } = 1_000_000;
    public long MaxSourceBytes { get; init; } = 64L * 1024 * 1024;
}
