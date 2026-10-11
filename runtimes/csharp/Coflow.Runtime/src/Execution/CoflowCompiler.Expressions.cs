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
        internal sealed class TypedFunction
        {
            private readonly Expr _body;
            private readonly CoflowFrozenArray<CoflowBindingDependency> _bindingDependencies;

            internal TypedFunction(Expr body, CoflowBindingDependency[] bindingDependencies)
            {
                _body = body;
                _bindingDependencies = CoflowFrozenArray<CoflowBindingDependency>.CopyOf(bindingDependencies);
            }

            internal Expr Body => _body;
            internal CoflowFrozenArray<CoflowBindingDependency> BindingDependencies => _bindingDependencies;
        }

        internal abstract record Expr(Type Type)
        {
            internal int SourceOffset { get; init; } = -1;

            internal Expr At(int offset) => SourceOffset >= 0 ? this : this with { SourceOffset = offset };

            internal virtual bool AlwaysTerminates => false;
            internal virtual CoflowFunctionSignature? CallableSignature =>
                FunctionSignature(Type);

            internal virtual Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!IsAssignable(Type, expected))
                    checker.Error("COFLOW-FUNCTION-TYPE",
                        $"expression has type `{checker.Format(Type)}` but `{checker.Format(expected)}` is required");
                return Type == expected ? this : new RetypedExpr(this, expected);
            }
        }

        private static CoflowFunctionSignature? FunctionSignature(Type type)
        {
            if (!CoflowFunctionHandle.IsFunctionType(type)) return null;
            var arguments = type.GetGenericArguments();
            return new CoflowFunctionSignature(arguments[^1], arguments[..^1]);
        }

        private static bool IsAssignable(Type source, Type target)
        {
            if (source == target) return true;
            var sourceFunction = FunctionSignature(source);
            var targetFunction = FunctionSignature(target);
            if (sourceFunction is not null || targetFunction is not null)
                return sourceFunction is not null && targetFunction is not null &&
                    IsFunctionAssignable(sourceFunction, targetFunction);
            return target.IsAssignableFrom(source);
        }

        private static bool IsFunctionAssignable(
            CoflowFunctionSignature source,
            CoflowFunctionSignature target)
        {
            if (source.ParameterTypes.Count != target.ParameterTypes.Count) return false;
            for (var index = 0; index < source.ParameterTypes.Count; index++)
                if (!IsAssignable(target.ParameterTypes[index], source.ParameterTypes[index]))
                    return false;
            return IsAssignable(source.ResultType, target.ResultType);
        }

        private static Type DelegateType(CoflowFunctionSignature signature)
        {
            var arguments = signature.ParameterTypes.Append(signature.ResultType).ToArray();
            var definition = arguments.Length switch
            {
                1 => typeof(CoflowFunction<>),
                2 => typeof(CoflowFunction<,>),
                3 => typeof(CoflowFunction<,,>),
                4 => typeof(CoflowFunction<,,,>),
                5 => typeof(CoflowFunction<,,,,>),
                6 => typeof(CoflowFunction<,,,,,>),
                7 => typeof(CoflowFunction<,,,,,,>),
                8 => typeof(CoflowFunction<,,,,,,,>),
                9 => typeof(CoflowFunction<,,,,,,,,>),
                _ => throw new InvalidOperationException("Coflow functions support at most eight parameters."),
            };
            return definition.MakeGenericType(arguments);
        }

        private sealed class NoneMarker { }
        private sealed class ResultBranchMarker { }

        private sealed record ConstantExpr(object? Value, Type ValueType) : Expr(ValueType)
        {
            internal object? TemplateValue { get; init; }
        }

        private sealed record RetypedExpr(Expr Value, Type ExpectedType) : Expr(ExpectedType)
        {
        }

        private sealed record InterpolationPart(string? Text, Expr? Value);

        private sealed record InterpolatedStringExpr(
            IReadOnlyList<InterpolationPart> Parts) : Expr(typeof(string))
        {
        }

        private sealed record ObjectExpr(
            ICoflowTypeMetadata Metadata,
            CfdLoadContext Context,
            IReadOnlyList<(string Name, Expr Value)> Fields) : Expr(Metadata.RuntimeType)
        {
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!expected.IsAssignableFrom(Type))
                    return base.WithExpected(expected, checker);
                return expected == Type ? this : new RetypedExpr(this, expected);
            }

        }

        private sealed record NoneExpr() : Expr(typeof(NoneMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Option<>))
                    checker.Error("COFLOW-FUNCTION-TYPE", "`None` requires an Option result type");
                return new TypedNoneExpr(expected);
            }

        }

        private sealed record TypedNoneExpr(Type OptionType) : Expr(OptionType)
        {
        }

        private sealed record SomeExpr(Expr Value) : Expr(typeof(Option<>).MakeGenericType(Value.Type))
        {
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Option<>))
                    checker.Error("COFLOW-FUNCTION-TYPE", "`Some(value)` requires an Option result type");
                var inner = expected.GetGenericArguments()[0];
                return new SomeExpr(Value.WithExpected(inner, checker));
            }

        }

        private sealed record ResultBranchExpr(Expr Value, bool IsOk) : Expr(typeof(ResultBranchMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Result<,>))
                    checker.Error("COFLOW-FUNCTION-TYPE", $"`{(IsOk ? "Ok" : "Err")}(value)` requires a Result result type");
                var arguments = expected.GetGenericArguments();
                return new TypedResultBranchExpr(
                    Value.WithExpected(arguments[IsOk ? 0 : 1], checker),
                    IsOk,
                    expected);
            }

        }

        private sealed record TypedResultBranchExpr(Expr Value, bool IsOk, Type ResultType) : Expr(ResultType)
        {
        }

        private sealed record ArgumentExpr(int Index, Type ArgumentType, string? Name = null) : Expr(ArgumentType)
        {
        }

        private sealed record LocalExpr(int Index, Type LocalType, string? Name = null) : Expr(LocalType)
        {
        }

        private sealed record TypeIsExpr(Expr Value, Type TargetType, string? NarrowName) : Expr(typeof(bool))
        {
        }

        private sealed record FunctionReferenceExpr(CoflowFunctionEntry Entry, Expr? Receiver) : Expr(DelegateType(Entry.Signature))
        {
            internal override CoflowFunctionSignature CallableSignature => Entry.Signature;
        }

        private sealed record LambdaExpr(
            CoflowFunctionSignature Signature,
            IReadOnlyList<Expr> Captures,
            Expr Body) : Expr(DelegateType(Signature))
        {
            internal override CoflowFunctionSignature CallableSignature => Signature;

        }

        private sealed class LambdaParseContext
        {
            private readonly Dictionary<string, int> _captureIndexes = new(StringComparer.Ordinal);
            private readonly List<Expr> _captures = new();
            private readonly int _parameterCount;

            internal LambdaParseContext(int scopeBase, Dictionary<string, (int Index, Type Type)> parameters, int parameterCount)
            {
                ScopeBase = scopeBase;
                Parameters = parameters;
                _parameterCount = parameterCount;
            }

            internal int ScopeBase { get; }
            internal Dictionary<string, (int Index, Type Type)> Parameters { get; }
            internal IReadOnlyList<Expr> Captures => _captures;

            internal Expr Capture(string identity, Expr source)
            {
                if (!_captureIndexes.TryGetValue(identity, out var index))
                {
                    index = _captures.Count;
                    _captureIndexes.Add(identity, index);
                    _captures.Add(source);
                }
                return new ArgumentExpr(_parameterCount + index, source.Type);
            }
        }

        private sealed record CallExpr(
            Expr Target,
            CoflowFunctionSignature Signature,
            IReadOnlyList<Expr> Arguments)
            : Expr(Signature.ResultType)
        {

        }

        private sealed record EmptyArrayExpr() : Expr(typeof(ArrayMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                    checker.Error("COFLOW-FUNCTION-TYPE", "empty array requires an array expected type");
                return new ArrayExpr(Array.Empty<Expr>(), expected);
            }
        }

        private sealed record EmptyDictionaryExpr() : Expr(typeof(DictionaryMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(IReadOnlyDictionary<,>))
                    checker.Error("COFLOW-FUNCTION-TYPE", "empty dictionary requires a dictionary expected type");
                return new DictionaryExpr(Array.Empty<(Expr, Expr)>(), expected);
            }
        }

        private sealed class ArrayMarker { }
        private sealed class DictionaryMarker { }

        private sealed record ArrayExpr(IReadOnlyList<Expr> Values, Type ArrayType) : Expr(ArrayType)
        {
        }

        private sealed record DictionaryExpr(
            IReadOnlyList<(Expr Key, Expr Value)> Entries,
            Type DictionaryType) : Expr(DictionaryType)
        {
        }

        private sealed record IndexExpr(
            Expr Receiver,
            Expr Index,
            Type ResultType,
            bool IsDictionary)
            : Expr(ResultType)
        {
            internal static Expr Create(Expr receiver, Expr index, FunctionTypeChecker checker)
            {
                if (!receiver.Type.IsGenericType)
                    return Invalid();
                var definition = receiver.Type.GetGenericTypeDefinition();
                var arguments = receiver.Type.GetGenericArguments();
                if (definition == typeof(IReadOnlyList<>))
                {
                    index.WithExpected(typeof(long), checker);
                    return new IndexExpr(receiver, index,
                        typeof(Option<>).MakeGenericType(arguments[0]), false);
                }
                if (definition == typeof(IReadOnlyDictionary<,>))
                {
                    index.WithExpected(arguments[0], checker);
                    return new IndexExpr(receiver, index,
                        typeof(Option<>).MakeGenericType(arguments[1]), true);
                }
                return Invalid();

                Expr Invalid()
                {
                    checker.Error("COFLOW-FUNCTION-INDEX", $"`{checker.Format(receiver.Type)}` cannot be indexed");
                    return null!;
                }
            }

        }

        private sealed record FieldExpr(Expr Receiver, Type FieldType, CoflowFieldAccess Access)
            : Expr(FieldType)
        {
        }

        private sealed record TransformExpr(Expr Receiver, Type ResultType, CoflowNativeCall Transform)
            : Expr(ResultType)
        {
        }

        private sealed record PropagateExpr(Expr Operand, Type ValueType) : Expr(ValueType)
        {
        }

        internal sealed record MatchArm(MatchPattern Pattern, int? BindingLocal, Expr Body);

        private sealed record MatchExpr(
            Expr Subject,
            int SubjectLocal,
            IReadOnlyList<MatchArm> Arms,
            bool LastIsComplement) : Expr(Arms[0].Body.Type)
        {
            internal override bool AlwaysTerminates => Arms.All(arm => arm.Body.AlwaysTerminates);
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker) =>
                new MatchExpr(Subject, SubjectLocal,
                    Arms.Select(arm => arm with { Body = arm.Body.WithExpected(expected, checker) }).ToArray(),
                    LastIsComplement);

        }

        internal sealed record MatchPattern(
            string Kind,
            bool IsCatchAll,
            string? BindingName,
            Type? BindingType,
            object? LiteralValue = null,
            Type? TypeTarget = null,
            bool? TagValue = null,
            int? Payload = null)
        {
            internal static MatchPattern CatchAll(string kind, string? name, Type? type) =>
                new(kind, true, name, type);
            internal static MatchPattern Literal(string kind, object value) =>
                new(kind, false, null, null, value);
        }

        private sealed record BuiltinExpr(
            Expr Receiver,
            IReadOnlyList<Expr> Arguments,
            CoflowBuiltin Builtin) : Expr(Builtin.ResultType)
        {
        }

        private sealed record HigherOrderExpr(
            Expr Receiver,
            IReadOnlyList<Expr> Arguments,
            CoflowHigherOrderOperation Operation) : Expr(Operation.ResultType)
        {
        }

        private sealed record StoreLocalExpr(int Index, Expr Value) : Expr(typeof(Unit))
        {
        }

        private sealed record AssignLocalExpr(int Index, Expr Value) : Expr(typeof(Unit))
        {
        }

        private sealed record ReturnExpr(Expr Value) : Expr(typeof(Unit))
        {
            internal override bool AlwaysTerminates => true;
        }

        private sealed record LoopControlExpr(bool IsBreak) : Expr(typeof(Unit))
        {
            internal override bool AlwaysTerminates => true;
        }

        private sealed record WhileExpr(Expr Condition, Expr Body) : Expr(typeof(Unit))
        {
        }

        private sealed record ForExpr(
            Expr Collection,
            bool IsArray,
            int CollectionLocal,
            int IndexLocal,
            int FirstLocal,
            int? SecondLocal,
            Type FirstType,
            Type? SecondType,
            Expr Body) : Expr(typeof(Unit))
        {
        }

        private sealed record RangeForExpr(
            Expr Start,
            Expr End,
            bool Inclusive,
            int ValueLocal,
            int EndLocal,
            int? IndexLocal,
            Expr Body) : Expr(typeof(Unit))
        {
        }

        private sealed record RangeExpr(Expr Start, Expr End, bool Inclusive) : Expr(typeof(RangeExpr))
        {
        }

        private sealed class LoopEmitContext
        {
            private readonly int continueTarget;

            internal LoopEmitContext(int continueTarget)
            {
                this.continueTarget = continueTarget;
                ContinueTarget = continueTarget;
            }

            internal int ContinueTarget { get; set; }
            internal List<int> BreakJumps { get; } = new();
            internal List<int> ContinueJumps { get; } = new();
        }

        private sealed record DiscardExpr(Expr Value) : Expr(typeof(Unit))
        {
        }

        private sealed record BlockExpr(IReadOnlyList<Expr> Statements, Expr Result) : Expr(Result.Type)
        {
            internal override bool AlwaysTerminates =>
                Statements.Any(statement => statement.AlwaysTerminates) || Result.AlwaysTerminates;
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker) =>
                new BlockExpr(Statements, Result.WithExpected(expected, checker));



        }

        private sealed record IfExpr(Expr Condition, Expr WhenTrue, Expr WhenFalse) : Expr(WhenTrue.Type)
        {
            internal static Expr Create(Expr condition, Expr whenTrue, Expr whenFalse) =>
                condition is ConstantExpr { Value: bool value }
                    ? value ? whenTrue : whenFalse
                    : new IfExpr(condition, whenTrue, whenFalse);

            internal override bool AlwaysTerminates => WhenTrue.AlwaysTerminates && WhenFalse.AlwaysTerminates;
            internal override Expr WithExpected(Type expected, FunctionTypeChecker checker) =>
                new IfExpr(
                    Condition,
                    WhenTrue.WithExpected(expected, checker),
                    WhenFalse.WithExpected(expected, checker));


        }

        private static class ValueFactories
        {

            internal static CoflowHigherOrderOperation HigherOrder(
                string name, Type element, Type outputElement, Type resultType) => new(
                    name,
                    element,
                    outputElement,
                    resultType);
            internal static MatchPattern MatchNone(Type subject, FunctionTypeChecker checker)
            {
                if (!subject.IsGenericType || subject.GetGenericTypeDefinition() != typeof(Option<>))
                    checker.Error("COFLOW-FUNCTION-TYPE", "None pattern requires Option");
                return new MatchPattern("None", false, null, null, TagValue: false);
            }
            internal static MatchPattern MatchBranch(Type subject, string kind, string binding, FunctionTypeChecker checker)
            {
                if (subject.IsGenericType && subject.GetGenericTypeDefinition() == typeof(Option<>) && kind == "Some")
                {
                    var type = subject.GetGenericArguments()[0];
                    return new MatchPattern(kind, false, binding, type,
                        TagValue: true, Payload: 0);
                }
                if (subject.IsGenericType && subject.GetGenericTypeDefinition() == typeof(Result<,>) && kind is "Ok" or "Err")
                {
                    var types = subject.GetGenericArguments();
                    var ok = kind == "Ok";
                    return new MatchPattern(kind, false, binding, types[ok ? 0 : 1],
                        TagValue: ok, Payload: ok ? 0 : 1);
                }
                checker.Error("COFLOW-FUNCTION-TYPE", $"{kind} pattern does not match subject type");
                return null!;
            }
        }

        private sealed record UnaryExpr(string Operation, Expr Operand, Type ResultType) : Expr(ResultType)
        {
            internal static Expr Create(string operation, Expr operand, FunctionTypeChecker checker)
            {
                if (operand is ConstantExpr constant)
                {
                    try
                    {
                        if (operation == "!" && constant.Value is bool boolean)
                            return new ConstantExpr(!boolean, typeof(bool));
                        if (operation == "-" && constant.Value is long integer)
                            return new ConstantExpr(checked(-integer), typeof(long));
                        if (operation == "-" && constant.Value is double floating)
                            return new ConstantExpr(-floating, typeof(double));
                        if (operation == "~" && constant.Value is long bits)
                            return new ConstantExpr(~bits, typeof(long));
                    }
                    catch (OverflowException) { }
                }
                if (operation == "!" && operand.Type == typeof(bool))
                    return new UnaryExpr(operation, operand, typeof(bool));
                if (operation == "-" && operand.Type == typeof(long))
                    return new UnaryExpr(operation, operand, typeof(long));
                if (operation == "-" && operand.Type == typeof(double))
                    return new UnaryExpr(operation, operand, typeof(double));
                if (operation == "~" && operand.Type == typeof(long))
                    return new UnaryExpr(operation, operand, typeof(long));
                if (operation == "~" && operand.Type.IsEnum)
                {
                    var metadata = checker.EnumMetadata(operand.Type);
                    if (!metadata.IsFlags)
                        checker.Error("COFLOW-FUNCTION-TYPE", "`~` requires a flag enum");
                    return new EnumUnaryExpr(operand, metadata);
                }
                checker.Error("COFLOW-FUNCTION-TYPE",
                    $"operator `{operation}` cannot be applied to `{checker.Format(operand.Type)}`");
                return null!;
            }

        }

        private sealed record EnumUnaryExpr(Expr Operand, ICoflowEnumMetadata Metadata) : Expr(Operand.Type)
        {
        }

        private sealed record EnumBinaryExpr(
            string Operation,
            Expr Left,
            Expr Right,
            Type ResultType) : Expr(ResultType)
        {
            internal static Expr Create(
                string operation,
                Expr left,
                Expr right,
                FunctionTypeChecker checker,
                ICoflowEnumMetadata metadata)
            {
                if (left.Type != right.Type || !left.Type.IsEnum)
                    checker.Error("COFLOW-FUNCTION-TYPE", "enum operators require the same enum type");
                if (operation is "&" or "|" or "^")
                {
                    if (!metadata.IsFlags)
                        checker.Error("COFLOW-FUNCTION-TYPE", "bit operators require a flag enum");
                    return new EnumBinaryExpr(operation, left, right, left.Type);
                }
                if (operation is "==" or "!=")
                    return new EqualityExpr(left, right, operation == "!=");
                if (operation is "<" or "<=" or ">" or ">=")
                    return new EnumBinaryExpr(operation, left, right, typeof(bool));
                checker.Error("COFLOW-FUNCTION-TYPE", $"operator `{operation}` cannot be applied to enum");
                return null!;
            }

        }

        private sealed record ConversionExpr(Expr Value, Type ResultType)
            : Expr(ResultType)
        {
            internal static Expr Create(Expr value, Type resultType)
            {
                if (value is ConstantExpr constant)
                {
                    try
                    {
                        if (resultType == typeof(double) && constant.Value is long integer)
                            return new ConstantExpr((double)integer, resultType);
                        if (resultType == typeof(long) && constant.Value is double floating)
                            return new ConstantExpr(checked((long)floating), resultType);
                    }
                    catch (OverflowException) { }
                }
                return new ConversionExpr(value, resultType);
            }

        }

        private sealed record BinaryExpr(
            string Operation,
            Expr Left,
            Expr Right,
            Type ResultType) : Expr(ResultType)
        {
            internal static Expr Create(string operation, Expr left, Expr right, FunctionTypeChecker checker)
            {
                if (left.Type != right.Type)
                    checker.Error("COFLOW-FUNCTION-TYPE",
                        $"operator `{operation}` requires equal operand types, found `{checker.Format(left.Type)}` and `{checker.Format(right.Type)}`");
                if (operation is "&&" or "||")
                {
                    if (left.Type != typeof(bool)) checker.Error("COFLOW-FUNCTION-TYPE", $"operator `{operation}` requires bool operands");
                    if (left is ConstantExpr { Value: bool leftBoolean })
                    {
                        if (operation == "&&") return leftBoolean ? right : left;
                        return leftBoolean ? left : right;
                    }
                    return new BinaryExpr(operation, left, right, typeof(bool));
                }
                if (operation is "==" or "!=")
                {
                    if (!checker.SupportsEquality(left.Type))
                        checker.Error("COFLOW-FUNCTION-TYPE", "function values cannot be compared");
                    return new EqualityExpr(left, right, operation == "!=");
                }
                ValidateOperation(operation, left.Type, checker);
                var result = operation is "<" or "<=" or ">" or ">=" ? typeof(bool) : left.Type;
                if (left is ConstantExpr leftConstant && right is ConstantExpr rightConstant &&
                    TryFold(operation, leftConstant.Value, rightConstant.Value, out var folded))
                    return new ConstantExpr(folded, result);
                return new BinaryExpr(operation, left, right, result);
            }

            private static bool TryFold(string operation, object? left, object? right, out object? result)
            {
                result = null;
                try
                {
                    if (left is long leftInteger && right is long rightInteger)
                    {
                        result = operation switch
                        {
                            "+" => checked(leftInteger + rightInteger), "-" => checked(leftInteger - rightInteger),
                            "*" => checked(leftInteger * rightInteger), "/" or "//" => checked(leftInteger / rightInteger),
                            "%" => checked(leftInteger % rightInteger), "**" => FoldPower(leftInteger, rightInteger),
                            "<<" => checked(leftInteger << checked((int)rightInteger)),
                            ">>" => leftInteger >> checked((int)rightInteger),
                            "&" => leftInteger & rightInteger, "^" => leftInteger ^ rightInteger, "|" => leftInteger | rightInteger,
                            "<" => leftInteger < rightInteger, "<=" => leftInteger <= rightInteger,
                            ">" => leftInteger > rightInteger, ">=" => leftInteger >= rightInteger,
                            _ => null,
                        };
                        return result is not null;
                    }
                    if (left is double leftFloat && right is double rightFloat)
                    {
                        result = operation switch
                        {
                            "+" => leftFloat + rightFloat, "-" => leftFloat - rightFloat,
                            "*" => leftFloat * rightFloat, "/" => leftFloat / rightFloat,
                            "**" => Math.Pow(leftFloat, rightFloat), "<" => leftFloat < rightFloat,
                            "<=" => leftFloat <= rightFloat, ">" => leftFloat > rightFloat,
                            ">=" => leftFloat >= rightFloat, _ => null,
                        };
                        return result is not null;
                    }
                    if (left is string leftString && right is string rightString)
                    {
                        var comparison = string.CompareOrdinal(leftString, rightString);
                        result = operation switch
                        {
                            "+" => leftString + rightString, "<" => comparison < 0,
                            "<=" => comparison <= 0, ">" => comparison > 0,
                            ">=" => comparison >= 0, _ => null,
                        };
                        return result is not null;
                    }
                }
                catch (ArithmeticException) { }
                return false;
            }

            private static long FoldPower(long value, long exponent)
            {
                if (exponent < 0) throw new ArithmeticException();
                var result = 1L;
                for (var factor = value; exponent != 0; exponent >>= 1)
                {
                    if ((exponent & 1) != 0) result = checked(result * factor);
                    if (exponent > 1) factor = checked(factor * factor);
                }
                return result;
            }

            private static void ValidateOperation(string operation, Type type, FunctionTypeChecker checker)
            {
                bool valid = type == typeof(long)
                    ? operation is "+" or "-" or "*" or "/" or "//" or "%" or "**" or
                        "<<" or ">>" or "&" or "^" or "|" or "<" or "<=" or ">" or ">="
                    : type == typeof(double)
                        ? operation is "+" or "-" or "*" or "/" or "**" or "<" or "<=" or ">" or ">="
                        : type == typeof(string) && operation is "+" or "<" or "<=" or ">" or ">=";
                if (!valid)
                    checker.Error("COFLOW-FUNCTION-TYPE",
                        $"operator `{operation}` cannot be applied to `{checker.Format(type)}`");
            }

        }

        private sealed record EqualityExpr(
            Expr Left,
            Expr Right,
            bool Negated) : Expr(typeof(bool))
        {
        }

        private sealed record ComparisonChainExpr(
            IReadOnlyList<Expr> Operands,
            IReadOnlyList<string> Operations) : Expr(typeof(bool))
        {
            internal static Expr Create(
                Expr first,
                Expr second,
                string firstOperation,
                string secondOperation,
                Expr third,
                FunctionTypeChecker checker)
            {
                _ = BinaryExpr.Create(firstOperation, first, second, checker);
                _ = BinaryExpr.Create(secondOperation, second, third, checker);
                return new ComparisonChainExpr(
                    new[] { first, second, third },
                    new[] { firstOperation, secondOperation });
            }

            internal ComparisonChainExpr Append(string operation, Expr operand, FunctionTypeChecker checker)
            {
                _ = BinaryExpr.Create(operation, Operands[^1], operand, checker);
                return new ComparisonChainExpr(
                    Operands.Append(operand).ToArray(),
                    Operations.Append(operation).ToArray());
            }

        }

        private static bool TryBinary(TokenKind kind, out int precedence, out string operation)
        {
            (precedence, operation) = kind switch
            {
                TokenKind.OrOr => (1, "||"), TokenKind.AndAnd => (2, "&&"),
                TokenKind.Pipe => (3, "|"), TokenKind.Caret => (4, "^"), TokenKind.Ampersand => (5, "&"),
                TokenKind.EqualEqual => (6, "=="), TokenKind.BangEqual => (6, "!="),
                TokenKind.Less => (7, "<"), TokenKind.LessEqual => (7, "<="),
                TokenKind.Greater => (7, ">"), TokenKind.GreaterEqual => (7, ">="),
                TokenKind.Plus => (8, "+"), TokenKind.Minus => (8, "-"),
                TokenKind.ShiftLeft => (8, "<<"), TokenKind.ShiftRight => (8, ">>"),
                TokenKind.Star => (9, "*"), TokenKind.Slash => (9, "/"),
                TokenKind.DoubleSlash => (9, "//"), TokenKind.Percent => (9, "%"),
                TokenKind.Power => (10, "**"),
                _ => (0, string.Empty),
            };
            return operation.Length != 0;
        }
}
}
