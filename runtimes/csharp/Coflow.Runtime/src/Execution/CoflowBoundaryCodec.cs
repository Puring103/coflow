using System;
using System.Linq.Expressions;
using System.Reflection;

namespace Coflow.Runtime.CompilerServices;

internal static class CoflowBoundaryCodec<T>
{
    internal static readonly Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> Write = CoflowBoundaryCodec.BuildWrite<T>();

    internal static readonly Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> WriteImported = CoflowBoundaryCodec.BuildImportingWrite<T>();

    internal static readonly Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> Read = CoflowBoundaryCodec.BuildRead<T>();

    internal static readonly Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> WriteRelative = CoflowBoundaryCodec.BuildWrite<T>(relative: true);

    internal static readonly Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> WriteImportedRelative = CoflowBoundaryCodec.BuildImportingWrite<T>(relative: true);

    internal static readonly Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> ReadRelative = CoflowBoundaryCodec.BuildRead<T>(relative: true);
}
internal static class CoflowBoundaryCodec
{
    internal static Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> BuildImportingWrite<T>(bool relative = false)
    {
        Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> write = BuildWrite<T>(relative);
        return delegate (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value)
        {
            write(context, register, CoflowInvocationContext.Import(value, context.Collections));
        };
    }

    internal static Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> BuildWrite<T>(bool relative = false)
    {
        if (CoflowValueShape.Of(typeof(T)).Kind == CoflowValueShapeKind.Collection)
        {
            return delegate (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value)
            {
                WriteCollection(context, register, value, relative);
            };
        }
        if (CoflowStructCodecs.TryGet(typeof(T), out CoflowStructDescriptor _))
        {
            CoflowStructDescriptor<T> descriptor2 = CoflowStructCodecs.Get<T>();
            return delegate (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value)
            {
                descriptor2.Write(context, relative ? register with
                {
                    IntegerBase = register.IntegerBase + context.IntegerBase,
                    FloatBase = register.FloatBase + context.FloatBase,
                    ReferenceBase = register.ReferenceBase + context.ReferenceBase
                } : register, value);
            };
        }
        if (CoflowTypes.TryGet(typeof(T), out var _) && !typeof(T).IsValueType)
        {
            return delegate (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value)
            {
                if (value == null || !CoflowTypeCodecs.TryGet(value.GetType(), out CoflowTypeDescriptor descriptor3))
                {
                    throw new CoflowBoundaryException($"A schema `{typeof(T)}` value has no concrete codec.");
                }
                context.WriteInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar, (long)descriptor3.GetValueIdObject(value).Packed);
            };
        }
        ParameterExpression parameterExpression = Expression.Parameter(typeof(CoflowVm.CoflowExecutionContext), "context");
        ParameterExpression parameterExpression2 = Expression.Parameter(typeof(CoflowValueRegister), "register");
        ParameterExpression parameterExpression3 = Expression.Parameter(typeof(T), "value");
        return CoflowExpressionCompiler.Compile(Expression.Lambda<Action<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T>>(WriteExpression(typeof(T), parameterExpression, parameterExpression2, parameterExpression3, relative), new ParameterExpression[3] { parameterExpression, parameterExpression2, parameterExpression3 }));
    }

    internal static Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T> BuildRead<T>(bool relative = false)
    {
        if (CoflowValueShape.Of(typeof(T)).Kind == CoflowValueShapeKind.Collection)
        {
            return (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register) => ReadCollection<T>(context, register, relative);
        }
        if (CoflowStructCodecs.TryGet(typeof(T), out CoflowStructDescriptor _))
        {
            CoflowStructDescriptor<T> descriptor2 = CoflowStructCodecs.Get<T>();
            return (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register) => descriptor2.Read(context, relative ? register with
            {
                IntegerBase = register.IntegerBase + context.IntegerBase,
                FloatBase = register.FloatBase + context.FloatBase,
                ReferenceBase = register.ReferenceBase + context.ReferenceBase
            } : register);
        }
        if (CoflowTypes.TryGet(typeof(T), out var _) && !typeof(T).IsValueType)
        {
            return (CoflowVm.CoflowExecutionContext context, CoflowValueRegister register) => (T)CoflowInvocationContext.ApiValue(CoflowValueId.FromPacked((ulong)context.ReadInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar)), typeof(T));
        }
        ParameterExpression parameterExpression = Expression.Parameter(typeof(CoflowVm.CoflowExecutionContext), "context");
        ParameterExpression parameterExpression2 = Expression.Parameter(typeof(CoflowValueRegister), "register");
        return CoflowExpressionCompiler.Compile(Expression.Lambda<Func<CoflowVm.CoflowExecutionContext, CoflowValueRegister, T>>(ReadExpression(typeof(T), parameterExpression, parameterExpression2, relative), new ParameterExpression[2] { parameterExpression, parameterExpression2 }));
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
                CoflowRegisterKind.Integer => Expression.Call(context, relative ? "WriteIntegerRelative" : "WriteInteger", Type.EmptyTypes, expression, (type == typeof(bool)) ? ((Expression)Expression.Condition(value, Expression.Constant(1L), Expression.Constant(0L))) : ((Expression)Expression.Convert(value, typeof(long)))),
                CoflowRegisterKind.Float => Expression.Call(context, relative ? "WriteFloatRelative" : "WriteFloat", Type.EmptyTypes, expression, value),
                _ => Expression.Call(context, relative ? "WriteReferenceRelative" : "WriteReference", Type.EmptyTypes, expression, Expression.Convert(value, typeof(object))),
            };
            return result;
        }
        Expression expression2 = Register(register, CoflowRegisterKind.Integer, relative, tag: true);
        MemberExpression register2 = Expression.Property(register, "First");
        if (coflowValueShape.Kind == CoflowValueShapeKind.Option)
        {
            MemberExpression test = Expression.Property(value, "HasValue");
            return Expression.Block(Expression.Call(context, relative ? "WriteIntegerRelative" : "WriteInteger", Type.EmptyTypes, expression2, Expression.Condition(test, Expression.Constant(1L), Expression.Constant(0L))), Expression.IfThen(test, WriteExpression(coflowValueShape.First!.Type, context, register2, Expression.Property(value, "Value"), relative)));
        }
        MemberExpression test2 = Expression.Property(value, "IsOk");
        MemberExpression register3 = Expression.Property(register, "Second");
        return Expression.Block(Expression.Call(context, relative ? "WriteIntegerRelative" : "WriteInteger", Type.EmptyTypes, expression2, Expression.Condition(test2, Expression.Constant(1L), Expression.Constant(0L))), Expression.IfThenElse(test2, WriteExpression(coflowValueShape.First!.Type, context, register2, Expression.Property(value, "Value"), relative), WriteExpression(coflowValueShape.Second!.Type, context, register3, Expression.Property(value, "Error"), relative)));
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
                CoflowRegisterKind.Integer => Expression.Call(context, relative ? "ReadIntegerRelative" : "ReadInteger", Type.EmptyTypes, expression),
                CoflowRegisterKind.Float => Expression.Call(context, relative ? "ReadFloatRelative" : "ReadFloat", Type.EmptyTypes, expression),
                _ => Expression.Call(context, relative ? "ReadReferenceRelative" : "ReadReference", Type.EmptyTypes, expression),
            };
            MethodCallExpression methodCallExpression2 = methodCallExpression;
            if (type == typeof(bool))
            {
                return Expression.NotEqual(methodCallExpression2, Expression.Constant(0L));
            }
            return Expression.Convert(methodCallExpression2, type);
        }
        Expression expression2 = Register(register, CoflowRegisterKind.Integer, relative, tag: true);
        BinaryExpression test = Expression.NotEqual(Expression.Call(context, relative ? "ReadIntegerRelative" : "ReadInteger", Type.EmptyTypes, expression2), Expression.Constant(0L));
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

    internal static void WriteStruct<T>(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value, bool relative)
    {
        CoflowStructDescriptor<T> coflowStructDescriptor = CoflowStructCodecs.Get<T>();
        coflowStructDescriptor.Write(context, relative ? register with
        {
            IntegerBase = register.IntegerBase + context.IntegerBase,
            FloatBase = register.FloatBase + context.FloatBase,
            ReferenceBase = register.ReferenceBase + context.ReferenceBase
        } : register, value);
    }

    internal static void WriteFunction<T>(CoflowVm.CoflowExecutionContext context,
        CoflowValueRegister register, T value, bool relative)
    {
        var integerBase = relative ? register.IntegerBase + context.IntegerBase : register.IntegerBase;
        context.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, integerBase),
            CoflowFunctionAccess<T>.FunctionId(value).Packed);
        context.WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, integerBase + 1),
            unchecked((long)CoflowFunctionAccess<T>.EnvironmentId(value).Packed));
    }

    internal static T ReadFunction<T>(CoflowVm.CoflowExecutionContext context,
        CoflowValueRegister register, bool relative)
    {
        var integerBase = relative ? register.IntegerBase + context.IntegerBase : register.IntegerBase;
        var functionId = CoflowFunctionId.FromPacked(unchecked((ulong)context.ReadInteger(
            new CoflowRegister(CoflowRegisterKind.Integer, integerBase))));
        var environmentId = CoflowValueId.FromPacked(unchecked((ulong)context.ReadInteger(
            new CoflowRegister(CoflowRegisterKind.Integer, integerBase + 1))));
        return CoflowFunctionHandle.Create<T>(functionId, environmentId);
    }

    internal static T ReadStruct<T>(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, bool relative)
    {
        CoflowStructDescriptor<T> coflowStructDescriptor = CoflowStructCodecs.Get<T>();
        return coflowStructDescriptor.Read(context, relative ? register with
        {
            IntegerBase = register.IntegerBase + context.IntegerBase,
            FloatBase = register.FloatBase + context.FloatBase,
            ReferenceBase = register.ReferenceBase + context.ReferenceBase
        } : register);
    }

    internal static void WriteCollection<T>(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, T value, bool relative)
    {
        CoflowEncodedValue coflowEncodedValue = CoflowCollectionEncoding.Encode(typeof(T), value, context.Collections);
        context.WriteInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar, coflowEncodedValue.Integers[0]);
    }

    internal static T ReadCollection<T>(CoflowVm.CoflowExecutionContext context, CoflowValueRegister register, bool relative)
    {
        long value = context.ReadInteger(relative ? new CoflowRegister(CoflowRegisterKind.Integer, register.IntegerBase + context.IntegerBase) : register.Scalar);
        return CoflowCollectionMaterializer<T>.Read(context, CoflowCollectionId.FromPacked((ulong)value));
    }
}
