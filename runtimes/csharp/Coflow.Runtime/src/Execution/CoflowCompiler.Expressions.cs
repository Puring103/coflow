namespace Coflow.Runtime.CompilerServices;

internal static partial class CoflowCompiler
{
    private sealed partial class FunctionParser
    {
        private abstract record Expr(Type Type)
        {
            internal int SourceOffset { get; init; } = -1;

            internal Expr At(int offset) => SourceOffset >= 0 ? this : this with { SourceOffset = offset };

            internal void Emit(FunctionParser parser)
            {
                var previous = parser.SetEmissionOffset(SourceOffset);
                var previousType = parser.SetEmissionType(Type);
                try { EmitCore(parser); }
                finally
                {
                    parser.RestoreEmissionType(previousType);
                    parser.RestoreEmissionOffset(previous);
                }
            }
            internal void EmitTail(FunctionParser parser)
            {
                var previous = parser.SetEmissionOffset(SourceOffset);
                var previousType = parser.SetEmissionType(Type);
                try { EmitTailCore(parser); }
                finally
                {
                    parser.RestoreEmissionType(previousType);
                    parser.RestoreEmissionOffset(previous);
                }
            }
            internal abstract void EmitCore(FunctionParser parser);
            internal virtual void EmitTailCore(FunctionParser parser)
            {
                EmitCore(parser);
                parser.Emit(CoflowOpCode.Return);
            }
            internal virtual bool AlwaysTerminates => false;
            internal virtual CoflowFunctionSignature? CallableSignature =>
                FunctionSignature(Type);

            internal virtual Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!IsAssignable(Type, expected))
                    parser.Error("COFLOW-FUNCTION-TYPE",
                        $"expression has type `{parser.FormatType(Type)}` but `{parser.FormatType(expected)}` is required");
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
            internal override void EmitCore(FunctionParser parser) =>
                parser.Emit(CoflowOpCode.Constant, parser.Constant(TemplateValue ?? Value));
        }

        private sealed record RetypedExpr(Expr Value, Type ExpectedType) : Expr(ExpectedType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.Reinterpret, 0, ExpectedType);
            }
        }

        private sealed record InterpolationPart(string? Text, Expr? Value);

        private sealed record InterpolatedStringExpr(
            IReadOnlyList<InterpolationPart> Parts) : Expr(typeof(string))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                var values = Parts.Where(part => part.Value is not null).ToArray();
                foreach (var part in values) part.Value!.Emit(parser);
                parser.Emit(CoflowOpCode.Native, parser.Constant(CoflowFormatting.Interpolation(
                    Parts.Select(part => part.Text).ToArray(),
                    Parts.Select(part => part.Value?.Type).ToArray(),
                    parser._metadata, parser._enums)));
            }
        }

        private sealed record ObjectExpr(
            ICoflowTypeMetadata Metadata,
            CfdLoadContext Context,
            IReadOnlyList<(string Name, Expr Value)> Fields) : Expr(Metadata.RuntimeType)
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsAssignableFrom(Type))
                    return base.WithExpected(expected, parser);
                return expected == Type ? this : new RetypedExpr(this, expected);
            }

            internal override void EmitCore(FunctionParser parser)
            {
                var supplied = Fields.ToDictionary(field => field.Name, StringComparer.Ordinal);
                foreach (var fieldMetadata in Metadata.Fields)
                {
                    var name = fieldMetadata.Name;
                    var fieldType = fieldMetadata.Binding.RuntimeType;
                    if (fieldMetadata.Binding.IsFunction) continue;
                    if (supplied.TryGetValue(name, out var field)) field.Value.Emit(parser);
                    else
                    {
                        var factory = Metadata.CreateVmDefaultFactory(name, Context);
                        parser.Emit(CoflowOpCode.Native,
                            parser.Constant(new CoflowNativeCall(factory)), fieldType);
                    }
                }
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(Metadata.CreateVmObjectFactory(Context))));
            }
        }

        private sealed record NoneExpr() : Expr(typeof(NoneMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Option<>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "`None` requires an Option result type");
                return new TypedNoneExpr(expected);
            }

            internal override void EmitCore(FunctionParser parser) =>
                throw new InvalidOperationException("None must be resolved against an expected type.");
        }

        private sealed record TypedNoneExpr(Type OptionType) : Expr(OptionType)
        {
            internal override void EmitCore(FunctionParser parser) =>
                parser.Emit(CoflowOpCode.MakeOptionNone, 0, OptionType);
        }

        private sealed record SomeExpr(Expr Value) : Expr(typeof(Option<>).MakeGenericType(Value.Type))
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Option<>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "`Some(value)` requires an Option result type");
                var inner = expected.GetGenericArguments()[0];
                return new SomeExpr(Value.WithExpected(inner, parser));
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.MakeOptionSome);
            }
        }

        private sealed record ResultBranchExpr(Expr Value, bool IsOk) : Expr(typeof(ResultBranchMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Result<,>))
                    parser.Error("COFLOW-FUNCTION-TYPE", $"`{(IsOk ? "Ok" : "Err")}(value)` requires a Result result type");
                var arguments = expected.GetGenericArguments();
                return new TypedResultBranchExpr(
                    Value.WithExpected(arguments[IsOk ? 0 : 1], parser),
                    IsOk,
                    expected);
            }

            internal override void EmitCore(FunctionParser parser) =>
                throw new InvalidOperationException("Result branch must be resolved against an expected type.");
        }

        private sealed record TypedResultBranchExpr(Expr Value, bool IsOk, Type ResultType) : Expr(ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                var arguments = ResultType.GetGenericArguments();
                parser.Emit(IsOk ? CoflowOpCode.MakeResultOk : CoflowOpCode.MakeResultErr);
            }
        }

        private sealed record ArgumentExpr(int Index, Type ArgumentType, string? Name = null) : Expr(ArgumentType)
        {
            internal override void EmitCore(FunctionParser parser) => parser.Emit(CoflowOpCode.Argument, Index);
        }

        private sealed record LocalExpr(int Index, Type LocalType, string? Name = null) : Expr(LocalType)
        {
            internal override void EmitCore(FunctionParser parser) => parser.Emit(CoflowOpCode.Local, Index);
        }

        private sealed record TypeIsExpr(Expr Value, Type TargetType, string? NarrowName) : Expr(typeof(bool))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.IsType, parser.Constant(TargetType), typeof(bool));
            }
        }

        private sealed record FunctionReferenceExpr(CoflowFunctionEntry Entry, Expr? Receiver) : Expr(DelegateType(Entry.Signature))
        {
            internal override CoflowFunctionSignature CallableSignature => Entry.Signature;
            internal override void EmitCore(FunctionParser parser)
            {
                if (Receiver is null)
                    parser.Emit(CoflowOpCode.Constant, parser.Constant(
                        new CoflowFunctionReferenceTemplate(Entry.Identity, null)));
                else
                {
                    Receiver.Emit(parser);
                    parser.Emit(CoflowOpCode.Native, parser.Constant(new CoflowFunctionReferenceTemplate(
                        Entry.Identity, Receiver.Type)));
                }
            }
        }

        private sealed record LambdaExpr(
            CoflowFunctionSignature Signature,
            IReadOnlyList<Expr> Captures,
            Expr Body) : Expr(DelegateType(Signature))
        {
            internal override CoflowFunctionSignature CallableSignature => Signature;

            internal override void EmitCore(FunctionParser parser)
            {
                foreach (var capture in Captures) capture.Emit(parser);
                var program = parser.CompileLambda(Signature, Captures, Body);
                parser.Emit(CoflowOpCode.MakeClosure,
                    parser.Constant(new CoflowClosureProgramTemplate(
                        program, Captures.Select(capture => capture.Type).ToArray())));
            }
        }

        private sealed class LambdaParseContext(
            int scopeBase,
            Dictionary<string, (int Index, Type Type)> parameters,
            int parameterCount)
        {
            private readonly Dictionary<string, int> _captureIndexes = new(StringComparer.Ordinal);
            private readonly List<Expr> _captures = new();

            internal int ScopeBase { get; } = scopeBase;
            internal Dictionary<string, (int Index, Type Type)> Parameters { get; } = parameters;
            internal IReadOnlyList<Expr> Captures => _captures;

            internal Expr Capture(string identity, Expr source)
            {
                if (!_captureIndexes.TryGetValue(identity, out var index))
                {
                    index = _captures.Count;
                    _captureIndexes.Add(identity, index);
                    _captures.Add(source);
                }
                return new ArgumentExpr(parameterCount + index, source.Type);
            }
        }

        private sealed record CallExpr(
            Expr Target,
            CoflowFunctionSignature Signature,
            IReadOnlyList<Expr> Arguments)
            : Expr(Signature.ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                if (Target is FunctionReferenceExpr direct)
                {
                    foreach (var argument in Arguments) argument.Emit(parser);
                    direct.Receiver?.Emit(parser);
                    parser.Emit(CoflowOpCode.Call,
                        parser.Constant(CoflowCallSite.From(direct.Entry,
                            Arguments.Count + (direct.Receiver is null ? 0 : 1))));
                    return;
                }
                Target.Emit(parser);
                foreach (var argument in Arguments) argument.Emit(parser);
                parser.Emit(CoflowOpCode.CallIndirect, Arguments.Count);
            }

            internal override void EmitTailCore(FunctionParser parser)
            {
                if (Target is FunctionReferenceExpr direct)
                {
                    foreach (var argument in Arguments) argument.Emit(parser);
                    direct.Receiver?.Emit(parser);
                    parser.Emit(CoflowOpCode.TailCall,
                        parser.Constant(CoflowCallSite.From(direct.Entry,
                            Arguments.Count + (direct.Receiver is null ? 0 : 1))));
                    return;
                }
                Target.Emit(parser);
                foreach (var argument in Arguments) argument.Emit(parser);
                parser.Emit(CoflowOpCode.TailCallIndirect, Arguments.Count);
            }
        }

        private sealed record EmptyArrayExpr() : Expr(typeof(ArrayMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "empty array requires an array expected type");
                return new ArrayExpr(Array.Empty<Expr>(), expected);
            }
            internal override void EmitCore(FunctionParser parser) =>
                throw new InvalidOperationException("empty array must be resolved against an expected type");
        }

        private sealed record EmptyDictionaryExpr() : Expr(typeof(DictionaryMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(IReadOnlyDictionary<,>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "empty dictionary requires a dictionary expected type");
                return new DictionaryExpr(Array.Empty<(Expr, Expr)>(), expected);
            }
            internal override void EmitCore(FunctionParser parser) =>
                throw new InvalidOperationException("empty dictionary must be resolved against an expected type");
        }

        private sealed class ArrayMarker { }
        private sealed class DictionaryMarker { }

        private sealed record ArrayExpr(IReadOnlyList<Expr> Values, Type ArrayType) : Expr(ArrayType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                foreach (var value in Values) value.Emit(parser);
                parser.Emit(CoflowOpCode.MakeArray, Values.Count);
            }
        }

        private sealed record DictionaryExpr(
            IReadOnlyList<(Expr Key, Expr Value)> Entries,
            Type DictionaryType) : Expr(DictionaryType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                foreach (var entry in Entries) { entry.Key.Emit(parser); entry.Value.Emit(parser); }
                parser.Emit(CoflowOpCode.MakeDictionary, Entries.Count);
            }
        }

        private sealed record IndexExpr(
            Expr Receiver,
            Expr Index,
            Type ResultType,
            bool IsDictionary)
            : Expr(ResultType)
        {
            internal static Expr Create(Expr receiver, Expr index, FunctionParser parser)
            {
                if (!receiver.Type.IsGenericType)
                    return Invalid();
                var definition = receiver.Type.GetGenericTypeDefinition();
                var arguments = receiver.Type.GetGenericArguments();
                if (definition == typeof(IReadOnlyList<>))
                {
                    index.WithExpected(typeof(long), parser);
                    return new IndexExpr(receiver, index,
                        typeof(Option<>).MakeGenericType(arguments[0]), false);
                }
                if (definition == typeof(IReadOnlyDictionary<,>))
                {
                    index.WithExpected(arguments[0], parser);
                    return new IndexExpr(receiver, index,
                        typeof(Option<>).MakeGenericType(arguments[1]), true);
                }
                return Invalid();

                Expr Invalid()
                {
                    parser.Error("COFLOW-FUNCTION-INDEX", $"`{parser.FormatType(receiver.Type)}` cannot be indexed");
                    return null!;
                }
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Receiver.Emit(parser);
                Index.Emit(parser);
                parser.Emit(IsDictionary ? CoflowOpCode.DictionaryIndex : CoflowOpCode.ArrayIndex);
            }
        }

        private sealed record FieldExpr(Expr Receiver, Type FieldType, CoflowFieldAccess Access)
            : Expr(FieldType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Receiver.Emit(parser);
                parser.Emit(CoflowOpCode.LoadField, parser.Constant(Access));
            }
        }

        private sealed record TransformExpr(Expr Receiver, Type ResultType, Delegate Transform)
            : Expr(ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Receiver.Emit(parser);
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(Transform)));
            }
        }

        private sealed record PropagateExpr(Expr Operand, Type ValueType) : Expr(ValueType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Operand.Emit(parser);
                parser.Emit(CoflowOpCode.Propagate);
            }
        }

        private sealed record MatchArm(MatchPattern Pattern, int? BindingLocal, Expr Body);

        private sealed record MatchExpr(
            Expr Subject,
            int SubjectLocal,
            IReadOnlyList<MatchArm> Arms,
            bool LastIsComplement) : Expr(Arms[0].Body.Type)
        {
            internal override bool AlwaysTerminates => Arms.All(arm => arm.Body.AlwaysTerminates);
            internal override Expr WithExpected(Type expected, FunctionParser parser) =>
                new MatchExpr(Subject, SubjectLocal,
                    Arms.Select(arm => arm with { Body = arm.Body.WithExpected(expected, parser) }).ToArray(),
                    LastIsComplement);

            internal override void EmitCore(FunctionParser parser)
            {
                Subject.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, SubjectLocal);
                var done = new List<int>();
                for (var index = 0; index < Arms.Count; index++)
                {
                    var arm = Arms[index];
                    int? next = null;
                    if (!(index == Arms.Count - 1 && (arm.Pattern.IsCatchAll || LastIsComplement)))
                    {
                        parser.Emit(CoflowOpCode.Local, SubjectLocal);
                        if (arm.Pattern.TagValue is { } tag)
                        {
                            parser.Emit(CoflowOpCode.ReadValueTag, 0, typeof(bool));
                            if (!tag) parser.Emit(CoflowOpCode.Not, 0, typeof(bool));
                        }
                        else if (arm.Pattern.TypeTarget is { } targetType)
                            parser.Emit(CoflowOpCode.IsType, parser.Constant(targetType), typeof(bool));
                        else
                        {
                            parser.Emit(CoflowOpCode.Constant,
                                parser.Constant(arm.Pattern.LiteralValue), Subject.Type);
                            var equal = CoflowValueShape.Scalar(Subject.Type) switch
                            {
                                CoflowRegisterKind.Integer => CoflowOpCode.EqualInteger,
                                CoflowRegisterKind.Float => CoflowOpCode.EqualFloat,
                                _ => CoflowOpCode.EqualReference,
                            };
                            parser.Emit(equal, 0, typeof(bool));
                        }
                        next = parser.Emit(CoflowOpCode.JumpIfFalse);
                    }
                    if (arm.BindingLocal is { } binding)
                    {
                        parser.Emit(CoflowOpCode.Local, SubjectLocal);
                        if (arm.Pattern.Payload is { } payload)
                            parser.Emit(payload == 0 ? CoflowOpCode.ReadFirstPayload : CoflowOpCode.ReadSecondPayload,
                                0, arm.Pattern.BindingType!);
                        else if (arm.Pattern.TypeTarget is not null)
                            parser.Emit(CoflowOpCode.Reinterpret, 0,
                                arm.Pattern.BindingType ?? Subject.Type);
                        parser.Emit(CoflowOpCode.StoreLocal, binding);
                    }
                    arm.Body.Emit(parser);
                    if (!arm.Body.AlwaysTerminates)
                        done.Add(parser.Emit(CoflowOpCode.Jump));
                    if (next is { } jump) parser.Patch(jump, parser._instructions.Count);
                }
                var end = parser._instructions.Count;
                foreach (var jump in done) parser.Patch(jump, end);
            }
        }

        private sealed record MatchPattern(
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
            internal override void EmitCore(FunctionParser parser)
            {
                Receiver.Emit(parser);
                foreach (var argument in Arguments) argument.Emit(parser);
                switch (Builtin.Kind)
                {
                    case CoflowBuiltinKind.CollectionCount:
                        parser.Emit(CoflowOpCode.CollectionCount, 0, Builtin.ResultType);
                        break;
                    case CoflowBuiltinKind.DictionaryKeys:
                        parser.Emit(CoflowOpCode.DictionaryKeys, 0, Builtin.ResultType);
                        break;
                    case CoflowBuiltinKind.DictionaryValues:
                        parser.Emit(CoflowOpCode.DictionaryValues, 0, Builtin.ResultType);
                        break;
                    case not CoflowBuiltinKind.Native:
                        parser.Emit(CoflowOpCode.CollectionBuiltin,
                            parser.Constant(Builtin), Builtin.ResultType);
                        break;
                    default:
                        parser.Emit(CoflowOpCode.Native,
                            parser.Constant(new CoflowNativeCall(Builtin.Invoke!)));
                        break;
                }
            }
        }

        private sealed record HigherOrderExpr(
            Expr Receiver,
            IReadOnlyList<Expr> Arguments,
            CoflowHigherOrderOperation Operation) : Expr(Operation.ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                var collection = parser._localCount++;
                var callable = parser._localCount++;
                var count = parser._localCount++;
                var index = parser._localCount++;
                var item = parser._localCount++;
                var result = parser._localCount++;
                var callbackResult = parser._localCount++;
                var fold = Operation.Name == "fold";

                Receiver.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, collection);
                if (fold)
                {
                    Arguments[0].Emit(parser);
                    parser.Emit(CoflowOpCode.StoreLocal, result);
                    Arguments[1].Emit(parser);
                }
                else Arguments[0].Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, callable);

                parser.Emit(CoflowOpCode.Local, collection);
                parser.Emit(CoflowOpCode.CollectionCount, 0, typeof(long));
                parser.Emit(CoflowOpCode.StoreLocal, count);

                if (Operation.Name is "map" or "filter")
                {
                    parser.Emit(CoflowOpCode.Local, count);
                    parser.Emit(CoflowOpCode.BeginArrayBuilder, 0, Operation.ResultType);
                    parser.Emit(CoflowOpCode.StoreLocal, result);
                }
                else if (Operation.Name == "find")
                {
                    parser.Emit(CoflowOpCode.MakeOptionNone, 0, Operation.ResultType);
                    parser.Emit(CoflowOpCode.StoreLocal, result);
                }
                else if (Operation.Name is "any" or "all")
                {
                    parser.Emit(CoflowOpCode.Constant,
                        parser.Constant(Operation.Name == "all"), typeof(bool));
                    parser.Emit(CoflowOpCode.StoreLocal, result);
                }
                parser.Emit(CoflowOpCode.Constant, parser.Constant(0L), typeof(long));
                parser.Emit(CoflowOpCode.StoreLocal, index);

                var condition = parser._instructions.Count;
                parser.Emit(CoflowOpCode.Local, index);
                parser.Emit(CoflowOpCode.Local, count);
                parser.Emit(CoflowOpCode.LessInt, 0, typeof(bool));
                var done = parser.Emit(CoflowOpCode.JumpIfFalse);

                parser.Emit(CoflowOpCode.Local, collection);
                parser.Emit(CoflowOpCode.Local, index);
                parser.Emit(CoflowOpCode.ArrayItem, 0, Operation.ElementType);
                parser.Emit(CoflowOpCode.StoreLocal, item);

                parser.Emit(CoflowOpCode.Local, callable);
                if (fold) parser.Emit(CoflowOpCode.Local, result);
                parser.Emit(CoflowOpCode.Local, item);
                var callbackType = Operation.Name == "map" || fold
                    ? Operation.OutputElementType : typeof(bool);
                parser.Emit(CoflowOpCode.CallIndirect, fold ? 2 : 1, callbackType);
                parser.Emit(CoflowOpCode.StoreLocal, callbackResult);

                int? earlyDone = null;
                if (Operation.Name == "map")
                {
                    parser.Emit(CoflowOpCode.Local, result);
                    parser.Emit(CoflowOpCode.Local, callbackResult);
                    parser.Emit(CoflowOpCode.AppendArrayBuilder, 0, typeof(Unit));
                    parser.Emit(CoflowOpCode.Pop);
                }
                else if (Operation.Name == "filter")
                {
                    parser.Emit(CoflowOpCode.Local, callbackResult);
                    var skip = parser.Emit(CoflowOpCode.JumpIfFalse);
                    parser.Emit(CoflowOpCode.Local, result);
                    parser.Emit(CoflowOpCode.Local, item);
                    parser.Emit(CoflowOpCode.AppendArrayBuilder, 0, typeof(Unit));
                    parser.Emit(CoflowOpCode.Pop);
                    parser.Patch(skip, parser._instructions.Count);
                }
                else if (fold)
                {
                    parser.Emit(CoflowOpCode.Local, callbackResult);
                    parser.Emit(CoflowOpCode.StoreLocal, result);
                }
                else
                {
                    parser.Emit(CoflowOpCode.Local, callbackResult);
                    if (Operation.Name == "all") parser.Emit(CoflowOpCode.Not, 0, typeof(bool));
                    var keepGoing = parser.Emit(CoflowOpCode.JumpIfFalse);
                    if (Operation.Name == "find")
                    {
                        parser.Emit(CoflowOpCode.Local, item);
                        parser.Emit(CoflowOpCode.MakeOptionSome, 0, Operation.ResultType);
                    }
                    else parser.Emit(CoflowOpCode.Constant,
                        parser.Constant(Operation.Name == "any"), typeof(bool));
                    parser.Emit(CoflowOpCode.StoreLocal, result);
                    earlyDone = parser.Emit(CoflowOpCode.Jump);
                    parser.Patch(keepGoing, parser._instructions.Count);
                }

                parser.Emit(CoflowOpCode.Local, index);
                parser.Emit(CoflowOpCode.Constant, parser.Constant(1L), typeof(long));
                parser.Emit(CoflowOpCode.AddInt, 0, typeof(long));
                parser.Emit(CoflowOpCode.StoreLocal, index);
                parser.Emit(CoflowOpCode.Jump, condition);
                var end = parser._instructions.Count;
                parser.Patch(done, end);
                if (earlyDone is { } jump) parser.Patch(jump, end);

                parser.Emit(CoflowOpCode.Local, result);
            }
        }

        private sealed record StoreLocalExpr(int Index, Expr Value) : Expr(typeof(Unit))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, Index);
            }
        }

        private sealed record AssignLocalExpr(int Index, Expr Value) : Expr(typeof(Unit))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, Index);
                parser.Emit(CoflowOpCode.Constant, parser.Constant(Unit.Value));
            }
        }

        private sealed record ReturnExpr(Expr Value) : Expr(typeof(Unit))
        {
            internal override bool AlwaysTerminates => true;
            internal override void EmitCore(FunctionParser parser)
            {
                Value.EmitTail(parser);
            }
        }

        private sealed record LoopControlExpr(bool IsBreak) : Expr(typeof(Unit))
        {
            internal override bool AlwaysTerminates => true;
            internal override void EmitCore(FunctionParser parser)
            {
                if (parser._loops.Count == 0)
                    throw new InvalidOperationException("loop control emitted outside a loop");
                var loop = parser._loops.Peek();
                var jump = parser.Emit(CoflowOpCode.Jump, IsBreak ? 0 : loop.ContinueTarget);
                if (IsBreak) loop.BreakJumps.Add(jump);
                else if (loop.ContinueTarget < 0) loop.ContinueJumps.Add(jump);
            }
        }

        private sealed record WhileExpr(Expr Condition, Expr Body) : Expr(typeof(Unit))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                var start = parser._instructions.Count;
                Condition.Emit(parser);
                var done = parser.Emit(CoflowOpCode.JumpIfFalse);
                var loop = new LoopEmitContext(start);
                parser._loops.Push(loop);
                Body.Emit(parser);
                parser._loops.Pop();
                if (!Body.AlwaysTerminates)
                {
                    parser.Emit(CoflowOpCode.Pop);
                    parser.Emit(CoflowOpCode.Jump, start);
                }
                var end = parser._instructions.Count;
                parser.Patch(done, end);
                foreach (var jump in loop.BreakJumps) parser.Patch(jump, end);
            }
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
            internal override void EmitCore(FunctionParser parser)
            {
                Collection.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, CollectionLocal);
                parser.Emit(CoflowOpCode.Constant, parser.Constant(0L), typeof(long));
                parser.Emit(CoflowOpCode.StoreLocal, IndexLocal);
                var condition = parser._instructions.Count;
                parser.Emit(CoflowOpCode.Local, IndexLocal);
                parser.Emit(CoflowOpCode.Local, CollectionLocal);
                parser.Emit(CoflowOpCode.CollectionCount, 0, typeof(long));
                parser.Emit(CoflowOpCode.LessInt, 0, typeof(bool));
                var done = parser.Emit(CoflowOpCode.JumpIfFalse);

                parser.Emit(CoflowOpCode.Local, CollectionLocal);
                parser.Emit(CoflowOpCode.Local, IndexLocal);
                parser.Emit(IsArray ? CoflowOpCode.ArrayItem : CoflowOpCode.DictionaryKey, 0, FirstType);
                parser.Emit(CoflowOpCode.StoreLocal, FirstLocal);
                if (SecondLocal is { } second)
                {
                    if (IsArray)
                    {
                        parser.Emit(CoflowOpCode.Local, IndexLocal);
                    }
                    else
                    {
                        parser.Emit(CoflowOpCode.Local, CollectionLocal);
                        parser.Emit(CoflowOpCode.Local, IndexLocal);
                        parser.Emit(CoflowOpCode.DictionaryValue, 0, SecondType!);
                    }
                    parser.Emit(CoflowOpCode.StoreLocal, second);
                }

                var loop = new LoopEmitContext(-1);
                parser._loops.Push(loop);
                Body.Emit(parser);
                parser._loops.Pop();
                if (!Body.AlwaysTerminates) parser.Emit(CoflowOpCode.Pop);
                if (!Body.AlwaysTerminates || loop.ContinueJumps.Count != 0)
                {
                    var increment = parser._instructions.Count;
                    loop.ContinueTarget = increment;
                    foreach (var jump in loop.ContinueJumps) parser.Patch(jump, increment);
                    parser.Emit(CoflowOpCode.Local, IndexLocal);
                    parser.Emit(CoflowOpCode.Constant, parser.Constant(1L), typeof(long));
                    parser.Emit(CoflowOpCode.AddInt, 0, typeof(long));
                    parser.Emit(CoflowOpCode.StoreLocal, IndexLocal);
                    parser.Emit(CoflowOpCode.Jump, condition);
                }
                var end = parser._instructions.Count;
                parser.Patch(done, end);
                foreach (var jump in loop.BreakJumps) parser.Patch(jump, end);
            }
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
            internal override void EmitCore(FunctionParser parser)
            {
                Start.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, ValueLocal);
                End.Emit(parser);
                parser.Emit(CoflowOpCode.StoreLocal, EndLocal);
                if (IndexLocal is { } index)
                {
                    parser.Emit(CoflowOpCode.Constant, parser.Constant(0L), typeof(long));
                    parser.Emit(CoflowOpCode.StoreLocal, index);
                }

                var condition = parser._instructions.Count;
                parser.Emit(CoflowOpCode.Local, ValueLocal, typeof(long));
                parser.Emit(CoflowOpCode.Local, EndLocal, typeof(long));
                parser.Emit(Inclusive ? CoflowOpCode.LessOrEqualInt : CoflowOpCode.LessInt,
                    0, typeof(bool));
                var done = parser.Emit(CoflowOpCode.JumpIfFalse);

                var loop = new LoopEmitContext(-1);
                parser._loops.Push(loop);
                Body.Emit(parser);
                parser._loops.Pop();
                if (!Body.AlwaysTerminates) parser.Emit(CoflowOpCode.Pop);
                int? inclusiveDone = null;
                if (!Body.AlwaysTerminates || loop.ContinueJumps.Count != 0)
                {
                    var increment = parser._instructions.Count;
                    loop.ContinueTarget = increment;
                    foreach (var jump in loop.ContinueJumps) parser.Patch(jump, increment);

                    if (Inclusive)
                    {
                        parser.Emit(CoflowOpCode.Local, ValueLocal, typeof(long));
                        parser.Emit(CoflowOpCode.Local, EndLocal, typeof(long));
                        parser.Emit(CoflowOpCode.EqualInteger, 0, typeof(bool));
                        var continueIncrement = parser.Emit(CoflowOpCode.JumpIfFalse);
                        inclusiveDone = parser.Emit(CoflowOpCode.Jump);
                        parser.Patch(continueIncrement, parser._instructions.Count);
                    }

                    parser.Emit(CoflowOpCode.Local, ValueLocal, typeof(long));
                    parser.Emit(CoflowOpCode.Constant, parser.Constant(1L), typeof(long));
                    parser.Emit(CoflowOpCode.AddInt, 0, typeof(long));
                    parser.Emit(CoflowOpCode.StoreLocal, ValueLocal);
                    if (IndexLocal is { } indexLocal)
                    {
                        parser.Emit(CoflowOpCode.Local, indexLocal, typeof(long));
                        parser.Emit(CoflowOpCode.Constant, parser.Constant(1L), typeof(long));
                        parser.Emit(CoflowOpCode.AddInt, 0, typeof(long));
                        parser.Emit(CoflowOpCode.StoreLocal, indexLocal);
                    }
                    parser.Emit(CoflowOpCode.Jump, condition);
                }
                var end = parser._instructions.Count;
                parser.Patch(done, end);
                if (inclusiveDone is { } completed) parser.Patch(completed, end);
                foreach (var jump in loop.BreakJumps) parser.Patch(jump, end);
            }
        }

        private sealed record RangeExpr(Expr Start, Expr End, bool Inclusive) : Expr(typeof(RangeExpr))
        {
            internal override void EmitCore(FunctionParser parser) =>
                parser.Error("COFLOW-FUNCTION-TYPE", "range expressions can only be used by a for loop");
        }

        private sealed class LoopEmitContext(int continueTarget)
        {
            internal int ContinueTarget { get; set; } = continueTarget;
            internal List<int> BreakJumps { get; } = new();
            internal List<int> ContinueJumps { get; } = new();
        }

        private sealed record DiscardExpr(Expr Value) : Expr(typeof(Unit))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.Pop);
            }
        }

        private sealed record BlockExpr(IReadOnlyList<Expr> Statements, Expr Result) : Expr(Result.Type)
        {
            internal override bool AlwaysTerminates =>
                Statements.Any(statement => statement.AlwaysTerminates) || Result.AlwaysTerminates;
            internal override Expr WithExpected(Type expected, FunctionParser parser) =>
                new BlockExpr(Statements, Result.WithExpected(expected, parser));

            internal override void EmitCore(FunctionParser parser)
            {
                foreach (var statement in Statements)
                {
                    statement.Emit(parser);
                    if (statement.AlwaysTerminates) return;
                }
                Result.Emit(parser);
            }


            internal override void EmitTailCore(FunctionParser parser)
            {
                foreach (var statement in Statements)
                {
                    statement.Emit(parser);
                    if (statement.AlwaysTerminates) return;
                }
                Result.EmitTail(parser);
            }
        }

        private sealed record IfExpr(Expr Condition, Expr WhenTrue, Expr WhenFalse) : Expr(WhenTrue.Type)
        {
            internal static Expr Create(Expr condition, Expr whenTrue, Expr whenFalse) =>
                condition is ConstantExpr { Value: bool value }
                    ? value ? whenTrue : whenFalse
                    : new IfExpr(condition, whenTrue, whenFalse);

            internal override bool AlwaysTerminates => WhenTrue.AlwaysTerminates && WhenFalse.AlwaysTerminates;
            internal override Expr WithExpected(Type expected, FunctionParser parser) =>
                new IfExpr(
                    Condition,
                    WhenTrue.WithExpected(expected, parser),
                    WhenFalse.WithExpected(expected, parser));

            internal override void EmitCore(FunctionParser parser)
            {
                Condition.Emit(parser);
                var otherwise = parser.Emit(CoflowOpCode.JumpIfFalse);
                WhenTrue.Emit(parser);
                var done = WhenTrue.AlwaysTerminates
                    ? (int?)null
                    : parser.Emit(CoflowOpCode.Jump);
                parser.Patch(otherwise, parser._instructions.Count);
                WhenFalse.Emit(parser);
                if (done is { } jump) parser.Patch(jump, parser._instructions.Count);
            }

            internal override void EmitTailCore(FunctionParser parser)
            {
                Condition.Emit(parser);
                var otherwise = parser.Emit(CoflowOpCode.JumpIfFalse);
                WhenTrue.EmitTail(parser);
                parser.Patch(otherwise, parser._instructions.Count);
                WhenFalse.EmitTail(parser);
            }
        }

        private static class ValueFactories
        {

            internal static CoflowHigherOrderOperation HigherOrder(
                string name, Type element, Type outputElement, Type resultType) => new(
                    name,
                    element,
                    outputElement,
                    resultType);
            internal static MatchPattern MatchNone(Type subject, FunctionParser parser)
            {
                if (!subject.IsGenericType || subject.GetGenericTypeDefinition() != typeof(Option<>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "None pattern requires Option");
                return new MatchPattern("None", false, null, null, TagValue: false);
            }
            internal static MatchPattern MatchBranch(Type subject, string kind, string binding, FunctionParser parser)
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
                parser.Error("COFLOW-FUNCTION-TYPE", $"{kind} pattern does not match subject type");
                return null!;
            }

            private static System.Reflection.MethodInfo Method(string name) =>
                typeof(ValueFactories).GetMethod(name,
                    System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
            private static Delegate Closed(System.Reflection.MethodInfo method, params Type[] arguments)
            {
                var closed = method.MakeGenericMethod(arguments);
                var signature = closed.GetParameters().Select(parameter => parameter.ParameterType)
                    .Append(closed.ReturnType).ToArray();
                return closed.CreateDelegate(System.Linq.Expressions.Expression.GetDelegateType(signature));
            }
        }

        private sealed record UnaryExpr(string Operation, Expr Operand, Type ResultType, CoflowOpCode Code) : Expr(ResultType)
        {
            internal static Expr Create(string operation, Expr operand, FunctionParser parser)
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
                    return new UnaryExpr(operation, operand, typeof(bool), CoflowOpCode.Not);
                if (operation == "-" && operand.Type == typeof(long))
                    return new UnaryExpr(operation, operand, typeof(long), CoflowOpCode.NegateInt);
                if (operation == "-" && operand.Type == typeof(double))
                    return new UnaryExpr(operation, operand, typeof(double), CoflowOpCode.NegateFloat);
                if (operation == "~" && operand.Type == typeof(long))
                    return new UnaryExpr(operation, operand, typeof(long), CoflowOpCode.BitNot);
                if (operation == "~" && operand.Type.IsEnum)
                {
                    var metadata = parser.EnumMetadata(operand.Type);
                    if (!metadata.IsFlags)
                        parser.Error("COFLOW-FUNCTION-TYPE", "`~` requires a flag enum");
                    return new EnumUnaryExpr(operand, metadata);
                }
                parser.Error("COFLOW-FUNCTION-TYPE",
                    $"operator `{operation}` cannot be applied to `{parser.FormatType(operand.Type)}`");
                return null!;
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Operand.Emit(parser);
                parser.Emit(Code);
            }
        }

        private sealed record EnumUnaryExpr(Expr Operand, ICoflowEnumMetadata Metadata) : Expr(Operand.Type)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Operand.Emit(parser);
                parser.Emit(CoflowOpCode.BitNot);
            }
        }

        private sealed record EnumBinaryExpr(
            string Operation,
            Expr Left,
            Expr Right,
            Type ResultType,
            CoflowOpCode Code) : Expr(ResultType)
        {
            internal static Expr Create(
                string operation,
                Expr left,
                Expr right,
                FunctionParser parser,
                ICoflowEnumMetadata metadata)
            {
                if (left.Type != right.Type || !left.Type.IsEnum)
                    parser.Error("COFLOW-FUNCTION-TYPE", "enum operators require the same enum type");
                if (operation is "&" or "|" or "^")
                {
                    if (!metadata.IsFlags)
                        parser.Error("COFLOW-FUNCTION-TYPE", "bit operators require a flag enum");
                    return new EnumBinaryExpr(operation, left, right, left.Type, operation switch
                    { "&" => CoflowOpCode.BitAnd, "|" => CoflowOpCode.BitOr, _ => CoflowOpCode.BitXor });
                }
                if (operation is "==" or "!=")
                    return new EqualityExpr(left, right, operation == "!=", parser._metadata);
                if (operation is "<" or "<=" or ">" or ">=")
                    return new EnumBinaryExpr(operation, left, right, typeof(bool), operation switch
                    { "<" => CoflowOpCode.LessInt, "<=" => CoflowOpCode.LessOrEqualInt,
                        ">" => CoflowOpCode.GreaterInt, _ => CoflowOpCode.GreaterOrEqualInt });
                parser.Error("COFLOW-FUNCTION-TYPE", $"operator `{operation}` cannot be applied to enum");
                return null!;
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Left.Emit(parser);
                Right.Emit(parser);
                parser.Emit(Code);
            }
        }

        private sealed record ConversionExpr(Expr Value, Type ResultType, CoflowOpCode Code)
            : Expr(ResultType)
        {
            internal static Expr Create(Expr value, Type resultType, CoflowOpCode code)
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
                return new ConversionExpr(value, resultType, code);
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(Code);
            }
        }

        private sealed record BinaryExpr(
            string Operation,
            Expr Left,
            Expr Right,
            Type ResultType,
            CoflowOpCode Code) : Expr(ResultType)
        {
            internal static Expr Create(string operation, Expr left, Expr right, FunctionParser parser)
            {
                if (left.Type != right.Type)
                    parser.Error("COFLOW-FUNCTION-TYPE",
                        $"operator `{operation}` requires equal operand types, found `{parser.FormatType(left.Type)}` and `{parser.FormatType(right.Type)}`");
                if (operation is "&&" or "||")
                {
                    if (left.Type != typeof(bool)) parser.Error("COFLOW-FUNCTION-TYPE", $"operator `{operation}` requires bool operands");
                    if (left is ConstantExpr { Value: bool leftBoolean })
                    {
                        if (operation == "&&") return leftBoolean ? right : left;
                        return leftBoolean ? left : right;
                    }
                    return new BinaryExpr(operation, left, right, typeof(bool),
                        operation == "&&" ? CoflowOpCode.JumpIfFalseKeep : CoflowOpCode.JumpIfTrueKeep);
                }
                if (operation is "==" or "!=")
                {
                    if (!parser.SupportsEquality(left.Type))
                        parser.Error("COFLOW-FUNCTION-TYPE", "function values cannot be compared");
                    return new EqualityExpr(left, right, operation == "!=", parser._metadata);
                }
                var code = SelectCode(operation, left.Type, parser);
                var result = operation is "<" or "<=" or ">" or ">=" ? typeof(bool) : left.Type;
                if (left is ConstantExpr leftConstant && right is ConstantExpr rightConstant &&
                    TryFold(operation, leftConstant.Value, rightConstant.Value, out var folded))
                    return new ConstantExpr(folded, result);
                return new BinaryExpr(operation, left, right, result, code);
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

            private static CoflowOpCode SelectCode(string operation, Type type, FunctionParser parser)
            {
                if (type == typeof(long)) return operation switch
                {
                    "+" => CoflowOpCode.AddInt, "-" => CoflowOpCode.SubtractInt,
                    "*" => CoflowOpCode.MultiplyInt, "/" => CoflowOpCode.DivideInt,
                    "//" => CoflowOpCode.IntegerDivide, "%" => CoflowOpCode.Remainder,
                    "**" => CoflowOpCode.PowerInt,
                    "<<" => CoflowOpCode.ShiftLeft, ">>" => CoflowOpCode.ShiftRight,
                    "&" => CoflowOpCode.BitAnd, "^" => CoflowOpCode.BitXor, "|" => CoflowOpCode.BitOr,
                    "<" => CoflowOpCode.LessInt, "<=" => CoflowOpCode.LessOrEqualInt,
                    ">" => CoflowOpCode.GreaterInt, ">=" => CoflowOpCode.GreaterOrEqualInt,
                    _ => Invalid(),
                };
                if (type == typeof(double)) return operation switch
                {
                    "+" => CoflowOpCode.AddFloat, "-" => CoflowOpCode.SubtractFloat,
                    "*" => CoflowOpCode.MultiplyFloat, "/" => CoflowOpCode.DivideFloat,
                    "**" => CoflowOpCode.PowerFloat,
                    "<" => CoflowOpCode.LessFloat, "<=" => CoflowOpCode.LessOrEqualFloat,
                    ">" => CoflowOpCode.GreaterFloat, ">=" => CoflowOpCode.GreaterOrEqualFloat,
                    _ => Invalid(),
                };
                if (type == typeof(string)) return operation switch
                {
                    "+" => CoflowOpCode.AddString,
                    "<" => CoflowOpCode.LessString, "<=" => CoflowOpCode.LessOrEqualString,
                    ">" => CoflowOpCode.GreaterString, ">=" => CoflowOpCode.GreaterOrEqualString,
                    _ => Invalid(),
                };
                return Invalid();

                CoflowOpCode Invalid()
                {
                    parser.Error("COFLOW-FUNCTION-TYPE",
                        $"operator `{operation}` cannot be applied to `{parser.FormatType(type)}`");
                    return default;
                }
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Left.Emit(parser);
                if (Operation is "&&" or "||")
                {
                    var jump = parser.Emit(Code);
                    Right.Emit(parser);
                    parser.Patch(jump, parser._instructions.Count);
                    return;
                }
                Right.Emit(parser);
                parser.Emit(Code);
            }
        }

        private sealed record EqualityExpr(
            Expr Left,
            Expr Right,
            bool Negated,
            IReadOnlyDictionary<string, ICoflowTypeMetadata> Metadata) : Expr(typeof(bool))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Left.Emit(parser);
                Right.Emit(parser);
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(CoflowEquality.Create(Left.Type, Metadata))));
                if (Negated) parser.Emit(CoflowOpCode.Not, 0, typeof(bool));
            }
        }

        private sealed record ComparisonChainExpr(
            IReadOnlyList<Expr> Operands,
            IReadOnlyList<string> Operations,
            IReadOnlyList<int> Locals,
            IReadOnlyList<CoflowOpCode> Codes) : Expr(typeof(bool))
        {
            internal static Expr Create(
                Expr first,
                Expr second,
                string firstOperation,
                string secondOperation,
                Expr third,
                FunctionParser parser)
            {
                var firstComparison = (BinaryExpr)BinaryExpr.Create(firstOperation, first, second, parser);
                var secondComparison = (BinaryExpr)BinaryExpr.Create(secondOperation, second, third, parser);
                return new ComparisonChainExpr(
                    new[] { first, second, third },
                    new[] { firstOperation, secondOperation },
                    new[] { parser._localCount++, parser._localCount++, parser._localCount++ },
                    new[] { firstComparison.Code, secondComparison.Code });
            }

            internal ComparisonChainExpr Append(string operation, Expr operand, FunctionParser parser)
            {
                var comparison = (BinaryExpr)BinaryExpr.Create(operation, Operands[^1], operand, parser);
                return new ComparisonChainExpr(
                    Operands.Append(operand).ToArray(),
                    Operations.Append(operation).ToArray(),
                    Locals.Append(parser._localCount++).ToArray(),
                    Codes.Append(comparison.Code).ToArray());
            }

            internal override void EmitCore(FunctionParser parser)
            {
                for (var index = 0; index < Operands.Count; index++)
                {
                    Operands[index].Emit(parser);
                    parser.Emit(CoflowOpCode.StoreLocal, Locals[index]);
                }
                var shortCircuits = new List<int>();
                for (var index = 0; index < Codes.Count; index++)
                {
                    parser.Emit(CoflowOpCode.Local, Locals[index]);
                    parser.Emit(CoflowOpCode.Local, Locals[index + 1]);
                    parser.Emit(Codes[index]);
                    if (index + 1 < Codes.Count)
                        shortCircuits.Add(parser.Emit(CoflowOpCode.JumpIfFalseKeep));
                }
                var end = parser._instructions.Count;
                foreach (var jump in shortCircuits) parser.Patch(jump, end);
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
