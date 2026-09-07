using System;
using System.Linq.Expressions;
using System.Reflection;

namespace Coflow.Runtime.CompilerServices;

internal static class CoflowBoundaryCodec<T>
{
    internal static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> Write =
        static (context, register, value) => CoflowBoundaryCodec.Write(context, register, value, false);

    internal static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> WriteImported =
        static (context, register, value) => CoflowBoundaryCodec.WriteImported(context, register, value, false);

    internal static readonly Func<CoflowExecutionSession, CoflowValueRegister, T> Read =
        static (context, register) => CoflowBoundaryCodec.Read<T>(context, register, false);

    internal static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> WriteRelative =
        static (context, register, value) => CoflowBoundaryCodec.Write(context, register, value, true);

    internal static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> WriteImportedRelative =
        static (context, register, value) => CoflowBoundaryCodec.WriteImported(context, register, value, true);

    internal static readonly Func<CoflowExecutionSession, CoflowValueRegister, T> ReadRelative =
        static (context, register) => CoflowBoundaryCodec.Read<T>(context, register, true);
}
internal static class CoflowBoundaryCodec
{
    internal static void Write<T>(CoflowExecutionSession context, CoflowValueRegister register,
        T value, bool relative)
    {
        if (context.Runtime is { } runtime)
            runtime.BoundaryWrite<T>(relative)(context, register, value);
        else
            SchemaFreeBoundaryCodec<T>.Write(relative)(context, register, value);
    }

    internal static void WriteImported<T>(CoflowExecutionSession context, CoflowValueRegister register,
        T value, bool relative)
    {
        if (context.Runtime is { } runtime)
            runtime.BoundaryImportedWrite<T>(relative)(context, register, value);
        else
            SchemaFreeBoundaryCodec<T>.WriteImported(relative)(context, register, value);
    }

    internal static T Read<T>(CoflowExecutionSession context, CoflowValueRegister register, bool relative)
    {
        if (context.Runtime is { } runtime)
            return runtime.BoundaryRead<T>(relative)(context, register);
        return SchemaFreeBoundaryCodec<T>.Read(relative)(context, register);
    }

    internal static Action<CoflowExecutionSession, CoflowValueRegister, T> BuildImportingWrite<T>(bool relative = false)
    {
        Action<CoflowExecutionSession, CoflowValueRegister, T> write = BuildWrite<T>(relative);
        return delegate (CoflowExecutionSession context, CoflowValueRegister register, T value)
        {
            write(context, register, context.ImportBoundary(value));
        };
    }

    internal static Action<CoflowExecutionSession, CoflowValueRegister, T> BuildWrite<T>(bool relative = false)
    {
        if (CoflowValueShape.Of(typeof(T)).Kind == CoflowValueShapeKind.Collection)
        {
            return delegate (CoflowExecutionSession context, CoflowValueRegister register, T value)
            {
                WriteCollection(context, register, value, relative);
            };
        }
        if (CoflowSchemaRuntimeContext.TryGetStructCodec(typeof(T), out CoflowStructDescriptor _))
        {
            CoflowStructDescriptor<T> descriptor2 = CoflowSchemaRuntimeContext.GetStructCodec<T>();
            return delegate (CoflowExecutionSession context, CoflowValueRegister register, T value)
            {
                descriptor2.Write(context, relative ? register with
                {
                    IntegerBase = register.IntegerBase + context.IntegerBase,
                    FloatBase = register.FloatBase + context.FloatBase,
                    ReferenceBase = register.ReferenceBase + context.ReferenceBase
                } : register, value);
            };
        }
        if (CoflowSchemaRuntimeContext.TryGetType(typeof(T), out var _) && !typeof(T).IsValueType)
        {
            return delegate (CoflowExecutionSession context, CoflowValueRegister register, T value)
            {
                if (value == null || !CoflowSchemaRuntimeContext.TryGetTypeCodec(value.GetType(), out CoflowTypeDescriptor descriptor3))
                {
                    throw new CoflowBoundaryException($"A schema `{typeof(T)}` value has no concrete codec.");
                }
                context.Registers.WriteInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar, (long)descriptor3.GetValueIdObject(value).Packed);
            };
        }
        ParameterExpression parameterExpression = Expression.Parameter(typeof(CoflowExecutionSession), "context");
        ParameterExpression parameterExpression2 = Expression.Parameter(typeof(CoflowValueRegister), "register");
        ParameterExpression parameterExpression3 = Expression.Parameter(typeof(T), "value");
        return CoflowExpressionCompiler.Compile(Expression.Lambda<Action<CoflowExecutionSession, CoflowValueRegister, T>>(WriteExpression(typeof(T), parameterExpression, parameterExpression2, parameterExpression3, relative), new ParameterExpression[3] { parameterExpression, parameterExpression2, parameterExpression3 }));
    }

    internal static Func<CoflowExecutionSession, CoflowValueRegister, T> BuildRead<T>(bool relative = false)
    {
        if (CoflowValueShape.Of(typeof(T)).Kind == CoflowValueShapeKind.Collection)
        {
            return (CoflowExecutionSession context, CoflowValueRegister register) => ReadCollection<T>(context, register, relative);
        }
        if (CoflowSchemaRuntimeContext.TryGetStructCodec(typeof(T), out CoflowStructDescriptor _))
        {
            CoflowStructDescriptor<T> descriptor2 = CoflowSchemaRuntimeContext.GetStructCodec<T>();
            return (CoflowExecutionSession context, CoflowValueRegister register) => descriptor2.Read(context, relative ? register with
            {
                IntegerBase = register.IntegerBase + context.IntegerBase,
                FloatBase = register.FloatBase + context.FloatBase,
                ReferenceBase = register.ReferenceBase + context.ReferenceBase
            } : register);
        }
        if (CoflowSchemaRuntimeContext.TryGetType(typeof(T), out var _) && !typeof(T).IsValueType)
        {
            return (CoflowExecutionSession context, CoflowValueRegister register) => (T)context.ApiValue(CoflowValueId.FromPacked((ulong)context.Registers.ReadInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar)), typeof(T));
        }
        ParameterExpression parameterExpression = Expression.Parameter(typeof(CoflowExecutionSession), "context");
        ParameterExpression parameterExpression2 = Expression.Parameter(typeof(CoflowValueRegister), "register");
        return CoflowExpressionCompiler.Compile(Expression.Lambda<Func<CoflowExecutionSession, CoflowValueRegister, T>>(ReadExpression(typeof(T), parameterExpression, parameterExpression2, relative), new ParameterExpression[2] { parameterExpression, parameterExpression2 }));
    }

    private static Expression WriteExpression(Type type, Expression context, Expression register, Expression value, bool relative)
    {
        CoflowValueShape coflowValueShape = CoflowValueShape.Of(type);
        if (coflowValueShape.Kind == CoflowValueShapeKind.Unit)
        {
            return Expression.Empty();
        }
        if (coflowValueShape.Kind == CoflowValueShapeKind.Struct)
        {
            return Expression.Call(typeof(CoflowBoundaryCodec), "WriteStruct", new Type[1] { type }, context, register, value, Expression.Constant(relative));
        }
        if (coflowValueShape.Kind == CoflowValueShapeKind.Function)
        {
            return Expression.Call(typeof(CoflowBoundaryCodec), nameof(WriteFunction),
                new Type[1] { type }, context, register, value, Expression.Constant(relative));
        }
        if (coflowValueShape.Kind == CoflowValueShapeKind.Scalar)
        {
            Expression expression = Register(register, coflowValueShape.ScalarKind!.Value, relative);
            MethodCallExpression result = coflowValueShape.ScalarKind switch
            {
                CoflowRegisterKind.Integer => Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "WriteIntegerRelative" : "WriteInteger", Type.EmptyTypes, expression, (type == typeof(bool)) ? ((Expression)Expression.Condition(value, Expression.Constant(1L), Expression.Constant(0L))) : ((Expression)Expression.Convert(value, typeof(long)))),
                CoflowRegisterKind.Float => Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "WriteFloatRelative" : "WriteFloat", Type.EmptyTypes, expression, value),
                _ => Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "WriteReferenceRelative" : "WriteReference", Type.EmptyTypes, expression, Expression.Convert(value, typeof(object))),
            };
            return result;
        }
        Expression expression2 = Register(register, CoflowRegisterKind.Integer, relative, tag: true);
        MemberExpression register2 = Expression.Property(register, "First");
        if (coflowValueShape.Kind == CoflowValueShapeKind.Option)
        {
            MemberExpression test = Expression.Property(value, "HasValue");
            return Expression.Block(Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "WriteIntegerRelative" : "WriteInteger", Type.EmptyTypes, expression2, Expression.Condition(test, Expression.Constant(1L), Expression.Constant(0L))), Expression.IfThen(test, WriteExpression(coflowValueShape.First!.Type, context, register2, Expression.Property(value, "Value"), relative)));
        }
        MemberExpression test2 = Expression.Property(value, "IsOk");
        MemberExpression register3 = Expression.Property(register, "Second");
        return Expression.Block(Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "WriteIntegerRelative" : "WriteInteger", Type.EmptyTypes, expression2, Expression.Condition(test2, Expression.Constant(1L), Expression.Constant(0L))), Expression.IfThenElse(test2, WriteExpression(coflowValueShape.First!.Type, context, register2, Expression.Property(value, "Value"), relative), WriteExpression(coflowValueShape.Second!.Type, context, register3, Expression.Property(value, "Error"), relative)));
    }

    private static Expression ReadExpression(Type type, Expression context, Expression register, bool relative)
    {
        CoflowValueShape coflowValueShape = CoflowValueShape.Of(type);
        if (coflowValueShape.Kind == CoflowValueShapeKind.Unit)
        {
            return Expression.Property(null, typeof(Unit), "Value");
        }
        if (coflowValueShape.Kind == CoflowValueShapeKind.Struct)
        {
            return Expression.Call(typeof(CoflowBoundaryCodec), "ReadStruct", new Type[1] { type }, context, register, Expression.Constant(relative));
        }
        if (coflowValueShape.Kind == CoflowValueShapeKind.Function)
        {
            return Expression.Call(typeof(CoflowBoundaryCodec), nameof(ReadFunction),
                new Type[1] { type }, context, register, Expression.Constant(relative));
        }
        if (coflowValueShape.Kind == CoflowValueShapeKind.Scalar)
        {
            Expression expression = Register(register, coflowValueShape.ScalarKind!.Value, relative);
            MethodCallExpression methodCallExpression = coflowValueShape.ScalarKind switch
            {
                CoflowRegisterKind.Integer => Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "ReadIntegerRelative" : "ReadInteger", Type.EmptyTypes, expression),
                CoflowRegisterKind.Float => Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "ReadFloatRelative" : "ReadFloat", Type.EmptyTypes, expression),
                _ => Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "ReadReferenceRelative" : "ReadReference", Type.EmptyTypes, expression),
            };
            MethodCallExpression methodCallExpression2 = methodCallExpression;
            if (type == typeof(bool))
            {
                return Expression.NotEqual(methodCallExpression2, Expression.Constant(0L));
            }
            return Expression.Convert(methodCallExpression2, type);
        }
        Expression expression2 = Register(register, CoflowRegisterKind.Integer, relative, tag: true);
        BinaryExpression test = Expression.NotEqual(Expression.Call(Expression.Property(context, nameof(CoflowExecutionSession.Registers)), relative ? "ReadIntegerRelative" : "ReadInteger", Type.EmptyTypes, expression2), Expression.Constant(0L));
        MemberExpression register2 = Expression.Property(register, "First");
        if (coflowValueShape.Kind == CoflowValueShapeKind.Option)
        {
            MethodInfo method = type.GetMethod("Some", BindingFlags.Static | BindingFlags.Public)!;
            MemberExpression ifFalse = Expression.Property(null, type, "None");
            return Expression.Condition(test, Expression.Call(method, ReadExpression(coflowValueShape.First!.Type, context, register2, relative)), ifFalse);
        }
        MemberExpression register3 = Expression.Property(register, "Second");
        return Expression.Condition(test, Expression.Call(type.GetMethod("Ok")!, ReadExpression(coflowValueShape.First!.Type, context, register2, relative)), Expression.Call(type.GetMethod("Err")!, ReadExpression(coflowValueShape.Second!.Type, context, register3, relative)));
    }

    private static Expression Register(Expression register, CoflowRegisterKind kind, bool relative, bool tag = false)
    {
        if (!relative)
        {
            return Expression.Property(register, tag ? "Tag" : "Scalar");
        }
        var propertyName = kind switch
        {
            CoflowRegisterKind.Integer => "IntegerBase",
            CoflowRegisterKind.Float => "FloatBase",
            _ => "ReferenceBase",
        };
        return Expression.Property(register, propertyName);
    }

    internal static void WriteStruct<T>(CoflowExecutionSession context, CoflowValueRegister register, T value, bool relative)
    {
        CoflowStructDescriptor<T> coflowStructDescriptor = CoflowSchemaRuntimeContext.GetStructCodec<T>();
        coflowStructDescriptor.Write(context, relative ? register with
        {
            IntegerBase = register.IntegerBase + context.IntegerBase,
            FloatBase = register.FloatBase + context.FloatBase,
            ReferenceBase = register.ReferenceBase + context.ReferenceBase
        } : register, value);
    }

    internal static void WriteFunction<T>(CoflowExecutionSession context,
        CoflowValueRegister register, T value, bool relative)
    {
        var integerBase = relative ? register.IntegerBase + context.IntegerBase : register.IntegerBase;
        context.Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, integerBase),
            CoflowFunctionAccess<T>.FunctionId(value).Packed);
        context.Registers.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, integerBase + 1),
            unchecked((long)CoflowFunctionAccess<T>.EnvironmentId(value).Packed));
    }

    internal static T ReadFunction<T>(CoflowExecutionSession context,
        CoflowValueRegister register, bool relative)
    {
        var integerBase = relative ? register.IntegerBase + context.IntegerBase : register.IntegerBase;
        var functionId = CoflowFunctionId.FromPacked(unchecked((ulong)context.Registers.ReadInteger(
            new CoflowRegister(CoflowRegisterKind.Integer, integerBase))));
        var environmentId = CoflowValueId.FromPacked(unchecked((ulong)context.Registers.ReadInteger(
            new CoflowRegister(CoflowRegisterKind.Integer, integerBase + 1))));
        return CoflowFunctionHandle.Create<T>(functionId, environmentId);
    }

    internal static T ReadStruct<T>(CoflowExecutionSession context, CoflowValueRegister register, bool relative)
    {
        CoflowStructDescriptor<T> coflowStructDescriptor = CoflowSchemaRuntimeContext.GetStructCodec<T>();
        return coflowStructDescriptor.Read(context, relative ? register with
        {
            IntegerBase = register.IntegerBase + context.IntegerBase,
            FloatBase = register.FloatBase + context.FloatBase,
            ReferenceBase = register.ReferenceBase + context.ReferenceBase
        } : register);
    }

    internal static void WriteCollection<T>(CoflowExecutionSession context, CoflowValueRegister register, T value, bool relative)
    {
        CoflowEncodedValue coflowEncodedValue = CoflowCollectionEncoding.Encode(
            typeof(T), value, context.Collections, budgetAlreadyCharged: true);
        context.Registers.WriteInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar, coflowEncodedValue.Integers[0]);
    }

    internal static T ReadCollection<T>(CoflowExecutionSession context, CoflowValueRegister register, bool relative)
    {
        long value = context.Registers.ReadInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar);
        return CoflowCollectionMaterializer<T>.Read(context, CoflowCollectionId.FromPacked((ulong)value));
    }
}

/// <summary>只缓存不依赖 Schema descriptor 的标量 adapter。</summary>
internal static class SchemaFreeBoundaryCodec<T>
{
    private static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> AbsoluteWrite =
        CoflowBoundaryCodec.BuildWrite<T>();
    private static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> RelativeWrite =
        CoflowBoundaryCodec.BuildWrite<T>(true);
    private static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> AbsoluteImportedWrite =
        CoflowBoundaryCodec.BuildImportingWrite<T>();
    private static readonly Action<CoflowExecutionSession, CoflowValueRegister, T> RelativeImportedWrite =
        CoflowBoundaryCodec.BuildImportingWrite<T>(true);
    private static readonly Func<CoflowExecutionSession, CoflowValueRegister, T> AbsoluteRead =
        CoflowBoundaryCodec.BuildRead<T>();
    private static readonly Func<CoflowExecutionSession, CoflowValueRegister, T> RelativeRead =
        CoflowBoundaryCodec.BuildRead<T>(true);

    internal static Action<CoflowExecutionSession, CoflowValueRegister, T> Write(bool relative) =>
        relative ? RelativeWrite : AbsoluteWrite;
    internal static Action<CoflowExecutionSession, CoflowValueRegister, T> WriteImported(bool relative) =>
        relative ? RelativeImportedWrite : AbsoluteImportedWrite;
    internal static Func<CoflowExecutionSession, CoflowValueRegister, T> Read(bool relative) =>
        relative ? RelativeRead : AbsoluteRead;
}
