using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal static partial class CoflowFunctionFrontend
{
    /// <summary>类型检查上下文集中拥有类型关系、运算符约束和类型诊断。</summary>
    internal sealed class FunctionTypeChecker
    {
        private readonly CoflowCompilerCatalog catalog;
        private readonly Func<int> currentOffset;

        internal FunctionTypeChecker(CoflowCompilerCatalog catalog, Func<int> currentOffset)
        {
            this.catalog = catalog;
            this.currentOffset = currentOffset;
        }

        internal readonly struct ForCollection
        {
            public bool IsRange { get; init; }
            public bool IsArray { get; init; }
            public Type FirstType { get; init; }
            public Type? SecondType { get; init; }

            public ForCollection(bool IsRange, bool IsArray, Type FirstType, Type? SecondType)
            {
                this.IsRange = IsRange;
                this.IsArray = IsArray;
                this.FirstType = FirstType;
                this.SecondType = SecondType;
            }
        }

        internal IReadOnlyDictionary<string, ICoflowTypeMetadata> Metadata => catalog.Metadata;

        internal string Format(Type type) => CoflowTypeNameResolver.Format(type, catalog);

        internal ICoflowEnumMetadata EnumMetadata(Type type) =>
            catalog.EnumsByRuntimeType.TryGetValue(type, out var metadata)
                ? metadata
                : throw new InvalidOperationException($"No enum metadata exists for `{type}`.");

        [System.Diagnostics.CodeAnalysis.DoesNotReturn]
        internal void Error(string code, string message) =>
            throw new FunctionCompileException(code, message, currentOffset());

        internal bool SupportsEquality(Type type) => SupportsEquality(type, new HashSet<Type>());

        internal bool IsInterpolatable(Type type) => IsInterpolatable(type, new HashSet<Type>());

        internal Expr ArrayLiteral(IReadOnlyList<Expr> values)
        {
            if (values.Count == 0) return new EmptyArrayExpr();
            var elementType = CommonType(values);
            var typed = values.Select(value => value.WithExpected(elementType, this)).ToArray();
            return new ArrayExpr(typed, typeof(IReadOnlyList<>).MakeGenericType(elementType));
        }

        internal Expr DictionaryLiteral(IReadOnlyList<(Expr Key, Expr Value)> entries)
        {
            if (entries.Count == 0) return new EmptyDictionaryExpr();
            var keyType = CommonType(entries.Select(entry => entry.Key).ToArray());
            var valueType = CommonType(entries.Select(entry => entry.Value).ToArray());
            if (keyType != typeof(long) && keyType != typeof(string) && !keyType.IsEnum)
                Error("COFLOW-FUNCTION-TYPE", "dictionary keys must be int, string, or enum");
            return new DictionaryExpr(entries.Select(entry => (
                    entry.Key.WithExpected(keyType, this),
                    entry.Value.WithExpected(valueType, this))).ToArray(),
                typeof(IReadOnlyDictionary<,>).MakeGenericType(keyType, valueType));
        }

        internal Expr IfExpression(Expr condition, Expr whenTrue, Expr? whenFalse)
        {
            if (condition.Type != typeof(bool))
                Error("COFLOW-FUNCTION-TYPE", "if condition must be bool");
            if (whenFalse is null)
            {
                if (whenTrue.Type != typeof(Unit) && !whenTrue.AlwaysTerminates)
                    Error("COFLOW-FUNCTION-TYPE", "if without else must have type `()`");
                whenFalse = new ConstantExpr(Unit.Value, typeof(Unit));
            }
            if (whenTrue.Type == typeof(NoneMarker) && IsOption(whenFalse.Type))
                whenTrue = whenTrue.WithExpected(whenFalse.Type, this);
            else if (whenFalse.Type == typeof(NoneMarker) && IsOption(whenTrue.Type))
                whenFalse = whenFalse.WithExpected(whenTrue.Type, this);
            else if (ResultBranch(whenTrue) is { } trueResult &&
                ResultBranch(whenFalse) is { } falseResult &&
                trueResult.IsOk != falseResult.IsOk)
            {
                var ok = trueResult.IsOk ? trueResult.Value.Type : falseResult.Value.Type;
                var error = trueResult.IsOk ? falseResult.Value.Type : trueResult.Value.Type;
                var resultType = typeof(Result<,>).MakeGenericType(ok, error);
                whenTrue = whenTrue.WithExpected(resultType, this);
                whenFalse = whenFalse.WithExpected(resultType, this);
            }
            if (whenTrue.Type != whenFalse.Type)
            {
                if (IsAssignable(whenTrue.Type, whenFalse.Type))
                    whenTrue = whenTrue.WithExpected(whenFalse.Type, this);
                else if (IsAssignable(whenFalse.Type, whenTrue.Type))
                    whenFalse = whenFalse.WithExpected(whenTrue.Type, this);
                else
                    Error("COFLOW-FUNCTION-TYPE",
                        $"if branches have different types `{Format(whenTrue.Type)}` and `{Format(whenFalse.Type)}`");
            }
            return IfExpr.Create(condition, whenTrue, whenFalse);

            static bool IsOption(Type type) => type.IsGenericType &&
                type.GetGenericTypeDefinition() == typeof(Option<>);
            static ResultBranchExpr? ResultBranch(Expr expression) => expression switch
            {
                ResultBranchExpr result => result,
                BlockExpr block => ResultBranch(block.Result),
                _ => null,
            };
        }

        internal Expr Call(Expr target, IReadOnlyList<Expr> arguments)
        {
            var signature = target.CallableSignature;
            if (signature is null)
                Error("COFLOW-FUNCTION-CALL", "expression is not callable");
            if (arguments.Count != signature.ParameterTypes.Count)
                Error("COFLOW-FUNCTION-CALL",
                    $"function expects {signature.ParameterTypes.Count} arguments but received {arguments.Count}");
            var typed = new Expr[arguments.Count];
            for (var index = 0; index < typed.Length; index++)
                typed[index] = arguments[index].WithExpected(signature.ParameterTypes[index], this);
            return new CallExpr(target, signature, typed);
        }

        internal Expr While(Expr condition, Expr body)
        {
            if (condition.Type != typeof(bool))
                Error("COFLOW-FUNCTION-TYPE", "while condition must be bool");
            if (body.Type != typeof(Unit) && !body.AlwaysTerminates)
                Error("COFLOW-FUNCTION-TYPE", "while body must have type `()`");
            return new WhileExpr(condition, body);
        }

        internal ForCollection ForCollectionType(Expr collection, bool hasSecondBinding)
        {
            if (collection is RangeExpr)
                return new ForCollection(true, true, typeof(long),
                    hasSecondBinding ? typeof(long) : null);
            if (!collection.Type.IsGenericType)
                Error("COFLOW-FUNCTION-TYPE", "for requires an array, dictionary, or range");
            var definition = collection.Type.GetGenericTypeDefinition();
            var arguments = collection.Type.GetGenericArguments();
            if (definition == typeof(IReadOnlyList<>))
                return new ForCollection(false, true, arguments[0],
                    hasSecondBinding ? typeof(long) : null);
            if (definition == typeof(IReadOnlyDictionary<,>))
            {
                if (!hasSecondBinding)
                    Error("COFLOW-FUNCTION-TYPE",
                        "dictionary for loops require `key, value` bindings");
                return new ForCollection(false, false, arguments[0], arguments[1]);
            }
            Error("COFLOW-FUNCTION-TYPE", "for requires an array, dictionary, or range");
            return default;
        }

        internal void RequireLoopBody(Expr body)
        {
            if (body.Type != typeof(Unit) && !body.AlwaysTerminates)
                Error("COFLOW-FUNCTION-TYPE", "for body must have type `()`");
        }

        internal Expr Propagate(Expr operand, Type returnType)
        {
            if (!operand.Type.IsGenericType)
                Error("COFLOW-FUNCTION-PROPAGATE", "`?` requires Option or Result");
            var definition = operand.Type.GetGenericTypeDefinition();
            var arguments = operand.Type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                if (!returnType.IsGenericType ||
                    returnType.GetGenericTypeDefinition() != typeof(Option<>))
                    Error("COFLOW-FUNCTION-PROPAGATE",
                        "Option can only propagate from an Option-returning function");
                return new PropagateExpr(operand, arguments[0]);
            }
            if (definition == typeof(Result<,>))
            {
                if (!returnType.IsGenericType ||
                    returnType.GetGenericTypeDefinition() != typeof(Result<,>) ||
                    returnType.GetGenericArguments()[1] != arguments[1])
                    Error("COFLOW-FUNCTION-PROPAGATE",
                        "Result can only propagate to a Result with the same error type");
                return new PropagateExpr(operand, arguments[0]);
            }
            Error("COFLOW-FUNCTION-PROPAGATE", "`?` requires Option or Result");
            return null!;
        }

        internal Expr NumericConversion(string target, Expr value)
        {
            var result = target == "int" ? typeof(long) : typeof(double);
            if (value.Type != typeof(long) && value.Type != typeof(double))
                Error("COFLOW-FUNCTION-TYPE", $"{target} conversion requires int or float");
            return value.Type == result ? value : ConversionExpr.Create(value, result);
        }

        internal Expr Builtin(Expr receiver, string name, IReadOnlyList<Expr> arguments)
        {
            string? regexPattern = null;
            if (name == "matches")
            {
                var pattern = arguments.Count == 1 && arguments[0] is ConstantExpr constant
                    ? constant.Value as string
                    : null;
                if (pattern is null)
                    Error("COFLOW-FUNCTION-BUILTIN", "matches pattern must be a string literal");
                regexPattern = pattern;
                try { CoflowBuiltinLibrary.ValidateRegexPattern(pattern); }
                catch (ArgumentException error) { Error("COFLOW-FUNCTION-BUILTIN", error.Message); }
            }
            if (name is "map" or "filter" or "fold" or "find" or "any" or "all")
                return HigherOrderBuiltin(receiver, name, arguments);
            try
            {
                var builtin = regexPattern is null
                    ? CoflowBuiltinLibrary.Resolve(name, receiver.Type,
                        arguments.Select(argument => argument.Type).ToArray())
                    : CoflowBuiltinLibrary.ResolveRegex(regexPattern);
                return new BuiltinExpr(receiver,
                    regexPattern is null ? arguments : Array.Empty<Expr>(), builtin);
            }
            catch (ArgumentException error)
            {
                Error("COFLOW-FUNCTION-BUILTIN", error.Message);
                return null!;
            }
        }

        private Expr HigherOrderBuiltin(Expr receiver, string name, IReadOnlyList<Expr> arguments)
        {
            if (!receiver.Type.IsGenericType ||
                receiver.Type.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                Error("COFLOW-FUNCTION-BUILTIN", $"{name} requires an array receiver");
            var element = receiver.Type.GetGenericArguments()[0];
            Type result;
            Type outputElement;
            if (name == "fold")
            {
                if (arguments.Count != 2)
                    Error("COFLOW-FUNCTION-BUILTIN", "fold requires an initial value and a function");
                var signature = arguments[1].CallableSignature;
                var expected = new CoflowFunctionSignature(
                    arguments[0].Type, new[] { arguments[0].Type, element });
                if (signature is null || !IsFunctionAssignable(signature, expected))
                    Error("COFLOW-FUNCTION-BUILTIN", "fold function must have signature fn(A, T) -> A");
                result = arguments[0].Type;
                outputElement = result;
            }
            else
            {
                if (arguments.Count != 1)
                    Error("COFLOW-FUNCTION-BUILTIN", $"{name} requires exactly one function");
                var signature = arguments[0].CallableSignature;
                if (signature is null || signature.ParameterTypes.Count != 1 ||
                    !IsAssignable(element, signature.ParameterTypes[0]))
                    Error("COFLOW-FUNCTION-BUILTIN",
                        $"{name} function must accept the array element type");
                if (name is "filter" or "find" or "any" or "all")
                {
                    if (signature.ResultType != typeof(bool))
                        Error("COFLOW-FUNCTION-BUILTIN", $"{name} function must return bool");
                    outputElement = element;
                    result = name switch
                    {
                        "filter" => receiver.Type,
                        "find" => typeof(Option<>).MakeGenericType(element),
                        _ => typeof(bool),
                    };
                }
                else
                {
                    outputElement = signature.ResultType;
                    result = typeof(IReadOnlyList<>).MakeGenericType(outputElement);
                }
            }
            return new HigherOrderExpr(receiver, arguments,
                ValueFactories.HigherOrder(name, element, outputElement, result));
        }

        internal Expr Match(Expr subject, int subjectLocal,
            IReadOnlyList<MatchArm> arms, bool lastIsComplement)
        {
            var resultType = CommonType(arms.Select(arm => arm.Body).ToArray());
            var typed = arms.Select(arm => arm with
            {
                Body = arm.Body.WithExpected(resultType, this),
            }).ToArray();
            return new MatchExpr(subject, subjectLocal, typed, lastIsComplement);
        }

        private Type CommonType(IReadOnlyList<Expr> values)
        {
            var type = values[0].Type;
            foreach (var value in values.Skip(1))
            {
                if (IsAssignable(value.Type, type)) continue;
                if (IsAssignable(type, value.Type))
                {
                    type = value.Type;
                    continue;
                }
                value.WithExpected(type, this);
            }
            return type;
        }

        private bool SupportsEquality(Type type, HashSet<Type> visiting)
        {
            if (CoflowFunctionHandle.IsFunctionType(type) || type == typeof(CoflowFunctionEntry))
                return false;
            catalog.MetadataByRuntimeType.TryGetValue(type, out var metadata);
            if (metadata is not null)
            {
                if (!visiting.Add(type)) return true;
                var result = metadata.Fields.All(field =>
                    SupportsEquality(field.Binding.RuntimeType, visiting));
                visiting.Remove(type);
                return result;
            }
            if (!type.IsGenericType) return true;
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            return definition == typeof(Option<>) && SupportsEquality(arguments[0], visiting) ||
                definition == typeof(Result<,>) && arguments.All(item => SupportsEquality(item, visiting)) ||
                definition == typeof(IReadOnlyList<>) && SupportsEquality(arguments[0], visiting) ||
                definition == typeof(IReadOnlyDictionary<,>) &&
                    arguments.All(item => SupportsEquality(item, visiting));
        }

        private bool IsInterpolatable(Type type, HashSet<Type> visiting)
        {
            if (type == typeof(long) || type == typeof(double) || type == typeof(bool) ||
                type == typeof(string) || type == typeof(Unit) || type.IsEnum)
                return true;
            if (CoflowFunctionHandle.IsFunctionType(type) || type == typeof(CoflowFunctionEntry))
                return false;
            catalog.MetadataByRuntimeType.TryGetValue(type, out var metadata);
            if (metadata is not null)
            {
                if (metadata is ICoflowRecordMetadata || !visiting.Add(type)) return true;
                var result = metadata.Fields.All(field =>
                    field.Binding.IsFunction || IsInterpolatable(field.Binding.RuntimeType, visiting));
                visiting.Remove(type);
                return result;
            }
            if (!type.IsGenericType) return false;
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            return definition == typeof(Option<>) && IsInterpolatable(arguments[0], visiting) ||
                definition == typeof(Result<,>) && arguments.All(item => IsInterpolatable(item, visiting)) ||
                definition == typeof(IReadOnlyList<>) && IsInterpolatable(arguments[0], visiting) ||
                definition == typeof(IReadOnlyDictionary<,>) &&
                    arguments.All(item => IsInterpolatable(item, visiting));
        }
    }
}
}
