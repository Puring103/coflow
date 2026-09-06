namespace Coflow.Runtime;

public sealed class CoflowOptions
{
    public static CoflowOptions Default { get; } = new();

    public CoflowOptions(
        long maxInstructions = 10_000_000,
        int maxFrameDepth = 1_024,
        int maxIntegerRegisters = 1_000_000,
        int maxFloatRegisters = 1_000_000,
        int maxReferenceRegisters = 1_000_000,
        int maxInvocationValues = 100_000,
        int maxCollectionElements = 1_000_000,
        long maxCollectionWork = 10_000_000,
        int maxClosureLanes = 1_000_000,
        int maxHostCalls = 1_000_000,
        int maxBoundaryLanes = 10_000_000,
        int maxEscapedValues = 100_000,
        int maxEscapedLanes = 10_000_000)
    {
        MaxInstructions = Positive(maxInstructions, nameof(maxInstructions));
        MaxFrameDepth = Positive(maxFrameDepth, nameof(maxFrameDepth));
        MaxIntegerRegisters = Positive(maxIntegerRegisters, nameof(maxIntegerRegisters));
        MaxFloatRegisters = Positive(maxFloatRegisters, nameof(maxFloatRegisters));
        MaxReferenceRegisters = Positive(maxReferenceRegisters, nameof(maxReferenceRegisters));
        MaxInvocationValues = Positive(maxInvocationValues, nameof(maxInvocationValues));
        MaxCollectionElements = Positive(maxCollectionElements, nameof(maxCollectionElements));
        MaxCollectionWork = Positive(maxCollectionWork, nameof(maxCollectionWork));
        MaxClosureLanes = Positive(maxClosureLanes, nameof(maxClosureLanes));
        MaxHostCalls = Positive(maxHostCalls, nameof(maxHostCalls));
        MaxBoundaryLanes = Positive(maxBoundaryLanes, nameof(maxBoundaryLanes));
        MaxEscapedValues = Positive(maxEscapedValues, nameof(maxEscapedValues));
        MaxEscapedLanes = Positive(maxEscapedLanes, nameof(maxEscapedLanes));
    }

    public long MaxInstructions { get; }
    public int MaxFrameDepth { get; }
    public int MaxIntegerRegisters { get; }
    public int MaxFloatRegisters { get; }
    public int MaxReferenceRegisters { get; }
    public int MaxInvocationValues { get; }
    public int MaxCollectionElements { get; }
    public long MaxCollectionWork { get; }
    public int MaxClosureLanes { get; }
    public int MaxHostCalls { get; }
    public int MaxBoundaryLanes { get; }
    public int MaxEscapedValues { get; }
    public int MaxEscapedLanes { get; }

    private static int Positive(int value, string name) => value > 0
        ? value : throw new ArgumentOutOfRangeException(name, "Coflow limits must be positive.");

    private static long Positive(long value, string name) => value > 0
        ? value : throw new ArgumentOutOfRangeException(name, "Coflow limits must be positive.");
}

public sealed class CoflowExecutionLimitException : InvalidOperationException
{
    internal CoflowExecutionLimitException(string limit) : base($"Coflow execution exceeded `{limit}`.")
    {
        Limit = limit;
    }

    public string Limit { get; }
}
