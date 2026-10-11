using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.Globalization;
using System.Linq.Expressions;
using System.Text;

internal static class CoflowFormatting
{
    private delegate void ValueRenderer(CoflowNativeFrame frame, int index, StringBuilder output);
    private static readonly System.Reflection.MethodInfo FrameReadMethod =
        typeof(CoflowNativeFrame).GetMethod(nameof(CoflowNativeFrame.Read))!;

    internal static CoflowNativeCall Interpolation(
        IReadOnlyList<string?> texts,
        IReadOnlyList<Type?> types,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<string, ICoflowEnumMetadata> enums)
    {
        var renderers = new ValueRenderer?[texts.Count];
        var argument = 0;
        for (var index = 0; index < texts.Count; index++)
        {
            if (types[index] is not { } type) continue;
            var formatter = CreateFormatter(type, metadata, enums);
            renderers[index] = CreateRenderer(type, formatter);
            argument++;
        }
        return new CoflowNativeCall(types.Where(type => type is not null).Select(type => type!).ToArray(),
            typeof(string), frame =>
        {
            var output = new StringBuilder();
            var valueIndex = 0;
            for (var index = 0; index < texts.Count; index++)
            {
                if (texts[index] is { } text) output.Append(text);
                else renderers[index]!(frame, valueIndex++, output);
            }
            frame.Write(output.ToString());
        });
    }

    internal static CoflowNativeCall RecordMetadata(
        Type receiverType,
        string name,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<string, ICoflowEnumMetadata> enums)
    {
        var formatters = metadata.Values.OfType<ICoflowRecordMetadata>()
            .ToDictionary(item => item.RuntimeType,
                item => CreateRecordMetadataFormatter(item, name, metadata, enums));
        return new CoflowNativeCall(new[] { receiverType }, typeof(string), frame =>
            frame.Write(FormatRecordMetadata(frame.ReadRecord(0, receiverType), formatters)));
    }

    private static string FormatRecordMetadata(
        object value,
        IReadOnlyDictionary<Type, Func<object, string>> formatters)
    {
        if (value is null) throw new InvalidOperationException("record metadata receiver is null");
        return formatters.TryGetValue(value.GetType(), out var format)
            ? format(value)
            : throw new InvalidOperationException(
                $"record type `{value.GetType()}` has no Coflow schema metadata");
    }

    private static Func<object, string> CreateRecordMetadataFormatter(
        ICoflowRecordMetadata record,
        string name,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<string, ICoflowEnumMetadata> enums)
    {
        var value = Expression.Parameter(typeof(object), "value");
        var key = Expression.Convert(Expression.Call(Expression.Constant(record),
            typeof(ICoflowRecordMetadata).GetMethod(nameof(ICoflowRecordMetadata.GetKey))!, value),
            record.KeyType);
        var rendered = Format(record.KeyType, key, Expression.Constant(false), metadata, enums);
        Expression result = name == "id" ? rendered : Expression.Call(
            typeof(CoflowFormatting), nameof(Concat2), Type.EmptyTypes,
            Expression.Constant(record.DeclaredType + "::"), rendered);
        return CoflowExpressionCompiler.Compile(
            Expression.Lambda<Func<object, string>>(result, value));
    }

    private static ValueRenderer CreateRenderer(Type type, Delegate formatter)
    {
        var frame = Expression.Parameter(typeof(CoflowNativeFrame), "frame");
        var index = Expression.Parameter(typeof(int), "index");
        var output = Expression.Parameter(typeof(StringBuilder), "output");
        var value = Expression.Call(frame, FrameReadMethod.MakeGenericMethod(type), index);
        var format = Expression.Invoke(
            Expression.Convert(Expression.Constant(formatter),
                Expression.GetFuncType(type, typeof(bool), typeof(string))),
            value,
            Expression.Constant(false));
        var append = Expression.Call(output,
            typeof(StringBuilder).GetMethod(nameof(StringBuilder.Append), new[] { typeof(string) })!,
            format);
        return CoflowExpressionCompiler.Compile(
            Expression.Lambda<ValueRenderer>(
                Expression.Block(append, Expression.Empty()), frame, index, output));
    }

    private static Delegate CreateFormatter(
        Type type,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<string, ICoflowEnumMetadata> enums)
    {
        var value = Expression.Parameter(type, "value");
        var nested = Expression.Parameter(typeof(bool), "nested");
        return CoflowExpressionCompiler.Compile(Expression.Lambda(
            Expression.GetFuncType(type, typeof(bool), typeof(string)),
            Format(type, value, nested, metadata, enums), value, nested));
    }

    private static Expression Format(
        Type type,
        Expression value,
        Expression nested,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<string, ICoflowEnumMetadata> enums)
    {
        if (type == typeof(string)) return Expression.Call(
            typeof(CoflowFormatting), nameof(FormatString), Type.EmptyTypes, value, nested);
        if (type == typeof(long)) return Expression.Call(value,
            typeof(long).GetMethod(nameof(long.ToString), new[] { typeof(IFormatProvider) })!,
            Expression.Constant(CultureInfo.InvariantCulture));
        if (type == typeof(double)) return Expression.Call(value,
            typeof(double).GetMethod(nameof(double.ToString), new[] { typeof(string), typeof(IFormatProvider) })!,
            Expression.Constant("R"), Expression.Constant(CultureInfo.InvariantCulture));
        if (type == typeof(bool)) return Expression.Condition(value,
            Expression.Constant("true"), Expression.Constant("false"));
        if (type == typeof(Unit)) return Expression.Constant("()");
        if (type.IsEnum)
        {
            var enumMetadata = enums.Values.Single(item => item.RuntimeType == type);
            var names = enumMetadata.Variants.ToDictionary(
                item => Convert.ToInt64(item.Value, CultureInfo.InvariantCulture), item => item.Key);
            return Expression.Call(typeof(CoflowFormatting), nameof(FormatEnum), Type.EmptyTypes,
                Expression.Convert(value, typeof(long)), Expression.Constant(names));
        }
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                var active = Expression.Property(value, nameof(Option<int>.HasValue));
                return Expression.Condition(active,
                    Expression.Call(typeof(CoflowFormatting), nameof(Concat3), Type.EmptyTypes,
                        Expression.Constant("Some("),
                        Format(arguments[0], Expression.Property(value, nameof(Option<int>.Value)),
                            Expression.Constant(true), metadata, enums), Expression.Constant(")")),
                    Expression.Constant("None"));
            }
            if (definition == typeof(Result<,>))
            {
                var active = Expression.Property(value, nameof(Result<int, int>.IsOk));
                return Expression.Condition(active,
                    Wrap("Ok", Format(arguments[0], Expression.Property(value, nameof(Result<int, int>.Value)),
                        Expression.Constant(true), metadata, enums)),
                    Wrap("Err", Format(arguments[1], Expression.Property(value, nameof(Result<int, int>.Error)),
                        Expression.Constant(true), metadata, enums)));
            }
            if (definition == typeof(IReadOnlyList<>))
                return Expression.Call(typeof(CoflowFormatting), nameof(FormatList), arguments,
                    value, Expression.Constant(CreateFormatter(arguments[0], metadata, enums)));
            if (definition == typeof(IReadOnlyDictionary<,>))
                return Expression.Call(typeof(CoflowFormatting), nameof(FormatDictionary), arguments,
                    value, Expression.Constant(CreateFormatter(arguments[0], metadata, enums)),
                    Expression.Constant(CreateFormatter(arguments[1], metadata, enums)));
        }
        var objectMetadata = metadata.Values.Single(item => item.RuntimeType == type);
        if (objectMetadata is ICoflowRecordMetadata recordMetadata)
        {
            var keyType = recordMetadata.KeyType;
            var key = Expression.Convert(Expression.Call(Expression.Constant(recordMetadata),
                typeof(ICoflowRecordMetadata).GetMethod(nameof(ICoflowRecordMetadata.GetKey))!,
                Expression.Convert(value, typeof(object))), keyType);
            var rendered = Format(keyType, key, Expression.Constant(false), metadata, enums);
            return Expression.Call(typeof(CoflowFormatting), nameof(FormatRecord), Type.EmptyTypes,
                Expression.Constant(objectMetadata.DeclaredType), rendered);
        }
        var fieldValues = objectMetadata.Fields
            .Select(field => field.Binding)
            .Where(binding => !binding.IsFunction)
            .Select(binding =>
        {
            var fieldType = binding.RuntimeType;
            var reader = binding.Reader;
            var fieldValue = Expression.Invoke(Expression.Convert(Expression.Constant(reader),
                Expression.GetFuncType(type, fieldType)), value);
            return Expression.Call(typeof(CoflowFormatting), nameof(Concat2), Type.EmptyTypes,
                Expression.Constant(binding.Name + ": "),
                Format(fieldType, fieldValue, Expression.Constant(true), metadata, enums));
        }).ToArray();
        return Expression.Call(typeof(CoflowFormatting), nameof(FormatObject), Type.EmptyTypes,
            Expression.Constant(objectMetadata.DeclaredType),
            Expression.NewArrayInit(typeof(string), fieldValues));
    }

    private static Expression Wrap(string name, Expression value) => Expression.Call(
        typeof(CoflowFormatting), nameof(Concat3), Type.EmptyTypes,
        Expression.Constant(name + "("), value, Expression.Constant(")"));

    private static string Concat2(string first, string second) => first + second;
    private static string Concat3(string first, string second, string third) => first + second + third;

    private static string FormatString(string value, bool nested) => nested
        ? "\"" + value.Replace("\\", "\\\\", StringComparison.Ordinal)
            .Replace("\"", "\\\"", StringComparison.Ordinal)
            .Replace("\n", "\\n", StringComparison.Ordinal)
            .Replace("\r", "\\r", StringComparison.Ordinal)
            .Replace("\t", "\\t", StringComparison.Ordinal) + "\""
        : value;
    private static string FormatEnum(long value, IReadOnlyDictionary<long, string> names) =>
        names.TryGetValue(value, out var name) ? name : value.ToString(CultureInfo.InvariantCulture);
    private static string FormatRecord(string type, string key) => key.Length == 0 ? type : $"&{type}::{key}";
    private static string FormatObject(string type, string[] fields) => $"{type} {{ {string.Join(", ", fields)} }}";
    private static string FormatList<T>(IReadOnlyList<T> values, Delegate formatter)
    {
        var format = (Func<T, bool, string>)formatter;
        return $"[{string.Join(", ", values.Select(value => format(value, true)))}]";
    }
    private static string FormatDictionary<TKey, TValue>(
        IReadOnlyDictionary<TKey, TValue> values, Delegate keyFormatter, Delegate valueFormatter)
        where TKey : notnull
    {
        var formatKey = (Func<TKey, bool, string>)keyFormatter;
        var formatValue = (Func<TValue, bool, string>)valueFormatter;
        var entries = string.Join(", ", values.Select(item =>
            formatKey(item.Key, true) + ": " + formatValue(item.Value, true)));
        return "{ " + entries + " }";
    }
}
}
