namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public readonly record struct CoflowHostFunctionBinding
{
    public CoflowHostFunctionBinding(CoflowFunctionEntry entry, Delegate implementation)
        : this(entry, implementation, null) { }

    private CoflowHostFunctionBinding(CoflowFunctionEntry entry, Delegate implementation, CoflowNativeCall? call)
    {
        Entry = entry ?? throw new ArgumentNullException(nameof(entry));
        Implementation = implementation ?? throw new ArgumentNullException(nameof(implementation));
        Call = call;
    }

    public CoflowFunctionEntry Entry { get; }
    public Delegate Implementation { get; }
    internal CoflowNativeCall? Call { get; }

    public static CoflowHostFunctionBinding Create<TResult>(CoflowFunctionEntry entry, Func<TResult> implementation) =>
        new(entry, implementation, new(Type.EmptyTypes, typeof(TResult), frame => frame.WriteImported(implementation())));
    public static CoflowHostFunctionBinding Create<T1, TResult>(CoflowFunctionEntry entry, Func<T1, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0)))));
    public static CoflowHostFunctionBinding Create<T1, T2, TResult>(CoflowFunctionEntry entry, Func<T1, T2, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, T6, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, T6, T7, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6)))));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowFunctionEntry entry, Func<T1, T2, T3, T4, T5, T6, T7, T8, TResult> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7), typeof(T8) }, typeof(TResult), frame => frame.WriteImported(implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6), frame.Read<T8>(7)))));

    public static CoflowHostFunctionBinding Create(CoflowFunctionEntry entry, Action implementation) =>
        new(entry, implementation, new(Type.EmptyTypes, typeof(Unit), frame => { implementation(); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1>(CoflowFunctionEntry entry, Action<T1> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2>(CoflowFunctionEntry entry, Action<T1, T2> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3>(CoflowFunctionEntry entry, Action<T1, T2, T3> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5, T6> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5, T6, T7> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6)); frame.WriteImported(Unit.Value); }));
    public static CoflowHostFunctionBinding Create<T1, T2, T3, T4, T5, T6, T7, T8>(CoflowFunctionEntry entry, Action<T1, T2, T3, T4, T5, T6, T7, T8> implementation) =>
        new(entry, implementation, new(new[] { typeof(T1), typeof(T2), typeof(T3), typeof(T4), typeof(T5), typeof(T6), typeof(T7), typeof(T8) }, typeof(Unit), frame => { implementation(frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2), frame.Read<T4>(3), frame.Read<T5>(4), frame.Read<T6>(5), frame.Read<T7>(6), frame.Read<T8>(7)); frame.WriteImported(Unit.Value); }));
}
