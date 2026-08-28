namespace CoflowRuntime;

/// <summary>Load-time bound access to an ordinary generated object.</summary>
internal sealed class CoflowFieldAccess
{
    private CoflowFieldAccess(
        string name,
        Type runtimeType,
        bool isHost,
        CoflowNativeCall call)
    {
        Name = name;
        RuntimeType = runtimeType;
        IsHost = isHost;
        Call = call;
    }

    public string Name { get; }
    public Type RuntimeType { get; }
    public bool IsHost { get; }
    internal CoflowNativeCall Call { get; }

    internal static CoflowFieldAccess Bind(ICoflowTypeMetadata metadata, string fieldName)
    {
        if (metadata is null) throw new ArgumentNullException(nameof(metadata));
        return new CoflowFieldAccess(
            fieldName,
            metadata.GetFieldType(fieldName),
            metadata is ICoflowHostMetadata,
            new CoflowNativeCall(metadata.GetFieldReader(fieldName)));
    }
}
