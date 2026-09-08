using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

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

    internal static TDelegate CompileCollectibleSafe<TDelegate>(
        Expression<TDelegate> expression,
        params Type[] referencedTypes)
        where TDelegate : Delegate
    {
        if (!DynamicCodeSupported || referencedTypes.Any(IsCollectible))
        {
            _interpretedCompilationCount++;
            return expression.Compile(preferInterpretation: true);
        }
        return expression.Compile();
    }

    internal static bool IsCollectible(Type type)
    {
#if NET5_0_OR_GREATER
        return type.Assembly.IsCollectible;
#else
        // netstandard2.1 不公开可卸载程序集 API；支持该能力的现代 Host 使用具体目标框架路径。
        return false;
#endif
    }

    internal static IDisposable OverrideDynamicCodeSupportForCurrentThread(bool supported)
    {
        var previous = _dynamicCodeOverride;
        _dynamicCodeOverride = supported;
        return new RestoreDynamicCodeSupport(previous);
    }

    private sealed class RestoreDynamicCodeSupport : IDisposable
    {
        private readonly bool? previous;

        public RestoreDynamicCodeSupport(bool? previous)
        {
            this.previous = previous;
        }

        private bool _disposed;

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _dynamicCodeOverride = previous;
        }
    }
}
}
