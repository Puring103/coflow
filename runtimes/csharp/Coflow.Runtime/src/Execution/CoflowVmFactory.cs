namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public delegate void CoflowVmFactoryInvoker(ref CoflowVmFactoryFrame frame);

/// <summary>生成代码使用的 VM 工厂帧；按声明索引读取参数并写入唯一结果。</summary>
[EditorBrowsable(EditorBrowsableState.Never)]
public struct CoflowVmFactoryFrame
{
    private CoflowNativeFrame _frame;

    internal CoflowVmFactoryFrame(CoflowNativeFrame frame) => _frame = frame;

    public T Read<T>(int index) => _frame.Read<T>(index);

    public void Write<T>(T value) => _frame.Write(value);
}

/// <summary>保存生成期已知的 VM 工厂签名和直接调用适配器。</summary>
[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowVmFactory
{
    public CoflowVmFactory(
        Type[] parameterTypes,
        Type resultType,
        CoflowVmFactoryInvoker invoke)
    {
        if (parameterTypes is null) throw new ArgumentNullException(nameof(parameterTypes));
        if (resultType is null) throw new ArgumentNullException(nameof(resultType));
        if (invoke is null) throw new ArgumentNullException(nameof(invoke));
        Call = new CoflowNativeCall(parameterTypes, resultType, nativeFrame =>
        {
            var frame = new CoflowVmFactoryFrame(nativeFrame);
            invoke(ref frame);
        });
    }

    internal CoflowNativeCall Call { get; }
}
