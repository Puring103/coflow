using Coflow.Runtime.CompilerServices;
using System;
using System.Collections.Generic;
using System.Collections.Concurrent;
using System.Linq;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Threading.Tasks;
using Xunit;

namespace Coflow.Runtime.Tests;

public sealed class CoflowLayoutRegistryTests
{
    [Fact]
    public void SchemaRuntimeKeepsTypeIdentityInsideEachSchema()
    {
        var firstBuilder = new CoflowSchemaRuntimeBuilder();
        firstBuilder.RegisterType<SharedRecord>(new CoflowTypeId(1));
        var first = firstBuilder.Build();
        var secondBuilder = new CoflowSchemaRuntimeBuilder();
        secondBuilder.RegisterType<SharedRecord>(new CoflowTypeId(9));
        var second = secondBuilder.Build();

        using (CoflowSchemaRuntimeContext.Enter(first))
        {
            Assert.True(CoflowSchemaRuntimeContext.TryGetType(
                typeof(SharedRecord), out var typeId));
            Assert.Equal(new CoflowTypeId(1), typeId);
        }
        using (CoflowSchemaRuntimeContext.Enter(second))
        {
            Assert.True(CoflowSchemaRuntimeContext.TryGetType(
                typeof(SharedRecord), out var typeId));
            Assert.Equal(new CoflowTypeId(9), typeId);
        }
    }

    [Fact]
    public void SchemaRuntimeBuildsBoundaryAdaptersOnceUnderParallelFirstUse()
    {
        var builder = new CoflowSchemaRuntimeBuilder();
        builder.RegisterStruct<SharedStruct>(1, 0, 0,
            static (ref CoflowValueWriter writer, SharedStruct value) => writer.Write(value.Value),
            static (ref CoflowValueReader reader) => new SharedStruct(reader.Read<long>()));
        var runtime = builder.Build();
        var adapters = new ConcurrentBag<Delegate>();

        Parallel.For(0, 32, _ =>
        {
            using var scope = CoflowSchemaRuntimeContext.Enter(runtime);
            adapters.Add(runtime.BoundaryWrite<SharedStruct>(false));
        });

        var expected = Assert.Single(adapters.Distinct());
        Assert.All(adapters, adapter => Assert.Same(expected, adapter));
        Assert.Throws<InvalidOperationException>(() => builder.RegisterArray<long>());
    }

    [Fact]
    public void NestedSchemaScopeRestoresStateAfterRejectedSwitch()
    {
        var first = RuntimeForSharedRecord(new CoflowTypeId(1), static value => value.First);
        var second = RuntimeForSharedRecord(new CoflowTypeId(2), static value => value.Second);

        using (CoflowSchemaRuntimeContext.Enter(first))
        {
            using (CoflowSchemaRuntimeContext.Enter(first))
                Assert.Same(first, CoflowSchemaRuntimeContext.Current);

            Assert.Throws<InvalidOperationException>(() =>
                CoflowSchemaRuntimeContext.Enter(second));
            Assert.Same(first, CoflowSchemaRuntimeContext.Current);
        }

        Assert.False(CoflowSchemaRuntimeContext.TryGet(out _));
    }

    [Fact]
    public void StaticEscapeAdapterResolvesTheCurrentSchemaCodec()
    {
        var first = RuntimeForSharedRecord(new CoflowTypeId(1), static value => value.First);
        var second = RuntimeForSharedRecord(new CoflowTypeId(9), static value => value.Second);
        var value = new SharedRecord
        {
            First = new CoflowValueId(7, 11),
            Second = new CoflowValueId(8, 22),
        };

        var firstCollector = new CoflowValueIdCollector();
        using (CoflowSchemaRuntimeContext.Enter(first))
            CoflowEscapeValue<SharedRecord>.Collect(value, firstCollector);
        Assert.Contains(value.First, firstCollector.Ids);
        Assert.DoesNotContain(value.Second, firstCollector.Ids);

        var secondCollector = new CoflowValueIdCollector();
        using (CoflowSchemaRuntimeContext.Enter(second))
            CoflowEscapeValue<SharedRecord>.Collect(value, secondCollector);
        Assert.Contains(value.Second, secondCollector.Ids);
        Assert.DoesNotContain(value.First, secondCollector.Ids);
    }

    [Fact]
    public void CandidateClosedLayoutsDoNotEscapeTheirCompilationScope()
    {
        var type = typeof(IReadOnlyList<IReadOnlyList<long>>);
        var candidate = new CoflowLayoutRegistry();

        using (CoflowLayoutCompilation.Enter(candidate))
        {
            var layout = CoflowValueShape.Of(type);
            Assert.Equal(CoflowValueShapeKind.Collection, layout.Kind);
            Assert.True(candidate.TryGet(type, out var registered));
            Assert.Same(layout, registered);
        }

        Assert.Throws<InvalidOperationException>(() => CoflowValueShape.Of(type));
    }

    [Fact]
    public void EachCandidateRebuildsClosedLayoutsIndependently()
    {
        var type = typeof(IReadOnlyDictionary<string, IReadOnlyList<IReadOnlyList<long>>>);
        var first = new CoflowLayoutRegistry();
        var second = new CoflowLayoutRegistry();
        CoflowValueShape firstLayout;

        using (CoflowLayoutCompilation.Enter(first))
            firstLayout = CoflowValueShape.Of(type);
        using (CoflowLayoutCompilation.Enter(second))
        {
            var secondLayout = CoflowValueShape.Of(type);
            Assert.NotSame(firstLayout, secondLayout);
            Assert.True(second.TryGet(type, out var registered));
            Assert.Same(secondLayout, registered);
        }

        Assert.Throws<InvalidOperationException>(() => CoflowValueShape.Of(type));
    }

    [Theory]
    [InlineData("boundary")]
    [InlineData("union")]
    [InlineData("dictionary")]
    [InlineData("receiver")]
    public void RuntimeAdaptersDoNotKeepCollectibleAssemblyAlive(string adapter)
    {
        var collectible = CreateCollectibleAdapter(adapter);

        for (var attempt = 0; collectible.IsAlive && attempt < 10; attempt++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
            GC.Collect();
        }

        Assert.False(collectible.IsAlive);
    }

    private static CoflowSchemaRuntime RuntimeForSharedRecord(
        CoflowTypeId typeId,
        Func<SharedRecord, CoflowValueId> getValueId)
    {
        var builder = new CoflowSchemaRuntimeBuilder();
        builder.RegisterTypeCodec(typeId, 1, 0, 0,
            getValueId,
            static _ => true,
            static (value, _) => value,
            static (_, value) => value,
            static (ref CoflowValueWriter writer, SharedRecord value) =>
                writer.WriteValueId(default));
        return builder.Build();
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference CreateCollectibleAdapter(string adapter)
    {
        var context = new IsolatedLoadContext();
        var assembly = context.LoadFromAssemblyPath(typeof(CoflowLayoutRegistryTests).Assembly.Location);
        var recordType = assembly.GetType(
            typeof(SharedRecord).FullName!, throwOnError: true)!;

        var builder = new CoflowSchemaRuntimeBuilder();
        typeof(CoflowSchemaRuntimeBuilder)
            .GetMethod(nameof(CoflowSchemaRuntimeBuilder.RegisterType))!
            .MakeGenericMethod(recordType)
            .Invoke(builder, new object[] { new CoflowTypeId(1) });
        var runtime = builder.Build();

        // 构造一次闭合 boundary adapter，覆盖最容易意外固定 collectible Type 的缓存路径。
        if (adapter == "boundary")
        {
            using (CoflowSchemaRuntimeContext.Enter(runtime))
            {
                typeof(CoflowSchemaRuntime)
                    .GetMethod("BoundaryWrite", BindingFlags.Instance | BindingFlags.NonPublic)!
                    .MakeGenericMethod(recordType)
                    .Invoke(runtime, new object[] { false });
            }
        }
        else if (adapter == "union")
            _ = CoflowUnionAccessors.For(
                typeof(Option<>).MakeGenericType(recordType), CoflowValueShapeKind.Option);
        else if (adapter == "dictionary")
            _ = CoflowDictionaryEntryAccessors.For(recordType, typeof(string));
        else if (adapter == "receiver")
        {
            var entry = new CoflowFunctionEntry(
                new CoflowFunctionIdentity("Record", "", "call"),
                new CoflowFunctionSignature(typeof(Unit), Array.Empty<Type>()),
                recordType,
                null,
                string.Empty,
                null);
            _ = CoflowNativeCallFactory.BindFunction(entry, recordType);
        }
        else throw new ArgumentOutOfRangeException(nameof(adapter));

        var weak = new WeakReference(context, trackResurrection: false);
        context.Unload();
        return weak;
    }

    private sealed class IsolatedLoadContext() : AssemblyLoadContext(isCollectible: true)
    {
        protected override Assembly? Load(AssemblyName assemblyName) =>
            AppDomain.CurrentDomain.GetAssemblies().FirstOrDefault(
                assembly => AssemblyName.ReferenceMatchesDefinition(
                    assembly.GetName(), assemblyName));
    }

    private sealed class SharedRecord
    {
        internal CoflowValueId First { get; init; }
        internal CoflowValueId Second { get; init; }
    }
    private readonly record struct SharedStruct(long Value);
}
