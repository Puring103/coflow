using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.ComponentModel;

/// <summary>生成 Schema 持有的不可变运行时描述；其生命周期不超过 Schema 实例。</summary>
[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowSchemaRuntime
{
    private readonly IReadOnlyDictionary<Type, CoflowTypeId> _types;
    private readonly IReadOnlyDictionary<Type, CoflowTypeDescriptor> _typeCodecs;
    private readonly IReadOnlyDictionary<Type, CoflowStructDescriptor> _structCodecs;
    private readonly CoflowLayoutRegistry _layouts;
    private readonly HashSet<string> _dimensionRecords;
    private readonly System.Collections.Concurrent.ConcurrentDictionary<(Type Type, bool Relative), Delegate>
        _boundaryWrites = new();
    private readonly System.Collections.Concurrent.ConcurrentDictionary<(Type Type, bool Relative), Delegate>
        _boundaryImportedWrites = new();
    private readonly System.Collections.Concurrent.ConcurrentDictionary<(Type Type, bool Relative), Delegate>
        _boundaryReads = new();

    internal CoflowSchemaRuntime(
        Dictionary<Type, CoflowTypeId> types,
        Dictionary<Type, CoflowTypeDescriptor> typeCodecs,
        Dictionary<Type, CoflowStructDescriptor> structCodecs,
        CoflowLayoutRegistry layouts, HashSet<string> dimensionRecords)
    {
        _types = new System.Collections.ObjectModel.ReadOnlyDictionary<Type, CoflowTypeId>(types);
        _typeCodecs = new System.Collections.ObjectModel.ReadOnlyDictionary<Type, CoflowTypeDescriptor>(typeCodecs);
        _structCodecs = new System.Collections.ObjectModel.ReadOnlyDictionary<Type, CoflowStructDescriptor>(structCodecs);
        _layouts = layouts;
        _dimensionRecords = dimensionRecords;
    }

    internal bool TryGetType(Type type, out CoflowTypeId typeId) =>
        _types.TryGetValue(type, out typeId);

    internal bool TryGetTypeCodec(Type type, out CoflowTypeDescriptor descriptor) =>
        _typeCodecs.TryGetValue(type, out descriptor!);

    internal bool TryGetTypeCodec<T>(out CoflowTypeDescriptor<T> descriptor)
    {
        if (_typeCodecs.TryGetValue(typeof(T), out var value))
        {
            descriptor = (CoflowTypeDescriptor<T>)value;
            return true;
        }
        descriptor = null!;
        return false;
    }

    internal bool TryGetStructCodec(Type type, out CoflowStructDescriptor descriptor) =>
        _structCodecs.TryGetValue(type, out descriptor!);

    internal CoflowStructDescriptor<T> GetStructCodec<T>() =>
        _structCodecs.TryGetValue(typeof(T), out var descriptor)
            ? (CoflowStructDescriptor<T>)descriptor
            : throw new InvalidOperationException($"No schema struct codec exists for `{typeof(T)}`.");

    internal CoflowLayoutRegistry CreateLayouts() => _layouts.Clone();

    internal bool TryGetLayout(Type type, out CoflowValueShape layout) =>
        _layouts.TryGet(type, out layout);

    internal bool IsDimensionRecord(string type) => _dimensionRecords.Contains(type);

    internal Action<CoflowExecutionSession, CoflowValueRegister, T> BoundaryWrite<T>(bool relative) =>
        (Action<CoflowExecutionSession, CoflowValueRegister, T>)_boundaryWrites.GetOrAdd(
            (typeof(T), relative), static key => CoflowBoundaryCodec.BuildWrite<T>(key.Relative));

    internal Action<CoflowExecutionSession, CoflowValueRegister, T> BoundaryImportedWrite<T>(bool relative) =>
        (Action<CoflowExecutionSession, CoflowValueRegister, T>)_boundaryImportedWrites.GetOrAdd(
            (typeof(T), relative), static key => CoflowBoundaryCodec.BuildImportingWrite<T>(key.Relative));

    internal Func<CoflowExecutionSession, CoflowValueRegister, T> BoundaryRead<T>(bool relative) =>
        (Func<CoflowExecutionSession, CoflowValueRegister, T>)_boundaryReads.GetOrAdd(
            (typeof(T), relative), static key => CoflowBoundaryCodec.BuildRead<T>(key.Relative));
}

/// <summary>由生成代码一次性建立 Schema runtime 描述，完成后通过 Build 封闭所有权。</summary>
[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowSchemaRuntimeBuilder
{
    private readonly HashSet<string> _dimensionRecords = new(StringComparer.Ordinal);

    public void RegisterDimension(string recordType)
    {
        EnsureMutable();
        _dimensionRecords.Add(recordType);
    }
    private readonly Dictionary<Type, CoflowTypeId> _types = new();
    private readonly Dictionary<Type, CoflowTypeDescriptor> _typeCodecs = new();
    private readonly Dictionary<Type, CoflowStructDescriptor> _structCodecs = new();
    private readonly CoflowLayoutRegistry _layouts = new();
    private bool _built;

    public CoflowSchemaRuntimeBuilder()
    {
        RegisterLayout(new CoflowValueShape(typeof(Unit), CoflowValueShapeKind.Unit, null, null, null));
        RegisterLayout(new CoflowValueShape(typeof(long), CoflowValueShapeKind.Scalar,
            CoflowRegisterKind.Integer, null, null));
        RegisterLayout(new CoflowValueShape(typeof(bool), CoflowValueShapeKind.Scalar,
            CoflowRegisterKind.Integer, null, null));
        RegisterLayout(new CoflowValueShape(typeof(double), CoflowValueShapeKind.Scalar,
            CoflowRegisterKind.Float, null, null));
        RegisterLayout(new CoflowValueShape(typeof(string), CoflowValueShapeKind.Scalar,
            CoflowRegisterKind.Reference, null, null));
    }

    public void RegisterEnum<T>() where T : struct, Enum =>
        RegisterLayout(new CoflowValueShape(typeof(T), CoflowValueShapeKind.Scalar,
            CoflowRegisterKind.Integer, null, null));

    public void RegisterOption<T>() => RegisterOption(typeof(Option<T>), typeof(T));

    public void RegisterResult<TOk, TError>() =>
        RegisterLayout(new CoflowValueShape(typeof(Result<TOk, TError>), CoflowValueShapeKind.Result,
            null, RequireLayout(typeof(TOk)), RequireLayout(typeof(TError))));

    public void RegisterArray<T>() => RegisterCollection(typeof(IReadOnlyList<T>));

    public void RegisterDictionary<TKey, TValue>() where TKey : notnull =>
        RegisterCollection(typeof(IReadOnlyDictionary<TKey, TValue>));

    public void RegisterFunction<TFunction>() =>
        RegisterLayout(new CoflowValueShape(typeof(TFunction), CoflowValueShapeKind.Function,
            null, null, null, 2, 0, 0));

    public void RegisterType<T>(CoflowTypeId typeId)
    {
        EnsureMutable();
        if (!typeId.IsValid) throw new ArgumentOutOfRangeException(nameof(typeId));
        RegisterType(typeof(T), typeId);
    }

    public void RegisterTypeCodec<T>(
        CoflowTypeId typeId,
        int arenaIntegerCount,
        int arenaFloatCount,
        int arenaReferenceCount,
        Func<T, CoflowValueId> getValueId,
        Func<T, bool> isInitialized,
        Func<T, CoflowValueId, T> withValueId,
        Func<CoflowImportContext, T, T> normalize,
        CoflowStructWriter<T> arenaWriter)
    {
        EnsureMutable();
        RegisterType(typeof(T), typeId);
        var descriptor = new CoflowTypeDescriptor<T>(typeId,
            arenaIntegerCount, arenaFloatCount, arenaReferenceCount,
            getValueId, isInitialized, withValueId, normalize, arenaWriter);
        if (!_typeCodecs.TryAdd(typeof(T), descriptor))
            throw new InvalidOperationException($"A type codec for `{typeof(T)}` is already registered.");
    }

    public void RegisterStruct<T>(
        int integerCount,
        int floatCount,
        int referenceCount,
        CoflowStructWriter<T> writer,
        CoflowStructReader<T> reader) where T : struct
    {
        EnsureMutable();
        if (integerCount < 1) throw new ArgumentOutOfRangeException(nameof(integerCount));
        if (floatCount < 0) throw new ArgumentOutOfRangeException(nameof(floatCount));
        if (referenceCount < 0) throw new ArgumentOutOfRangeException(nameof(referenceCount));
        var descriptor = new CoflowStructDescriptor<T>(
            integerCount, floatCount, referenceCount, writer, reader);
        if (!_structCodecs.TryAdd(typeof(T), descriptor))
            throw new InvalidOperationException($"A struct codec for `{typeof(T)}` is already registered.");
        RegisterLayout(new CoflowValueShape(typeof(T), CoflowValueShapeKind.Struct,
            null, null, null, integerCount, floatCount, referenceCount));
    }

    public CoflowSchemaRuntime Build()
    {
        EnsureMutable();
        _built = true;
        return new CoflowSchemaRuntime(
            new Dictionary<Type, CoflowTypeId>(_types),
            new Dictionary<Type, CoflowTypeDescriptor>(_typeCodecs),
            new Dictionary<Type, CoflowStructDescriptor>(_structCodecs),
            _layouts.Clone(), new HashSet<string>(_dimensionRecords, StringComparer.Ordinal));
    }

    private void RegisterType(Type type, CoflowTypeId typeId)
    {
        if (!typeId.IsValid) throw new ArgumentOutOfRangeException(nameof(typeId));
        if (_types.TryGetValue(type, out var existing))
        {
            if (existing.Value != typeId.Value)
                throw new InvalidOperationException($"Coflow type `{type}` has conflicting TypeIds.");
            return;
        }
        _types.Add(type, typeId);
        if (!_structCodecs.ContainsKey(type))
            RegisterLayout(new CoflowValueShape(type, CoflowValueShapeKind.Record,
                CoflowRegisterKind.Integer, null, null));
    }

    private void RegisterOption(Type type, Type itemType) =>
        RegisterLayout(new CoflowValueShape(type, CoflowValueShapeKind.Option,
            null, RequireLayout(itemType), null));

    private void RegisterCollection(Type type) =>
        RegisterLayout(new CoflowValueShape(type, CoflowValueShapeKind.Collection,
            CoflowRegisterKind.Integer, null, null));

    private CoflowValueShape RequireLayout(Type type)
    {
        if (_layouts.TryGet(type, out var layout)) return layout;
        throw new InvalidOperationException($"Schema layout dependency `{type}` is not registered.");
    }

    private void RegisterLayout(CoflowValueShape layout)
    {
        EnsureMutable();
        _layouts.Register(layout);
    }

    private void EnsureMutable()
    {
        if (_built) throw new InvalidOperationException("The Coflow schema runtime builder is already built.");
    }
}

internal static class CoflowSchemaRuntimeContext
{
    [ThreadStatic]
    private static CoflowSchemaRuntime? _current;

    internal static CoflowSchemaRuntime Current => TryGet(out var runtime)
        ? runtime
        : throw new InvalidOperationException(
            "Schema runtime metadata was accessed outside compilation or invocation.");

    internal static bool TryGet(out CoflowSchemaRuntime runtime)
    {
        if (_current is not null)
        {
            runtime = _current;
            return true;
        }
        return CoflowInvocationContext.TryGetRuntime(out runtime);
    }

    internal static bool TryGetType(Type type, out CoflowTypeId typeId)
    {
        if (TryGet(out var runtime)) return runtime.TryGetType(type, out typeId);
        typeId = default;
        return false;
    }

    internal static bool TryGetTypeCodec(Type type, out CoflowTypeDescriptor descriptor)
    {
        if (TryGet(out var runtime)) return runtime.TryGetTypeCodec(type, out descriptor);
        descriptor = null!;
        return false;
    }

    internal static bool TryGetTypeCodec<T>(out CoflowTypeDescriptor<T> descriptor)
    {
        if (TryGet(out var runtime)) return runtime.TryGetTypeCodec(out descriptor);
        descriptor = null!;
        return false;
    }

    internal static bool TryGetStructCodec(Type type, out CoflowStructDescriptor descriptor)
    {
        if (TryGet(out var runtime)) return runtime.TryGetStructCodec(type, out descriptor);
        descriptor = null!;
        return false;
    }

    internal static CoflowStructDescriptor<T> GetStructCodec<T>() => Current.GetStructCodec<T>();

    internal static Scope Enter(CoflowSchemaRuntime runtime)
    {
        if (runtime is null) throw new ArgumentNullException(nameof(runtime));
        if (_current is not null && !ReferenceEquals(_current, runtime))
            throw new InvalidOperationException("A Coflow operation cannot switch schema runtime scopes.");
        var owns = _current is null;
        _current = runtime;
        return new Scope(owns);
    }

    internal readonly struct Scope : IDisposable
    {
        private readonly bool _owns;
        internal Scope(bool owns) { _owns = owns; }
        public void Dispose()
        {
            if (_owns) _current = null;
        }
    }
}
}
