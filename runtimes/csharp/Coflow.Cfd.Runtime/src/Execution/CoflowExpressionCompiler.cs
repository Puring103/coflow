namespace CoflowRuntime.Generated;

using System.Linq.Expressions;

internal static class CoflowExpressionCompiler
{
    [ThreadStatic]
    private static bool? _dynamicCodeOverride;
    [ThreadStatic]
    private static int _interpretedCompilationCount;

    internal static bool DynamicCodeSupported => _dynamicCodeOverride ??
        System.Runtime.CompilerServices.RuntimeFeature.IsDynamicCodeSupported;
    internal static int InterpretedCompilationCount => _interpretedCompilationCount;

    internal static TDelegate Compile<TDelegate>(Expression<TDelegate> expression)
        where TDelegate : Delegate
    {
        if (DynamicCodeSupported) return expression.Compile();
        _interpretedCompilationCount++;
        return expression.Compile(preferInterpretation: true);
    }

    internal static Delegate Compile(LambdaExpression expression)
    {
        if (DynamicCodeSupported) return expression.Compile();
        _interpretedCompilationCount++;
        return expression.Compile(preferInterpretation: true);
    }

    internal static IDisposable OverrideDynamicCodeSupportForCurrentThread(bool supported)
    {
        var previous = _dynamicCodeOverride;
        _dynamicCodeOverride = supported;
        return new RestoreDynamicCodeSupport(previous);
    }

    private sealed class RestoreDynamicCodeSupport(bool? previous) : IDisposable
    {
        private bool _disposed;

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _dynamicCodeOverride = previous;
        }
    }
}
