namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowInvoker
{
    public static TResult Invoke<TReceiver, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<TResult>();
    }

    public static TResult Invoke<TReceiver, T1, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, TResult>(arg1);
    }

    public static TResult Invoke<TReceiver, T1, T2, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, TResult>(arg1, arg2);
    }

    public static TResult Invoke<TReceiver, T1, T2, T3, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, T3, TResult>(arg1, arg2, arg3);
    }

    public static TResult Invoke<TReceiver, T1, T2, T3, T4, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, T3, T4, TResult>(arg1, arg2, arg3, arg4);
    }

    public static TResult Invoke<TReceiver, T1, T2, T3, T4, T5, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, T3, T4, T5, TResult>(arg1, arg2, arg3, arg4, arg5);
    }

    public static TResult Invoke<TReceiver, T1, T2, T3, T4, T5, T6, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, T3, T4, T5, T6, TResult>(arg1, arg2, arg3, arg4, arg5, arg6);
    }

    public static TResult Invoke<TReceiver, T1, T2, T3, T4, T5, T6, T7, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    public static TResult Invoke<TReceiver, T1, T2, T3, T4, T5, T6, T7, T8, TResult>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        using var scope = Enter(coflow);
        return coflow.Snapshot.Function(id, typeId, fieldId).Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);
    }

    public static void InvokeVoid<TReceiver>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid();
    }

    public static void InvokeVoid<TReceiver, T1>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1);
    }

    public static void InvokeVoid<TReceiver, T1, T2>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2);
    }

    public static void InvokeVoid<TReceiver, T1, T2, T3>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2, arg3);
    }

    public static void InvokeVoid<TReceiver, T1, T2, T3, T4>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2, arg3, arg4);
    }

    public static void InvokeVoid<TReceiver, T1, T2, T3, T4, T5>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2, arg3, arg4, arg5);
    }

    public static void InvokeVoid<TReceiver, T1, T2, T3, T4, T5, T6>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6);
    }

    public static void InvokeVoid<TReceiver, T1, T2, T3, T4, T5, T6, T7>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6, arg7);
    }

    public static void InvokeVoid<TReceiver, T1, T2, T3, T4, T5, T6, T7, T8>(global::Coflow.Runtime.Coflow coflow, TReceiver receiver, CoflowValueId id, CoflowTypeId typeId, CoflowFieldId fieldId, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        using var scope = Enter(coflow);
        coflow.Snapshot.Function(id, typeId, fieldId).InvokeVoid(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8);
    }

    private static global::Coflow.Runtime.Coflow.ExecutionScope Enter(global::Coflow.Runtime.Coflow coflow)
    {
        if (coflow is null) throw new ArgumentNullException(nameof(coflow));
        return coflow.EnterExecution();
    }
}
