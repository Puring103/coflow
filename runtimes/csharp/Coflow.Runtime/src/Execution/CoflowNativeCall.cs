using System;
using System.Linq;
using System.Linq.Expressions;
using System.Reflection;

namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowNativeCall
{
    internal int ArgumentCount => ParameterTypes.Length;

    internal Type[] ParameterTypes { get; }

    internal Type ResultType { get; }

    internal CoflowNativeInvoker Invoke { get; }

    internal CoflowNativeCall(Delegate implementation)
    {
        if ((object)implementation == null)
        {
            throw new ArgumentNullException("implementation");
        }
        var method = implementation.GetType().GetMethod("Invoke")!;
        ParameterTypes = (from parameter in method.GetParameters()
                          select parameter.ParameterType).ToArray();
        ResultType = ((method.ReturnType == typeof(void)) ? typeof(Unit) : method.ReturnType);
        Invoke = Build(implementation, method);
    }

    internal CoflowNativeCall(Type[] parameterTypes, Type resultType, CoflowNativeInvoker invoke)
    {
        ParameterTypes = parameterTypes ?? throw new ArgumentNullException("parameterTypes");
        ResultType = resultType ?? throw new ArgumentNullException("resultType");
        Invoke = invoke ?? throw new ArgumentNullException("invoke");
    }

    internal static CoflowNativeCall Create<TRecord, TValue>(Func<TRecord, TValue> implementation)
    {
        if (implementation == null)
        {
            throw new ArgumentNullException("implementation");
        }
        return new CoflowNativeCall(new Type[1] { typeof(TRecord) }, typeof(TValue), delegate (CoflowNativeFrame frame)
        {
            frame.Write(implementation(frame.Read<TRecord>(0)));
        });
    }

    private static CoflowNativeInvoker Build(Delegate implementation, MethodInfo invoke)
    {
        ParameterExpression frame = Expression.Parameter(typeof(CoflowNativeFrame), "frame");
        MethodCallExpression[] array = invoke.GetParameters().Select((ParameterInfo parameter, int index) => Expression.Call(frame, typeof(CoflowNativeFrame).GetMethod("Read")!.MakeGenericMethod(parameter.ParameterType), Expression.Constant(index))).ToArray();
        ConstantExpression expression = Expression.Constant(implementation);
        Expression[] arguments = array;
        InvocationExpression invocationExpression = Expression.Invoke(expression, arguments);
        Expression body = invoke.ReturnType == typeof(void)
            ? Expression.Block(invocationExpression, Expression.Call(frame,
                typeof(CoflowNativeFrame).GetMethod("Write")!.MakeGenericMethod(typeof(Unit)),
                Expression.Property(null, typeof(Unit), "Value")))
            : Expression.Call(frame, typeof(CoflowNativeFrame).GetMethod("Write")!
                .MakeGenericMethod(invoke.ReturnType), invocationExpression);
        return CoflowExpressionCompiler.Compile(Expression.Lambda<CoflowNativeInvoker>(body, new ParameterExpression[1] { frame }));
    }
}
