namespace CoflowRuntime;

using System.Globalization;
using System.Diagnostics.CodeAnalysis;

internal static class CoflowCompiler
{
    internal static void Compile(
        IReadOnlyList<CoflowFunctionSlot> slots,
        ICoflowGeneratedContract module,
        IReadOnlyDictionary<(string DeclaredType, string Key), object> records,
        CfdLoadContext context,
        CoflowGenerationStorage storage)
    {
        var compiled = new List<(CoflowFunctionSlot Slot, CoflowProgram? Body)>();
        var diagnostics = new List<CfdDiagnostic>();
        foreach (var slot in slots)
        {
            if (slot.Source is null)
            {
                if (slot.RequiresCfdBody)
                {
                    diagnostics.Add(new CfdDiagnostic(
                        "COFLOW-FUNCTION-MISSING",
                        $"{slot.Identity.DeclaredType}.{slot.Identity.RecordKey}.{slot.Identity.FieldName}: ordinary functions require a CFD body",
                        slot.SourcePath,
                        slot.SourceSpan));
                    continue;
                }
                compiled.Add((slot, null));
                continue;
            }
            try
            {
                compiled.Add((slot, new FunctionParser(slot, module, records, context, storage).Parse()));
            }
            catch (FunctionCompileException error)
            {
                diagnostics.Add(new CfdDiagnostic(
                    error.Code,
                    $"{slot.Identity.DeclaredType}.{slot.Identity.RecordKey}.{slot.Identity.FieldName}: {error.Message}",
                    slot.SourcePath,
                    error.Offset is { } offset
                        ? FunctionSpan(slot.Source, offset)
                        : slot.Source.Span));
            }
        }
        if (diagnostics.Count != 0) throw new CoflowLoadException(diagnostics);
        foreach (var item in compiled) item.Slot.PublishCompiled(item.Body);
    }

    private static CfdSpan FunctionSpan(CfdFunctionValue function, int offset)
    {
        var line = function.Span.StartLine;
        var column = function.Span.StartColumn;
        var length = Math.Min(Math.Max(offset, 0), function.Source.Length);
        for (var index = 0; index < length; index++)
        {
            if (function.Source[index] == '\n')
            {
                line++;
                column = 1;
            }
            else
            {
                column++;
            }
        }
        return new CfdSpan(line, column, line, column + (length < function.Source.Length ? 1 : 0));
    }

    private sealed class FunctionCompileException : Exception
    {
        internal FunctionCompileException(string code, string message, int? offset = null) : base(message)
        {
            Code = code;
            Offset = offset;
        }
        internal string Code { get; }
        internal int? Offset { get; }
    }

    private enum TokenKind
    {
        End,
        Identifier,
        Integer,
        Float,
        String,
        InterpolatedStringStart,
        InterpolationStart,
        InterpolationEnd,
        InterpolatedStringEnd,
        LeftParen,
        RightParen,
        LeftBrace,
        RightBrace,
        LeftBracket,
        RightBracket,
        Less,
        Greater,
        Comma,
        Colon,
        Equal,
        Semicolon,
        Arrow,
        DoubleColon,
        Ampersand,
        Plus,
        Minus,
        Star,
        Slash,
        DoubleSlash,
        Percent,
        Tilde,
        Pipe,
        Caret,
        Power,
        ShiftLeft,
        ShiftRight,
        DotDot,
        DotDotEqual,
        Bang,
        EqualEqual,
        BangEqual,
        LessEqual,
        GreaterEqual,
        PlusEqual,
        MinusEqual,
        StarEqual,
        SlashEqual,
        AndAnd,
        OrOr,
        Dot,
        Dollar,
        Question,
        FatArrow,
    }

    private readonly record struct Token(TokenKind Kind, string Text, int Offset);

    private sealed class FunctionParser
    {
        private const int MaximumParseDepth = 128;
        private readonly CoflowFunctionSlot _slot;
        private readonly IReadOnlyDictionary<(string DeclaredType, string Key), object> _records;
        private readonly IReadOnlyDictionary<string, ICoflowTypeMetadata> _metadata;
        private readonly IReadOnlyDictionary<string, ICoflowEnumMetadata> _enums;
        private readonly IReadOnlyDictionary<string, CoflowConstant> _declaredConstants;
        private readonly CfdLoadContext _context;
        private readonly CoflowGenerationStorage _storage;
        private readonly HashSet<string> _ownerFieldNames;
        private readonly Dictionary<Type, string> _generatedNames;
        private readonly CfdNameResolver _names;
        private readonly List<Token> _tokens;
        private readonly List<CoflowInstruction> _instructions = new();
        private readonly List<CfdSpan?> _instructionSpans = new();
        private readonly List<object?> _constants = new();
        private readonly Dictionary<string, (int Index, Type Type)> _parameters = new(StringComparer.Ordinal);
        private readonly List<Dictionary<string, (int Index, Type Type)>> _localScopes = new();
        private readonly Stack<LambdaParseContext> _lambdaContexts = new();
        private readonly Stack<IReadOnlyDictionary<string, Type>> _narrowings = new();
        private readonly Stack<LoopEmitContext> _loops = new();
        private readonly HashSet<int> _mutableLocals = new();
        private Type _returnType = typeof(Unit);
        private int _loopParseDepth;
        private int _localCount;
        private int _index;
        private int _expressionDepth;
        private int _binaryDepth;
        private int _unaryDepth;

        internal FunctionParser(
            CoflowFunctionSlot slot,
            ICoflowGeneratedContract module,
            IReadOnlyDictionary<(string DeclaredType, string Key), object> records,
            CfdLoadContext context,
            CoflowGenerationStorage storage)
        {
            _slot = slot;
            _records = records;
            _context = context;
            _storage = storage;
            _metadata = module.Types.ToDictionary(item => item.DeclaredType, StringComparer.Ordinal);
            _enums = module.Enums.ToDictionary(item => item.DeclaredType, StringComparer.Ordinal);
            _declaredConstants = module.Constants.ToDictionary(
                item => item.DeclaredName, StringComparer.Ordinal);
            _ownerFieldNames = _metadata.TryGetValue(slot.Identity.DeclaredType, out var owner)
                ? owner.FieldNames.ToHashSet(StringComparer.Ordinal)
                : new HashSet<string>(StringComparer.Ordinal);
            _generatedNames = module.Types
                .Select(item => (item.RuntimeType, item.DeclaredType))
                .Concat(module.Enums.Select(item => (item.RuntimeType, item.DeclaredType)))
                .ToDictionary(item => item.RuntimeType, item => item.DeclaredType);
            _names = slot.Names;
            _tokens = Lex(slot.Source!.Source);
        }

        internal CoflowProgram Parse()
        {
            ExpectIdentifier("fn");
            Expect(TokenKind.LeftParen, "expected `(` after `fn`");
            var parameterIndex = 0;
            if (!Match(TokenKind.RightParen))
            {
                do
                {
                    var name = ExpectBindingIdentifier("expected a parameter name").Text;
                    Expect(TokenKind.Colon, "expected `:` after the parameter name");
                    var declaredName = ParseTypeName();
                    if (parameterIndex >= _slot.Signature.ParameterTypes.Count)
                        Error("COFLOW-FUNCTION-SIGNATURE", "CFD function declares too many parameters");
                    var declared = ResolveTypeName(declaredName);
                    var expectedType = _slot.Signature.ParameterTypes[parameterIndex];
                    var expected = FormatType(expectedType);
                    if (declared != expectedType)
                        Error("COFLOW-FUNCTION-SIGNATURE",
                            $"parameter `{name}` has type `{declaredName}` but CFT requires `{expected}`");
                    if (!_parameters.TryAdd(name, (parameterIndex, expectedType)))
                        Error("COFLOW-FUNCTION-NAME", $"parameter `{name}` is declared more than once");
                    parameterIndex++;
                } while (Match(TokenKind.Comma));
                Expect(TokenKind.RightParen, "expected `)` after function parameters");
            }
            if (parameterIndex != _slot.Signature.ParameterTypes.Count)
                Error("COFLOW-FUNCTION-SIGNATURE",
                    $"CFD function declares {parameterIndex} parameters but CFT requires {_slot.Signature.ParameterTypes.Count}");
            Expect(TokenKind.Arrow, "expected `->` after function parameters");
            var resultName = ParseTypeName();
            var expectedResult = FormatType(_slot.Signature.ResultType);
            _returnType = _slot.Signature.ResultType;
            if (ResolveTypeName(resultName) != _slot.Signature.ResultType)
                Error("COFLOW-FUNCTION-SIGNATURE",
                    $"function returns `{resultName}` but CFT requires `{expectedResult}`");
            Expect(TokenKind.LeftBrace, "expected a function body");
            var expression = ParseBlockContents().WithExpected(_slot.Signature.ResultType, this);
            Expect(TokenKind.End, "unexpected content after the function body");
            if (expression.Type != _slot.Signature.ResultType && !expression.AlwaysTerminates)
                Error("COFLOW-FUNCTION-RETURN",
                    $"body has type `{FormatType(expression.Type)}` but function returns `{expectedResult}`");
            expression.EmitTail(this);
            return new CoflowProgram(
                _slot.Identity,
                _slot.SourcePath,
                _slot.SourceSpan,
                _instructions,
                _instructionSpans,
                _constants,
                parameterIndex,
                _localCount);
        }

        private Expr ParseBlockContents(Dictionary<string, (int Index, Type Type)>? initialScope = null)
        {
            _localScopes.Add(initialScope ?? new Dictionary<string, (int Index, Type Type)>(StringComparer.Ordinal));
            var statements = new List<Expr>();
            Expr? result = null;
            try
            {
                while (!Match(TokenKind.RightBrace))
                {
                    if (Peek().Kind == TokenKind.End)
                        Error("COFLOW-FUNCTION-SYNTAX", "unterminated block");
                    if (Peek().Kind == TokenKind.Identifier && Peek().Text == "var")
                    {
                        Advance();
                        statements.Add(ParseVariable());
                        Expect(TokenKind.Semicolon, "expected `;` after local variable declaration");
                        continue;
                    }
                    if (Peek().Kind == TokenKind.Identifier && Peek().Text == "return")
                    {
                        Advance();
                        var value = ParseExpression().WithExpected(_returnType, this);
                        Expect(TokenKind.Semicolon, "expected `;` after return");
                        statements.Add(new ReturnExpr(value));
                        continue;
                    }
                    if (Peek().Kind == TokenKind.Identifier && Peek().Text == "while")
                    {
                        Advance();
                        statements.Add(ParseWhile());
                        continue;
                    }
                    if (Peek().Kind == TokenKind.Identifier && Peek().Text == "for")
                    {
                        Advance();
                        statements.Add(ParseFor());
                        continue;
                    }
                    if (Peek().Kind == TokenKind.Identifier && Peek().Text is "break" or "continue")
                    {
                        var keyword = Advance().Text;
                        if (_loopParseDepth == 0)
                            Error("COFLOW-FUNCTION-CONTROL", $"`{keyword}` can only be used inside a loop");
                        Expect(TokenKind.Semicolon, $"expected `;` after `{keyword}`");
                        statements.Add(new LoopControlExpr(keyword == "break"));
                        continue;
                    }

                    var expression = ParseExpression();
                    if (Match(TokenKind.Semicolon))
                    {
                        statements.Add(new DiscardExpr(expression));
                        if (Peek().Kind == TokenKind.RightBrace)
                            result = new ConstantExpr(Unit.Value, typeof(Unit));
                        continue;
                    }
                    result = expression;
                    Expect(TokenKind.RightBrace, "expected `}` after block result");
                    break;
                }
            }
            finally
            {
                _localScopes.RemoveAt(_localScopes.Count - 1);
            }
            return new BlockExpr(statements, result ?? new ConstantExpr(Unit.Value, typeof(Unit)));
        }

        private Expr ParseWhile()
        {
            var condition = ParseExpression();
            if (condition.Type != typeof(bool))
                Error("COFLOW-FUNCTION-TYPE", "while condition must be bool");
            Expect(TokenKind.LeftBrace, "expected `{` after while condition");
            _loopParseDepth++;
            Expr body;
            try { body = ParseBlockContents(); }
            finally { _loopParseDepth--; }
            if (body.Type != typeof(Unit) && !body.AlwaysTerminates)
                Error("COFLOW-FUNCTION-TYPE", "while body must have type `()`");
            return new WhileExpr(condition, body);
        }

        private Expr ParseFor()
        {
            var firstName = ExpectBindingIdentifier("expected a loop binding after `for`").Text;
            string? secondName = null;
            if (Match(TokenKind.Comma))
                secondName = ExpectBindingIdentifier("expected a second loop binding").Text;
            ExpectIdentifier("in");
            var collection = ParseExpression();
            var isRange = collection.Type == typeof(CoflowRange);
            if (!collection.Type.IsGenericType && !isRange)
                Error("COFLOW-FUNCTION-TYPE", "for requires an array, dictionary, or range");
            var definition = collection.Type.IsGenericType ? collection.Type.GetGenericTypeDefinition() : null;
            var types = collection.Type.IsGenericType ? collection.Type.GetGenericArguments() : new[] { typeof(long) };
            var isArray = definition == typeof(IReadOnlyList<>);
            var isDictionary = definition == typeof(IReadOnlyDictionary<,>);
            if (!isArray && !isDictionary && !isRange)
                Error("COFLOW-FUNCTION-TYPE", "for requires an array, dictionary, or range");
            if (isDictionary && secondName is null)
                Error("COFLOW-FUNCTION-TYPE", "dictionary for loops require `key, value` bindings");
            if (secondName == firstName)
                Error("COFLOW-FUNCTION-NAME", $"loop binding `{firstName}` is declared more than once");

            var collectionLocal = _localCount++;
            var indexLocal = _localCount++;
            var firstLocal = _localCount++;
            int? secondLocal = secondName is null ? null : _localCount++;
            var scope = new Dictionary<string, (int Index, Type Type)>(StringComparer.Ordinal);
            if (isArray || isRange)
            {
                scope.Add(firstName, (firstLocal, types[0]));
                if (secondName is not null) scope.Add(secondName, (secondLocal!.Value, typeof(long)));
            }
            else
            {
                scope.Add(firstName, (firstLocal, types[0]));
                scope.Add(secondName!, (secondLocal!.Value, types[1]));
            }
            Expect(TokenKind.LeftBrace, "expected `{` after for collection");
            _loopParseDepth++;
            Expr body;
            try { body = ParseBlockContents(scope); }
            finally { _loopParseDepth--; }
            if (body.Type != typeof(Unit) && !body.AlwaysTerminates)
                Error("COFLOW-FUNCTION-TYPE", "for body must have type `()`");
            return new ForExpr(collection, isArray || isRange, collectionLocal, indexLocal,
                firstLocal, secondLocal, body,
                isRange ? ValueFactories.RangeLoop() :
                isArray ? ValueFactories.ArrayLoop(types[0]) : ValueFactories.DictionaryLoop(types[0], types[1]));
        }

        private Expr ParseVariable()
        {
            var name = ExpectBindingIdentifier("expected a local variable name").Text;
            string? declaredType = null;
            if (Match(TokenKind.Colon)) declaredType = ParseTypeName();
            Expect(TokenKind.Equal, "expected `=` in local variable declaration");
            var value = ParseExpression();
            if (declaredType is not null)
                value = value.WithExpected(ResolveTypeName(declaredType), this);
            var scope = _localScopes[^1];
            if (scope.ContainsKey(name) ||
                (_lambdaContexts.Count == 0 ? _parameters.ContainsKey(name) : _lambdaContexts.Peek().Parameters.ContainsKey(name)))
                Error("COFLOW-FUNCTION-NAME", $"name `{name}` is already declared in this scope");
            var local = (_localCount++, value.Type);
            scope.Add(name, local);
            _mutableLocals.Add(local.Item1);
            return new StoreLocalExpr(local.Item1, value);
        }

        private Expr ParseExpression()
        {
            if (++_expressionDepth > MaximumParseDepth)
                Error("COFLOW-FUNCTION-LIMIT", "function expression exceeds the nesting depth limit");
            try
            {
                var offset = Peek().Offset;
                return ParseExpressionCore().At(offset);
            }
            finally
            {
                _expressionDepth--;
            }
        }

        private Expr ParseExpressionCore()
        {
            var left = ParseBinary(0);
            if (Peek().Kind is TokenKind.DotDot or TokenKind.DotDotEqual)
            {
                var inclusive = Advance().Kind == TokenKind.DotDotEqual;
                var end = ParseBinary(0);
                left = new RangeExpr(
                    left.WithExpected(typeof(long), this),
                    end.WithExpected(typeof(long), this),
                    inclusive);
            }
            var assignment = Peek().Kind;
            if (assignment is not (TokenKind.Equal or TokenKind.PlusEqual or TokenKind.MinusEqual or
                TokenKind.StarEqual or TokenKind.SlashEqual))
                return left;
            Advance();
            if (left is not LocalExpr)
                Error("COFLOW-FUNCTION-ASSIGN", "only a local `var` can be assigned");
            var local = (LocalExpr)left;
            if (!_mutableLocals.Contains(local.Index))
                Error("COFLOW-FUNCTION-ASSIGN", "only a local `var` can be assigned");
            var right = ParseExpression();
            if (assignment == TokenKind.Equal)
                return new AssignLocalExpr(local.Index, right.WithExpected(local.Type, this));
            var operation = assignment switch
            {
                TokenKind.PlusEqual => "+", TokenKind.MinusEqual => "-",
                TokenKind.StarEqual => "*", TokenKind.SlashEqual => "/",
                _ => throw new InvalidOperationException(),
            };
            return new AssignLocalExpr(local.Index,
                BinaryExpr.Create(operation, local, right, this).WithExpected(local.Type, this));
        }

        private Expr ParseBinary(int minimumPrecedence)
        {
            if (++_binaryDepth > MaximumParseDepth)
                Error("COFLOW-FUNCTION-LIMIT", "binary expression exceeds the nesting depth limit");
            try
            {
                var left = ParseUnary();
                while (true)
                {
                    if (Peek().Kind == TokenKind.Identifier && Peek().Text == "is" && minimumPrecedence <= 7)
                    {
                        Advance();
                        var target = ResolveTypeName(ParseTypeName());
                        if (!_generatedNames.ContainsKey(target))
                            Error("COFLOW-FUNCTION-TYPE", "`is` target must be a generated object type");
                        left = new TypeIsExpr(left, target, left switch
                        {
                            LocalExpr local => local.Name,
                            ArgumentExpr argument => argument.Name,
                            _ => null,
                        });
                        continue;
                    }
                    if (!TryBinary(Peek().Kind, out var precedence, out var operation) || precedence < minimumPrecedence)
                        break;
                    Advance();
                    var right = ParseBinary(operation == "**" ? precedence : precedence + 1);
                    if (IsComparison(operation) && left is ComparisonChainExpr chain)
                    {
                        left = chain.Append(operation, right, this);
                    }
                    else if (IsComparison(operation) && left is BinaryExpr previous && IsComparison(previous.Operation))
                    {
                        left = ComparisonChainExpr.Create(previous.Left, previous.Right,
                            previous.Operation, operation, right, this);
                    }
                    else
                    {
                        left = CreateBinary(operation, left, right);
                    }
                }
                return left;
            }
            finally
            {
                _binaryDepth--;
            }
        }

        private static bool IsComparison(string operation) => operation is "<" or "<=" or ">" or ">=";

        private Expr CreateBinary(string operation, Expr left, Expr right) =>
            left.Type.IsEnum || right.Type.IsEnum
                ? EnumBinaryExpr.Create(operation, left, right, this, EnumMetadata(left.Type))
                : BinaryExpr.Create(operation, left, right, this);

        private ICoflowEnumMetadata EnumMetadata(Type type)
        {
            var metadata = _enums.Values.FirstOrDefault(item => item.RuntimeType == type);
            if (metadata is null)
                Error("COFLOW-FUNCTION-TYPE", $"enum `{type}` has no generated metadata");
            return metadata;
        }

        private Expr ParseUnary()
        {
            if (++_unaryDepth > MaximumParseDepth)
                Error("COFLOW-FUNCTION-LIMIT", "unary expression exceeds the nesting depth limit");
            try
            {
                if (Match(TokenKind.Minus)) return UnaryExpr.Create("-", ParseUnary(), this);
                if (Match(TokenKind.Bang)) return UnaryExpr.Create("!", ParseUnary(), this);
                if (Match(TokenKind.Tilde)) return UnaryExpr.Create("~", ParseUnary(), this);
                var expression = ParsePrimary();
                while (true)
                {
                    if (Match(TokenKind.LeftParen))
                    {
                        expression = ParseCall(expression);
                        continue;
                    }
                    if (Match(TokenKind.LeftBracket))
                    {
                        var index = ParseExpression();
                        Expect(TokenKind.RightBracket, "expected `]` after index");
                        expression = IndexExpr.Create(expression, index, this);
                        continue;
                    }
                    if (Match(TokenKind.Dot))
                    {
                        if (Match(TokenKind.Dollar))
                        {
                            var metadataName = Expect(TokenKind.Identifier,
                                "expected metadata name after `.$`").Text;
                            expression = ParseRecordMetadata(expression, metadataName);
                            continue;
                        }
                        var field = Expect(TokenKind.Identifier, "expected a field name after `.`").Text;
                        var metadata = _metadata.Values.FirstOrDefault(item => item.RuntimeType == expression.Type);
                        if (metadata is not null && metadata.FieldNames.Contains(field, StringComparer.Ordinal))
                        {
                            expression = ParseField(expression, field);
                        }
                        else
                        {
                            Expect(TokenKind.LeftParen, $"expected `(` after built-in method `{field}`");
                            expression = ParseBuiltin(expression, field);
                        }
                        continue;
                    }
                    if (Match(TokenKind.Question))
                    {
                        expression = ParsePropagation(expression);
                        continue;
                    }
                    break;
                }
                return expression;
            }
            finally
            {
                _unaryDepth--;
            }
        }

        private Expr ParseBuiltin(Expr receiver, string name)
        {
            var arguments = new List<Expr>();
            if (!Match(TokenKind.RightParen))
            {
                do arguments.Add(ParseExpression()); while (Match(TokenKind.Comma));
                Expect(TokenKind.RightParen, "expected `)` after built-in arguments");
            }
            string? regexPattern = null;
            if (name == "matches")
            {
                if (arguments.Count != 1 || arguments[0] is not ConstantExpr { Value: string })
                    Error("COFLOW-FUNCTION-BUILTIN", "matches pattern must be a string literal");
                var pattern = (string)((ConstantExpr)arguments[0]).Value!;
                regexPattern = pattern;
                try { CoflowBuiltinLibrary.ValidateRegexPattern(pattern); }
                catch (ArgumentException error) { Error("COFLOW-FUNCTION-BUILTIN", error.Message); }
            }
            if (name is "map" or "filter" or "fold" or "find" or "any" or "all")
                return ParseHigherOrderBuiltin(receiver, name, arguments);
            try
            {
                var builtin = regexPattern is null
                    ? CoflowBuiltinLibrary.Resolve(name, receiver.Type,
                        arguments.Select(argument => argument.Type).ToArray())
                    : CoflowBuiltinLibrary.ResolveRegex(regexPattern);
                return new BuiltinExpr(receiver, arguments, builtin);
            }
            catch (ArgumentException error)
            {
                Error("COFLOW-FUNCTION-BUILTIN", error.Message);
                return null!;
            }
        }

        private Expr ParseHigherOrderBuiltin(Expr receiver, string name, IReadOnlyList<Expr> arguments)
        {
            if (!receiver.Type.IsGenericType || receiver.Type.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                Error("COFLOW-FUNCTION-BUILTIN", $"{name} requires an array receiver");
            var element = receiver.Type.GetGenericArguments()[0];
            Expr callable;
            Type result;
            Type outputElement;
            if (name == "fold")
            {
                if (arguments.Count != 2)
                    Error("COFLOW-FUNCTION-BUILTIN", "fold requires an initial value and a function");
                callable = arguments[1];
                var signature = callable.CallableSignature;
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
                callable = arguments[0];
                var signature = callable.CallableSignature;
                if (signature is null || signature.ParameterTypes.Count != 1 ||
                    !IsAssignable(element, signature.ParameterTypes[0]))
                    Error("COFLOW-FUNCTION-BUILTIN", $"{name} function must accept the array element type");
                if (name is "filter" or "find" or "any" or "all")
                {
                    if (signature!.ResultType != typeof(bool))
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
                    outputElement = signature!.ResultType;
                    result = typeof(IReadOnlyList<>).MakeGenericType(outputElement);
                }
            }
            return new HigherOrderExpr(receiver, arguments,
                ValueFactories.HigherOrder(name, element, outputElement, result));
        }

        private Expr ParsePropagation(Expr operand)
        {
            if (!operand.Type.IsGenericType)
                return Invalid();
            var definition = operand.Type.GetGenericTypeDefinition();
            var arguments = operand.Type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                if (!_returnType.IsGenericType || _returnType.GetGenericTypeDefinition() != typeof(Option<>))
                    Error("COFLOW-FUNCTION-PROPAGATE", "Option can only propagate from an Option-returning function");
                return new PropagateExpr(operand, arguments[0],
                    ValueFactories.OptionPropagation(arguments[0], _returnType.GetGenericArguments()[0]));
            }
            if (definition == typeof(Result<,>))
            {
                if (!_returnType.IsGenericType || _returnType.GetGenericTypeDefinition() != typeof(Result<,>) ||
                    _returnType.GetGenericArguments()[1] != arguments[1])
                    Error("COFLOW-FUNCTION-PROPAGATE", "Result can only propagate to a Result with the same error type");
                return new PropagateExpr(operand, arguments[0],
                    ValueFactories.ResultPropagation(arguments[0], arguments[1], _returnType.GetGenericArguments()[0]));
            }
            return Invalid();

            Expr Invalid()
            {
                Error("COFLOW-FUNCTION-PROPAGATE", "`?` requires Option or Result");
                return null!;
            }
        }

        private Expr ParseField(Expr receiver, string fieldName)
        {
            var metadata = _metadata.Values.FirstOrDefault(item => item.RuntimeType == receiver.Type);
            if (metadata is null || !metadata.FieldNames.Contains(fieldName, StringComparer.Ordinal))
                Error("COFLOW-FUNCTION-FIELD",
                    $"type `{FormatType(receiver.Type)}` has no field `{fieldName}`");
            var resolved = metadata!;
            return new FieldExpr(receiver, resolved.GetFieldType(fieldName),
                _storage.BindField(resolved, fieldName));
        }

        private Expr ParseCall(Expr target)
        {
            var signature = target.CallableSignature;
            if (signature is null)
                Error("COFLOW-FUNCTION-CALL", "expression is not callable");
            var callable = signature!;
            var arguments = new List<Expr>();
            if (!Match(TokenKind.RightParen))
            {
                do arguments.Add(ParseExpression()); while (Match(TokenKind.Comma));
                Expect(TokenKind.RightParen, "expected `)` after function arguments");
            }
            if (arguments.Count != callable.ParameterTypes.Count)
                Error("COFLOW-FUNCTION-CALL",
                    $"function expects {callable.ParameterTypes.Count} arguments but received {arguments.Count}");
            for (var index = 0; index < arguments.Count; index++)
                arguments[index] = arguments[index].WithExpected(
                    callable.ParameterTypes[index], this);
            return new CallExpr(target, callable, arguments);
        }

        private Expr ParsePrimary()
        {
            var token = Advance();
            switch (token.Kind)
            {
                case TokenKind.Integer:
                    if (!long.TryParse(token.Text, NumberStyles.None, CultureInfo.InvariantCulture, out var integer))
                        Error("COFLOW-FUNCTION-LITERAL", $"integer literal `{token.Text}` is out of range");
                    return new ConstantExpr(integer, typeof(long));
                case TokenKind.Float:
                    if (!double.TryParse(token.Text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) || !double.IsFinite(number))
                        Error("COFLOW-FUNCTION-LITERAL", $"float literal `{token.Text}` is invalid");
                    return new ConstantExpr(number, typeof(double));
                case TokenKind.String:
                    return new ConstantExpr(token.Text, typeof(string));
                case TokenKind.InterpolatedStringStart:
                    return ParseInterpolatedString();
                case TokenKind.Identifier when token.Text == "true":
                    return new ConstantExpr(true, typeof(bool));
                case TokenKind.Identifier when token.Text == "false":
                    return new ConstantExpr(false, typeof(bool));
                case TokenKind.Identifier when token.Text is "int" or "float":
                    return ParseNumericConversion(token.Text);
                case TokenKind.Identifier when token.Text == "None":
                    return new NoneExpr();
                case TokenKind.Identifier when token.Text is "Some" or "Ok" or "Err":
                    return ParseValueConstructor(token.Text);
                case TokenKind.Identifier when token.Text == "if":
                    return ParseIfExpression();
                case TokenKind.Identifier when token.Text == "match":
                    return ParseMatchExpression();
                case TokenKind.Identifier when token.Text == "fn":
                    return ParseAnonymousFunction();
                case TokenKind.LeftBracket:
                    return ParseArrayLiteral();
                case TokenKind.LeftBrace:
                    return ParseDictionaryLiteral();
                case TokenKind.Ampersand:
                    return ParseRecordFieldReference();
                case TokenKind.Dollar:
                    return ParseContextMetadata();
                case TokenKind.Identifier:
                    if (StartsObjectConstructor(token.Text))
                        return ParseObjectConstructor(token);
                    if (Peek().Kind == TokenKind.DoubleColon)
                        return ParseStaticValue(token);
                    if (Peek().Kind == TokenKind.LeftParen &&
                        TryResolveEnum(token.Text, out var enumConstructor))
                    {
                        Advance();
                        var enumInteger = ParseExpression().WithExpected(typeof(long), this);
                        Expect(TokenKind.RightParen, "expected `)` after enum integer value");
                        return new ConversionExpr(enumInteger, enumConstructor.RuntimeType,
                            value => enumConstructor.FromInt64((long)value!));
                    }
                    if (_lambdaContexts.TryPeek(out var lambda) &&
                        lambda.Parameters.TryGetValue(token.Text, out var lambdaParameter))
                        return new ArgumentExpr(lambdaParameter.Index, lambdaParameter.Type);
                    for (var scope = _localScopes.Count - 1; scope >= 0; scope--)
                    {
                        if (_localScopes[scope].TryGetValue(token.Text, out var local))
                        {
                            if (lambda is null || scope >= lambda.ScopeBase)
                                return new LocalExpr(local.Index, NarrowedType(token.Text) ?? local.Type, token.Text);
                            return lambda.Capture($"L:{local.Index}", new LocalExpr(local.Index, NarrowedType(token.Text) ?? local.Type, token.Text));
                        }
                    }
                    if (_parameters.TryGetValue(token.Text, out var parameter))
                    {
                        var argument = new ArgumentExpr(parameter.Index, NarrowedType(token.Text) ?? parameter.Type, token.Text);
                        return lambda is null ? argument : lambda.Capture($"A:{parameter.Index}", argument);
                    }
                    if (_records.TryGetValue(
                            (_slot.Identity.DeclaredType, _slot.Identity.RecordKey), out var owner) &&
                        _metadata.TryGetValue(_slot.Identity.DeclaredType, out var ownerMetadata) &&
                        ownerMetadata.FieldNames.Contains(token.Text, StringComparer.Ordinal))
                    {
                        if (ownerMetadata.GetFieldType(token.Text) == typeof(CoflowFunctionSlot))
                            return new FunctionReferenceExpr((CoflowFunctionSlot)ownerMetadata.GetField(owner, token.Text));
                        return new FieldExpr(
                            new ConstantExpr(owner, ownerMetadata.RuntimeType),
                            ownerMetadata.GetFieldType(token.Text),
                            _storage.BindField(ownerMetadata, token.Text));
                    }
                    if (_declaredConstants.TryGetValue(_names.Resolve(token.Text), out var constant))
                        return new ConstantExpr(_context.ResolveConstant(constant), constant.RuntimeType);
                    Error("COFLOW-FUNCTION-NAME", $"unknown name `{token.Text}`");
                    return null!;
                case TokenKind.LeftParen:
                {
                    if (Match(TokenKind.RightParen))
                        return new ConstantExpr(Unit.Value, typeof(Unit));
                    var expression = ParseExpression();
                    Expect(TokenKind.RightParen, "expected `)`");
                    return expression;
                }
                default:
                    Error("COFLOW-FUNCTION-EXPRESSION", $"expected an expression, found `{token.Text}`");
                    return null!;
            }
        }

        private Expr ParseInterpolatedString()
        {
            var parts = new List<InterpolationPart>();
            while (!Match(TokenKind.InterpolatedStringEnd))
            {
                if (Peek().Kind == TokenKind.String)
                {
                    parts.Add(new InterpolationPart(Advance().Text, null));
                    continue;
                }
                Expect(TokenKind.InterpolationStart, "expected an interpolation expression");
                if (Peek().Kind == TokenKind.InterpolationEnd)
                    Error("COFLOW-FUNCTION-INTERPOLATION", "string interpolation expression cannot be empty");
                var value = ParseExpression();
                Expect(TokenKind.InterpolationEnd, "expected `}` after string interpolation expression");
                if (!IsInterpolatable(value.Type))
                    Error("COFLOW-FUNCTION-INTERPOLATION",
                        $"values of type `{FormatType(value.Type)}` cannot be interpolated");
                parts.Add(new InterpolationPart(null, value));
            }
            return new InterpolatedStringExpr(parts);
        }

        private bool IsInterpolatable(Type type) => IsInterpolatable(type, new HashSet<Type>());

        private bool SupportsEquality(Type type) => SupportsEquality(type, new HashSet<Type>());

        private bool SupportsEquality(Type type, HashSet<Type> visiting)
        {
            if (typeof(Delegate).IsAssignableFrom(type) || type == typeof(CoflowFunctionSlot))
                return false;
            var metadata = _metadata.Values.FirstOrDefault(item => item.RuntimeType == type);
            if (metadata is not null)
            {
                if (!visiting.Add(type)) return true;
                var result = metadata.FieldNames.All(field =>
                    SupportsEquality(metadata.GetFieldType(field), visiting));
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
            if (typeof(Delegate).IsAssignableFrom(type) || type == typeof(CoflowFunctionSlot))
                return false;
            var metadata = _metadata.Values.FirstOrDefault(item => item.RuntimeType == type);
            if (metadata is not null)
            {
                if (metadata.IsRecord || !visiting.Add(type)) return true;
                var result = metadata.FieldNames.All(field =>
                    IsInterpolatable(metadata.GetFieldType(field), visiting));
                visiting.Remove(type);
                return result;
            }
            if (!type.IsGenericType) return false;
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            return definition == typeof(Option<>) && IsInterpolatable(arguments[0], visiting) ||
                   definition == typeof(Result<,>) && arguments.All(argument => IsInterpolatable(argument, visiting)) ||
                   definition == typeof(IReadOnlyList<>) && IsInterpolatable(arguments[0], visiting) ||
                   definition == typeof(IReadOnlyDictionary<,>) &&
                       arguments.All(argument => IsInterpolatable(argument, visiting));
        }

        private bool StartsObjectConstructor(string first)
        {
            var cursor = _index;
            var segments = new List<string> { first };
            while (cursor + 1 < _tokens.Count &&
                   _tokens[cursor].Kind == TokenKind.DoubleColon &&
                   _tokens[cursor + 1].Kind == TokenKind.Identifier)
            {
                segments.Add(_tokens[cursor + 1].Text);
                cursor += 2;
            }
            return cursor < _tokens.Count &&
                   _tokens[cursor].Kind == TokenKind.LeftBrace &&
                   _metadata.ContainsKey(_names.Resolve(string.Join("::", segments)));
        }

        private Expr ParseObjectConstructor(Token first)
        {
            var segments = new List<string> { first.Text };
            while (Match(TokenKind.DoubleColon))
                segments.Add(Expect(TokenKind.Identifier, "expected a name after `::`").Text);
            var sourceName = string.Join("::", segments);
            var declaredName = _names.Resolve(sourceName);
            if (!_metadata.TryGetValue(declaredName, out var metadata))
                Error("COFLOW-FUNCTION-OBJECT", $"unknown object type `{declaredName}`");
            if (metadata.IsAbstract)
                Error("COFLOW-FUNCTION-OBJECT", $"abstract type `{declaredName}` cannot be constructed");
            if (metadata.IsHost || metadata.IsSingleton)
                Error("COFLOW-FUNCTION-OBJECT", $"singleton type `{declaredName}` cannot be constructed as a value");
            if (metadata.FieldNames.Any(field => metadata.GetFieldType(field) == typeof(CoflowFunctionSlot)))
                Error("COFLOW-FUNCTION-OBJECT", $"type `{declaredName}` has function fields and cannot be constructed as an object");

            Expect(TokenKind.LeftBrace, "expected `{` after an object type");
            var fields = new List<(string Name, Expr Value)>();
            var seen = new HashSet<string>(StringComparer.Ordinal);
            while (!Match(TokenKind.RightBrace))
            {
                var field = Expect(TokenKind.Identifier, "expected an object field name");
                if (!seen.Add(field.Text))
                    Error("COFLOW-FUNCTION-OBJECT", $"object field `{field.Text}` is specified more than once");
                if (!metadata.FieldNames.Contains(field.Text, StringComparer.Ordinal))
                    Error("COFLOW-FUNCTION-OBJECT", $"type `{declaredName}` has no field `{field.Text}`");
                var fieldType = metadata.GetFieldType(field.Text);
                if (fieldType == typeof(CoflowFunctionSlot))
                    Error("COFLOW-FUNCTION-OBJECT", $"function field `{field.Text}` cannot be supplied by an object constructor");
                Expect(TokenKind.Colon, "expected `:` after an object field name");
                fields.Add((field.Text, ParseExpression().WithExpected(fieldType, this)));
                if (!Match(TokenKind.Comma))
                {
                    Expect(TokenKind.RightBrace, "expected `,` or `}` after an object field");
                    break;
                }
                if (Match(TokenKind.RightBrace)) break;
            }
            foreach (var field in metadata.FieldNames)
            {
                if (metadata.GetFieldType(field) != typeof(CoflowFunctionSlot) &&
                    !seen.Contains(field) &&
                    !metadata.HasFieldDefault(field))
                    Error("COFLOW-FUNCTION-OBJECT", $"object `{declaredName}` is missing required field `{field}`");
            }
            return new ObjectExpr(metadata, _context, fields);
        }

        private Expr ParseContextMetadata()
        {
            var name = Expect(TokenKind.Identifier, "expected metadata name after `$`").Text;
            var value = name switch
            {
                "id" => _slot.Identity.RecordKey,
                "path" => $"{_slot.Identity.DeclaredType}::{_slot.Identity.RecordKey}",
                "type" => _slot.Identity.DeclaredType,
                "field" => _slot.Identity.FieldName,
                "function" => _slot.Identity.FieldName,
                _ => null,
            };
            if (value is null)
                Error("COFLOW-FUNCTION-METADATA", $"unknown compile-time metadata `${name}`");
            return new ConstantExpr(value, typeof(string));
        }

        private Expr ParseRecordMetadata(Expr receiver, string name)
        {
            if (name is not ("id" or "path"))
                Error("COFLOW-FUNCTION-METADATA",
                    $"record metadata only supports `$id` and `$path`, found `${name}`");
            if (!_metadata.Values.Any(item => item.RuntimeType.IsAssignableFrom(receiver.Type) ||
                    receiver.Type.IsAssignableFrom(item.RuntimeType)))
                Error("COFLOW-FUNCTION-METADATA",
                    $"`${name}` requires a generated record receiver");
            return new TransformExpr(receiver, typeof(string), value =>
            {
                var actual = value ?? throw new InvalidOperationException("record metadata receiver is null");
                var metadata = _metadata.Values.SingleOrDefault(item => item.RuntimeType == actual.GetType())
                    ?? throw new InvalidOperationException(
                        $"generated record type `{actual.GetType()}` has no Coflow metadata");
                var key = RenderRecordKey(metadata.GetKey(actual));
                return name == "id" ? key : $"{metadata.DeclaredType}::{key}";
            });
        }

        private string RenderRecordKey(object key)
        {
            if (key is string text) return text;
            var metadata = _enums.Values.SingleOrDefault(item => item.RuntimeType == key.GetType())
                ?? throw new InvalidOperationException(
                    $"generated record key type `{key.GetType()}` has no enum metadata");
            return metadata.Variants.Single(pair => Equals(pair.Value, key)).Key;
        }

        private Expr ParseStaticValue(Token first)
        {
            var segments = new List<string> { first.Text };
            while (Match(TokenKind.DoubleColon))
                segments.Add(Expect(TokenKind.Identifier, "expected a name after `::`").Text);
            if (segments.Count < 2)
                Error("COFLOW-FUNCTION-NAME", $"invalid static path `{first.Text}`");
            var staticPath = _names.ResolveStaticPath(string.Join("::", segments));
            if (_declaredConstants.TryGetValue(staticPath, out var constant))
                return new ConstantExpr(_context.ResolveConstant(constant), constant.RuntimeType);
            var owner = _names.Resolve(string.Join("::", segments.Take(segments.Count - 1)));
            var member = segments[^1];
            if (!_enums.TryGetValue(owner, out var enumMetadata))
                Error("COFLOW-FUNCTION-NAME", $"unknown static owner `{owner}`");
            if (!enumMetadata.Variants.TryGetValue(member, out var enumValue))
                Error("COFLOW-FUNCTION-NAME", $"enum `{owner}` has no variant `{member}`");
            return new ConstantExpr(enumValue, enumMetadata.RuntimeType);
        }

        private bool TryResolveEnum(string name, out ICoflowEnumMetadata metadata) =>
            _enums.TryGetValue(_names.Resolve(name), out metadata!);

        private Type? NarrowedType(string name)
        {
            foreach (var narrowing in _narrowings)
                if (narrowing.TryGetValue(name, out var type)) return type;
            return null;
        }

        private Expr ParseNumericConversion(string target)
        {
            Expect(TokenKind.LeftParen, $"expected `(` after `{target}`");
            var value = ParseExpression();
            Expect(TokenKind.RightParen, "expected `)` after numeric conversion");
            var result = target == "int" ? typeof(long) : typeof(double);
            if (value.Type is not null && value.Type != typeof(long) && value.Type != typeof(double))
                Error("COFLOW-FUNCTION-TYPE", $"{target} conversion requires int or float");
            Func<object?, object?> convert = (value.Type, result) switch
            {
                (var source, var destination) when source == destination => static value => value,
                (var source, _) when source == typeof(long) => static value => (double)(long)value!,
                _ => static value => checked((long)(double)value!),
            };
            return new ConversionExpr(value, result, convert);
        }

        private Expr ParseAnonymousFunction()
        {
            Expect(TokenKind.LeftParen, "expected `(` after `fn`");
            var parameters = new Dictionary<string, (int Index, Type Type)>(StringComparer.Ordinal);
            var parameterTypes = new List<Type>();
            if (!Match(TokenKind.RightParen))
            {
                do
                {
                    var name = ExpectBindingIdentifier("expected an anonymous function parameter name").Text;
                    Expect(TokenKind.Colon, "expected `:` after anonymous function parameter");
                    var type = ResolveTypeName(ParseTypeName());
                    if (!parameters.TryAdd(name, (parameterTypes.Count, type)))
                        Error("COFLOW-FUNCTION-NAME", $"parameter `{name}` is declared more than once");
                    parameterTypes.Add(type);
                } while (Match(TokenKind.Comma));
                Expect(TokenKind.RightParen, "expected `)` after anonymous function parameters");
            }
            Expect(TokenKind.Arrow, "expected `->` after anonymous function parameters");
            var resultType = ResolveTypeName(ParseTypeName());
            Expect(TokenKind.LeftBrace, "expected an anonymous function body");
            var context = new LambdaParseContext(_localScopes.Count, parameters, parameterTypes.Count);
            _lambdaContexts.Push(context);
            var previousReturn = _returnType;
            _returnType = resultType;
            Expr body;
            try { body = ParseBlockContents().WithExpected(resultType, this); }
            finally
            {
                _returnType = previousReturn;
                _lambdaContexts.Pop();
            }
            return new LambdaExpr(
                new CoflowFunctionSignature(resultType, parameterTypes),
                context.Captures,
                body);
        }

        private Type ResolveTypeName(string name)
        {
            if (name.StartsWith("&", StringComparison.Ordinal)) name = name[1..];
            if (name == "int") return typeof(long);
            if (name == "float") return typeof(double);
            if (name == "bool") return typeof(bool);
            if (name == "string") return typeof(string);
            if (name == "()") return typeof(Unit);
            var canonicalName = _names.Resolve(name);
            var generated = _generatedNames.FirstOrDefault(item => item.Value == canonicalName);
            if (generated.Key is not null) return generated.Key;
            if (name.StartsWith("[", StringComparison.Ordinal) && name.EndsWith(']'))
                return typeof(IReadOnlyList<>).MakeGenericType(ResolveTypeName(name[1..^1]));
            if (name.StartsWith("{", StringComparison.Ordinal) && name.EndsWith('}'))
            {
                var parts = SplitTypeArguments(name[1..^1], ':');
                return typeof(IReadOnlyDictionary<,>).MakeGenericType(
                    ResolveTypeName(parts[0]), ResolveTypeName(parts[1]));
            }
            if (name.StartsWith("Option<", StringComparison.Ordinal))
                return typeof(Option<>).MakeGenericType(ResolveTypeName(name[7..^1]));
            if (name.StartsWith("Result<", StringComparison.Ordinal))
            {
                var parts = SplitTypeArguments(name[7..^1], ',');
                return typeof(Result<,>).MakeGenericType(ResolveTypeName(parts[0]), ResolveTypeName(parts[1]));
            }
            if (name.StartsWith("fn(", StringComparison.Ordinal))
            {
                var arrow = FindFunctionParameterEnd(name);
                var parameterText = name[3..arrow];
                var parameterTypes = parameterText.Length == 0
                    ? Array.Empty<Type>()
                    : SplitTypeArguments(parameterText, ',').Select(ResolveTypeName).ToArray();
                return DelegateType(new CoflowFunctionSignature(
                    ResolveTypeName(name[(arrow + 3)..]), parameterTypes));
            }
            Error("COFLOW-FUNCTION-TYPE", $"unknown type `{name}`");
            return null!;
        }

        private static int FindFunctionParameterEnd(string name)
        {
            var depth = 0;
            for (var index = 2; index < name.Length; index++)
            {
                if (name[index] == '(') depth++;
                else if (name[index] == ')' && --depth == 0 &&
                    name.AsSpan(index).StartsWith(")->", StringComparison.Ordinal))
                    return index;
            }
            throw new InvalidOperationException($"invalid function type `{name}`");
        }

        private static string[] SplitTypeArguments(string value, char separator)
        {
            var result = new List<string>();
            var depth = 0;
            var start = 0;
            for (var index = 0; index < value.Length; index++)
            {
                depth += value[index] is '<' or '[' or '{' or '(' ? 1 : 0;
                depth -= value[index] is '>' or ']' or '}' or ')' ? 1 : 0;
                if (value[index] == separator && depth == 0)
                {
                    result.Add(value[start..index]);
                    start = index + 1;
                }
            }
            result.Add(value[start..]);
            return result.ToArray();
        }

        private Expr ParseMatchExpression()
        {
            var subject = ParseExpression();
            Expect(TokenKind.LeftBrace, "expected `{` after match value");
            var subjectLocal = _localCount++;
            var arms = new List<MatchArm>();
            var patternKinds = new HashSet<string>(StringComparer.Ordinal);
            var hasCatchAll = false;
            while (!Match(TokenKind.RightBrace))
            {
                if (hasCatchAll)
                    Error("COFLOW-FUNCTION-MATCH", "no match arm may follow a binding or `_` arm");
                var pattern = ParseMatchPattern(subject.Type);
                if (!patternKinds.Add(pattern.Kind))
                    Error("COFLOW-FUNCTION-MATCH", $"duplicate match pattern `{pattern.Kind}`");
                hasCatchAll = pattern.IsCatchAll;
                Expect(TokenKind.FatArrow, "expected `=>` after match pattern");
                var scope = new Dictionary<string, (int Index, Type Type)>(StringComparer.Ordinal);
                int? bindingLocal = null;
                if (pattern.BindingName is { } binding)
                {
                    bindingLocal = _localCount++;
                    scope.Add(binding, (bindingLocal.Value, pattern.BindingType!));
                }
                Expr body;
                if (Match(TokenKind.LeftBrace))
                {
                    body = ParseBlockContents(scope);
                }
                else
                {
                    _localScopes.Add(scope);
                    try { body = ParseExpression(); }
                    finally { _localScopes.RemoveAt(_localScopes.Count - 1); }
                }
                arms.Add(new MatchArm(pattern, bindingLocal, body));
                if (!Match(TokenKind.Comma))
                    Expect(TokenKind.RightBrace, "expected `,` or `}` after match arm");
                else if (Match(TokenKind.RightBrace))
                    break;
            }
            if (arms.Count == 0) Error("COFLOW-FUNCTION-MATCH", "match requires at least one arm");
            var exhaustive = hasCatchAll || IsExhaustiveMatch(subject.Type, patternKinds);
            if (!exhaustive) Error("COFLOW-FUNCTION-MATCH", "match is not exhaustive");
            var resultType = arms[0].Body.Type;
            foreach (var arm in arms.Skip(1))
            {
                if (IsAssignable(arm.Body.Type, resultType)) continue;
                if (IsAssignable(resultType, arm.Body.Type))
                {
                    resultType = arm.Body.Type;
                    continue;
                }
                else
                    Error("COFLOW-FUNCTION-TYPE", "match arms must have the same result type");
            }
            var typedArms = arms
                .Select(arm => arm with { Body = arm.Body.WithExpected(resultType, this) })
                .ToArray();
            return new MatchExpr(subject, subjectLocal, typedArms, !hasCatchAll);
        }

        private MatchPattern ParseMatchPattern(Type subjectType)
        {
            var token = Advance();
            var negative = false;
            if (token.Kind == TokenKind.Minus)
            {
                negative = true;
                token = Advance();
                if (token.Kind is not (TokenKind.Integer or TokenKind.Float))
                    Error("COFLOW-FUNCTION-MATCH", "`-` in a match pattern must precede a numeric literal");
            }
            if (token.Kind == TokenKind.Identifier && token.Text == "_")
                return MatchPattern.CatchAll("_", null, null);
            if (token.Kind == TokenKind.Identifier && token.Text is "Some" or "Ok" or "Err")
            {
                Expect(TokenKind.LeftParen, $"expected `(` after `{token.Text}`");
                var binding = ExpectBindingIdentifier("expected a pattern binding").Text;
                Expect(TokenKind.RightParen, "expected `)` after pattern binding");
                return ValueFactories.MatchBranch(subjectType, token.Text, binding, this);
            }
            if (token.Kind == TokenKind.Identifier && token.Text == "None")
                return ValueFactories.MatchNone(subjectType, this);
            if (token.Kind == TokenKind.Identifier)
            {
                var segments = new List<string> { token.Text };
                while (Match(TokenKind.DoubleColon))
                    segments.Add(Expect(TokenKind.Identifier, "expected a name after `::`").Text);
                if (segments.Count > 1)
                {
                    var enumName = _names.Resolve(string.Join("::", segments.Take(segments.Count - 1)));
                    var variant = segments[^1];
                    if (_enums.TryGetValue(enumName, out var enumMetadata))
                    {
                        if (!enumMetadata.Variants.TryGetValue(variant, out var enumValue))
                            Error("COFLOW-FUNCTION-MATCH", $"enum `{enumName}` has no variant `{variant}`");
                        if (enumMetadata.RuntimeType != subjectType)
                            Error("COFLOW-FUNCTION-TYPE", "match enum literal type does not match subject type");
                        return MatchPattern.Literal($"{enumName}::{variant}", enumValue);
                    }
                }

                var typeName = _names.Resolve(string.Join("::", segments));
                var generated = _metadata.Values.FirstOrDefault(item => item.DeclaredType == typeName);
                if (generated is not null && Peek().Kind == TokenKind.Identifier)
                {
                    var binding = ExpectBindingIdentifier("expected a type pattern binding").Text;
                    if (!subjectType.IsAssignableFrom(generated.RuntimeType))
                        Error("COFLOW-FUNCTION-TYPE", $"type pattern `{typeName}` is not assignable to match subject");
                    return new MatchPattern($"type:{typeName}", false, binding, generated.RuntimeType,
                        value => value is not null && generated.RuntimeType.IsInstanceOfType(value), static value => value);
                }

                if (segments.Count > 1)
                    Error("COFLOW-FUNCTION-MATCH", $"unknown match pattern `{string.Join("::", segments)}`");
            }
            object? literal = token.Kind switch
            {
                TokenKind.Integer when long.TryParse(
                    negative ? "-" + token.Text : token.Text,
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var value) => value,
                TokenKind.Float when double.TryParse(
                    negative ? "-" + token.Text : token.Text,
                    NumberStyles.Float,
                    CultureInfo.InvariantCulture,
                    out var value) && double.IsFinite(value) => value,
                TokenKind.String => token.Text,
                TokenKind.Identifier when token.Text == "true" => true,
                TokenKind.Identifier when token.Text == "false" => false,
                _ => null,
            };
            if (literal is not null)
            {
                if (literal.GetType() != subjectType)
                    Error("COFLOW-FUNCTION-TYPE", "match literal type does not match subject type");
                if (literal is double floating && floating == 0) literal = 0d;
                var canonical = Convert.ToString(literal, CultureInfo.InvariantCulture)!;
                return MatchPattern.Literal($"literal:{subjectType.FullName}:{canonical}", literal);
            }
            if (token.Kind == TokenKind.Identifier)
            {
                ValidateBindingIdentifier(token);
                return MatchPattern.CatchAll("binding", token.Text, subjectType);
            }
            Error("COFLOW-FUNCTION-MATCH", $"invalid match pattern `{token.Text}`");
            return null!;
        }

        private bool IsExhaustiveMatch(Type subjectType, HashSet<string> kinds)
        {
            if (subjectType == typeof(bool)) return kinds.Contains("true") && kinds.Contains("false");
            var subjectEnum = _enums.Values.FirstOrDefault(item => item.RuntimeType == subjectType);
            if (subjectEnum is not null)
                return subjectEnum.Variants.Keys.All(variant =>
                    kinds.Contains($"{subjectEnum.DeclaredType}::{variant}"));
            if (!subjectType.IsGenericType)
            {
                var subject = _metadata.Values.FirstOrDefault(item => item.RuntimeType == subjectType);
                if (subject is null) return false;
                var concrete = _metadata.Values.Where(item => !item.IsAbstract &&
                    subjectType.IsAssignableFrom(item.RuntimeType)).ToArray();
                return concrete.Length != 0 && concrete.All(item => item.IsSealed &&
                    kinds.Contains($"type:{item.DeclaredType}"));
            }
            var definition = subjectType.GetGenericTypeDefinition();
            return definition == typeof(Option<>)
                ? kinds.Contains("Some") && kinds.Contains("None")
                : definition == typeof(Result<,>) && kinds.Contains("Ok") && kinds.Contains("Err");
        }

        private Expr ParseRecordFieldReference()
        {
            var first = Expect(TokenKind.Identifier, "expected a record key or type after `&`").Text;
            var segments = new List<string> { first };
            while (Match(TokenKind.DoubleColon))
                segments.Add(Expect(TokenKind.Identifier, "expected a name after `::`").Text);
            string? declaredType = null;
            var key = segments[0];
            var fullName = _names.Resolve(string.Join("::", segments));
            if (_metadata.TryGetValue(fullName, out var singletonMetadata) && singletonMetadata.IsSingleton)
            {
                declaredType = fullName;
                key = string.Empty;
            }
            else if (segments.Count > 1)
            {
                declaredType = _names.Resolve(string.Join("::", segments.Take(segments.Count - 1)));
                key = segments[^1];
            }
            Expect(TokenKind.Dot, "record references in functions must select a field");
            var fieldName = Expect(TokenKind.Identifier, "expected a field name after record reference").Text;
            var matches = _records
                .Where(item => item.Key.Key == key &&
                    (declaredType is null || item.Key.DeclaredType == declaredType))
                .Where(item => _metadata[item.Key.DeclaredType].FieldNames.Contains(fieldName, StringComparer.Ordinal))
                .ToArray();
            if (matches.Length == 0)
                Error("COFLOW-FUNCTION-REFERENCE", $"record `{key}` with field `{fieldName}` was not found");
            if (matches.Length != 1)
                Error("COFLOW-FUNCTION-REFERENCE", $"record reference `{key}.{fieldName}` is ambiguous");
            var match = matches[0];
            var metadata = _metadata[match.Key.DeclaredType];
            if (metadata.GetFieldType(fieldName) == typeof(CoflowFunctionSlot))
                return new FunctionReferenceExpr((CoflowFunctionSlot)metadata.GetField(match.Value, fieldName));
            return new FieldExpr(
                new ConstantExpr(match.Value, metadata.RuntimeType),
                metadata.GetFieldType(fieldName),
                _storage.BindField(metadata, fieldName));
        }

        private Expr ParseArrayLiteral()
        {
            var values = new List<Expr>();
            if (!Match(TokenKind.RightBracket))
            {
                do values.Add(ParseExpression()); while (Match(TokenKind.Comma));
                Expect(TokenKind.RightBracket, "expected `]` after array literal");
            }
            if (values.Count == 0) return new EmptyArrayExpr();
            var elementType = values[0].Type;
            foreach (var value in values.Skip(1))
            {
                if (IsAssignable(value.Type, elementType)) continue;
                if (IsAssignable(elementType, value.Type))
                {
                    elementType = value.Type;
                    continue;
                }
                value.WithExpected(elementType, this);
            }
            var typedValues = values.Select(value => value.WithExpected(elementType, this)).ToArray();
            return new ArrayExpr(typedValues, typeof(IReadOnlyList<>).MakeGenericType(elementType));
        }

        private Expr ParseDictionaryLiteral()
        {
            var entries = new List<(Expr Key, Expr Value)>();
            if (!Match(TokenKind.RightBrace))
            {
                do
                {
                    var key = ParseExpression();
                    Expect(TokenKind.Colon, "expected `:` between dictionary key and value");
                    entries.Add((key, ParseExpression()));
                } while (Match(TokenKind.Comma));
                Expect(TokenKind.RightBrace, "expected `}` after dictionary literal");
            }
            if (entries.Count == 0) return new EmptyDictionaryExpr();
            var keyType = entries[0].Key.Type;
            var valueType = entries[0].Value.Type;
            foreach (var entry in entries.Skip(1))
            {
                if (!IsAssignable(entry.Key.Type, keyType))
                {
                    if (IsAssignable(keyType, entry.Key.Type)) keyType = entry.Key.Type;
                    else entry.Key.WithExpected(keyType, this);
                }
                if (!IsAssignable(entry.Value.Type, valueType))
                {
                    if (IsAssignable(valueType, entry.Value.Type)) valueType = entry.Value.Type;
                    else entry.Value.WithExpected(valueType, this);
                }
            }
            if (keyType != typeof(long) && keyType != typeof(string) && !keyType.IsEnum)
                Error("COFLOW-FUNCTION-TYPE", "dictionary keys must be int, string, or enum");
            var typedEntries = entries.Select(entry => (
                entry.Key.WithExpected(keyType, this),
                entry.Value.WithExpected(valueType, this))).ToArray();
            return new DictionaryExpr(typedEntries,
                typeof(IReadOnlyDictionary<,>).MakeGenericType(keyType, valueType));
        }

        private Expr ParseValueConstructor(string name)
        {
            Expect(TokenKind.LeftParen, $"expected `(` after `{name}`");
            var value = ParseExpression();
            Expect(TokenKind.RightParen, $"expected `)` after `{name}` value");
            return name switch
            {
                "Some" => new SomeExpr(value),
                "Ok" => new ResultBranchExpr(value, IsOk: true),
                "Err" => new ResultBranchExpr(value, IsOk: false),
                _ => throw new InvalidOperationException(),
            };
        }

        private Expr ParseIfExpression()
        {
            var condition = ParseExpression();
            if (condition.Type != typeof(bool))
                Error("COFLOW-FUNCTION-TYPE", "if condition must be bool");
            Expect(TokenKind.LeftBrace, "expected `{` after if condition");
            if (condition is TypeIsExpr { NarrowName: { } name } typeIs)
                _narrowings.Push(new Dictionary<string, Type>(StringComparer.Ordinal) { [name] = typeIs.TargetType });
            var whenTrue = ParseBlockContents();
            if (condition is TypeIsExpr { NarrowName: not null }) _narrowings.Pop();
            Expr whenFalse;
            if (Peek().Kind == TokenKind.Identifier && Peek().Text == "else")
            {
                Advance();
                Expect(TokenKind.LeftBrace, "expected `{` after `else`");
                whenFalse = ParseBlockContents();
            }
            else
            {
                if (whenTrue.Type != typeof(Unit) && !whenTrue.AlwaysTerminates)
                    Error("COFLOW-FUNCTION-TYPE", "if without else must have type `()`");
                whenFalse = new ConstantExpr(Unit.Value, typeof(Unit));
            }
            if (whenTrue.Type == typeof(NoneMarker) &&
                whenFalse.Type.IsGenericType &&
                whenFalse.Type.GetGenericTypeDefinition() == typeof(Option<>))
                whenTrue = whenTrue.WithExpected(whenFalse.Type, this);
            else if (whenFalse.Type == typeof(NoneMarker) &&
                whenTrue.Type.IsGenericType &&
                whenTrue.Type.GetGenericTypeDefinition() == typeof(Option<>))
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
                        $"if branches have different types `{FormatType(whenTrue.Type)}` and `{FormatType(whenFalse.Type)}`");
            }
            return new IfExpr(condition, whenTrue, whenFalse);

            static ResultBranchExpr? ResultBranch(Expr expression) => expression switch
            {
                ResultBranchExpr result => result,
                BlockExpr block => ResultBranch(block.Result),
                _ => null,
            };
        }

        private string ParseTypeName()
        {
            if (Match(TokenKind.LeftParen))
            {
                Expect(TokenKind.RightParen, "only `()` is valid as a tuple type");
                return "()";
            }
            if (Match(TokenKind.LeftBracket))
            {
                var inner = ParseTypeName();
                Expect(TokenKind.RightBracket, "expected `]` in array type");
                return $"[{inner}]";
            }
            if (Match(TokenKind.LeftBrace))
            {
                var key = ParseTypeName();
                Expect(TokenKind.Colon, "expected `:` in dictionary type");
                var value = ParseTypeName();
                Expect(TokenKind.RightBrace, "expected `}` in dictionary type");
                return $"{{{key}:{value}}}";
            }
            var reference = Match(TokenKind.Ampersand);
            var name = Expect(TokenKind.Identifier, "expected a type name").Text;
            if (name == "fn" && Match(TokenKind.LeftParen))
            {
                var parameters = new List<string>();
                if (!Match(TokenKind.RightParen))
                {
                    do parameters.Add(ParseTypeName()); while (Match(TokenKind.Comma));
                    Expect(TokenKind.RightParen, "expected `)` after function parameter types");
                }
                Expect(TokenKind.Arrow, "expected `->` in function type");
                return $"fn({string.Join(",", parameters)})->{ParseTypeName()}";
            }
            while (Match(TokenKind.DoubleColon))
                name += "::" + Expect(TokenKind.Identifier, "expected a name after `::`").Text;
            if (Match(TokenKind.Less))
            {
                var arguments = new List<string>();
                do arguments.Add(ParseTypeName()); while (Match(TokenKind.Comma));
                Expect(TokenKind.Greater, "expected `>` after generic arguments");
                name += $"<{string.Join(",", arguments)}>";
            }
            return reference ? $"&{name}" : name;
        }

        private string FormatType(Type type)
        {
            if (type == typeof(long) || type == typeof(int)) return "int";
            if (type == typeof(double) || type == typeof(float)) return "float";
            if (type == typeof(bool)) return "bool";
            if (type == typeof(string)) return "string";
            if (type == typeof(Unit)) return "()";
            if (_generatedNames.TryGetValue(type, out var generated)) return generated;
            if (typeof(Delegate).IsAssignableFrom(type))
            {
                var invoke = type.GetMethod("Invoke")!;
                var parameters = invoke.GetParameters().Select(parameter => FormatType(parameter.ParameterType));
                var result = invoke.ReturnType == typeof(void) ? "()" : FormatType(invoke.ReturnType);
                return $"fn({string.Join(",", parameters)})->{result}";
            }
            if (type.IsGenericType)
            {
                var definition = type.GetGenericTypeDefinition();
                var arguments = type.GetGenericArguments().Select(FormatType).ToArray();
                if (definition == typeof(Option<>)) return $"Option<{arguments[0]}>";
                if (definition == typeof(Result<,>)) return $"Result<{arguments[0]},{arguments[1]}>";
                if (definition == typeof(IReadOnlyList<>)) return $"[{arguments[0]}]";
                if (definition == typeof(IReadOnlyDictionary<,>)) return $"{{{arguments[0]}:{arguments[1]}}}";
            }
            return type.Name;
        }

        private int Constant(object? value)
        {
            var index = _constants.Count;
            _constants.Add(value);
            return index;
        }

        private int Emit(CoflowOpCode code, int operand = 0)
        {
            var index = _instructions.Count;
            _instructions.Add(new CoflowInstruction(code, operand));
            _instructionSpans.Add(_emissionOffset < 0 || _slot.Source is null
                ? null
                : FunctionSpan(_slot.Source, _emissionOffset));
            return index;
        }

        private int _emissionOffset = -1;

        private int SetEmissionOffset(int offset)
        {
            var previous = _emissionOffset;
            if (offset >= 0) _emissionOffset = offset;
            return previous;
        }

        private void RestoreEmissionOffset(int offset) => _emissionOffset = offset;

        private void Patch(int index, int target) => _instructions[index] =
            _instructions[index] with { Operand = target };

        private CoflowProgram CompileLambda(
            CoflowFunctionSignature signature,
            int captureCount,
            Expr body)
        {
            var outerInstructions = _instructions.ToArray();
            var outerInstructionSpans = _instructionSpans.ToArray();
            var outerConstants = _constants.ToArray();
            _instructions.Clear();
            _instructionSpans.Clear();
            _constants.Clear();
            body.EmitTail(this);
            var program = new CoflowProgram(
                _slot.Identity,
                _slot.SourcePath,
                _slot.SourceSpan,
                _instructions.ToArray(),
                _instructionSpans.ToArray(),
                _constants.ToArray(),
                signature.ParameterTypes.Count + captureCount,
                _localCount);
            _instructions.Clear();
            _instructions.AddRange(outerInstructions);
            _instructionSpans.Clear();
            _instructionSpans.AddRange(outerInstructionSpans);
            _constants.Clear();
            _constants.AddRange(outerConstants);
            return program;
        }

        private Token Peek() => _tokens[_index];
        private Token Advance() => _tokens[_index++];
        private bool Match(TokenKind kind)
        {
            if (Peek().Kind != kind) return false;
            _index++;
            return true;
        }
        private Token Expect(TokenKind kind, string message)
        {
            if (Peek().Kind != kind) Error("COFLOW-FUNCTION-SYNTAX", message);
            return Advance();
        }
        private Token ExpectBindingIdentifier(string message)
        {
            var token = Expect(TokenKind.Identifier, message);
            ValidateBindingIdentifier(token);
            return token;
        }
        private void ValidateBindingIdentifier(Token token)
        {
            if (!CfdIdentifiers.IsIdentifier(token.Text))
                Error("COFLOW-FUNCTION-NAME", $"`{token.Text}` is reserved and cannot be used as a binding");
            if (_ownerFieldNames.Contains(token.Text))
                Error("COFLOW-FUNCTION-NAME",
                    $"binding `{token.Text}` conflicts with a field on `{_slot.Identity.DeclaredType}`");
        }
        private void ExpectIdentifier(string value)
        {
            var token = Expect(TokenKind.Identifier, $"expected `{value}`");
            if (token.Text != value) Error("COFLOW-FUNCTION-SYNTAX", $"expected `{value}`");
        }
        [DoesNotReturn]
        private void Error(string code, string message) =>
            throw new FunctionCompileException(code, message, Peek().Offset);

        private abstract record Expr(Type Type)
        {
            internal int SourceOffset { get; init; } = -1;

            internal Expr At(int offset) => SourceOffset >= 0 ? this : this with { SourceOffset = offset };

            internal void Emit(FunctionParser parser)
            {
                var previous = parser.SetEmissionOffset(SourceOffset);
                try { EmitCore(parser); }
                finally { parser.RestoreEmissionOffset(previous); }
            }
            internal void EmitTail(FunctionParser parser)
            {
                var previous = parser.SetEmissionOffset(SourceOffset);
                try { EmitTailCore(parser); }
                finally { parser.RestoreEmissionOffset(previous); }
            }
            internal abstract void EmitCore(FunctionParser parser);
            internal virtual void EmitTailCore(FunctionParser parser)
            {
                EmitCore(parser);
                parser.Emit(CoflowOpCode.Return);
            }
            internal virtual bool AlwaysTerminates => false;
            internal virtual CoflowFunctionSignature? CallableSignature =>
                DelegateSignature(Type);

            internal virtual Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!IsAssignable(Type, expected))
                    parser.Error("COFLOW-FUNCTION-TYPE",
                        $"expression has type `{parser.FormatType(Type)}` but `{parser.FormatType(expected)}` is required");
                return Type == expected ? this : new RetypedExpr(this, expected);
            }
        }

        private static CoflowFunctionSignature? DelegateSignature(Type type)
        {
            if (!typeof(Delegate).IsAssignableFrom(type)) return null;
            var invoke = type.GetMethod("Invoke");
            if (invoke is null) return null;
            return new CoflowFunctionSignature(
                invoke.ReturnType == typeof(void) ? typeof(Unit) : invoke.ReturnType,
                invoke.GetParameters().Select(parameter => parameter.ParameterType).ToArray());
        }

        private static bool IsAssignable(Type source, Type target)
        {
            if (source == target) return true;
            var sourceFunction = DelegateSignature(source);
            var targetFunction = DelegateSignature(target);
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
            var parameters = signature.ParameterTypes.ToArray();
            if (signature.ResultType == typeof(Unit))
                return parameters.Length == 0 ? typeof(Action) : System.Linq.Expressions.Expression.GetActionType(parameters);
            return System.Linq.Expressions.Expression.GetFuncType(parameters.Append(signature.ResultType).ToArray());
        }

        private sealed class NoneMarker { }
        private sealed class ResultBranchMarker { }

        private sealed record ConstantExpr(object? Value, Type ValueType) : Expr(ValueType)
        {
            internal override void EmitCore(FunctionParser parser) =>
                parser.Emit(CoflowOpCode.Constant, parser.Constant(Value));
        }

        private sealed record RetypedExpr(Expr Value, Type ExpectedType) : Expr(ExpectedType)
        {
            internal override void EmitCore(FunctionParser parser) => Value.Emit(parser);
        }

        private sealed record InterpolationPart(string? Text, Expr? Value);

        private sealed record InterpolatedStringExpr(
            IReadOnlyList<InterpolationPart> Parts) : Expr(typeof(string))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                var values = Parts.Where(part => part.Value is not null).ToArray();
                foreach (var part in values) part.Value!.Emit(parser);
                var metadata = parser._metadata;
                var enums = parser._enums;
                parser.Emit(CoflowOpCode.Native, parser.Constant(new CoflowNativeCall(arguments =>
                {
                    var rendered = new System.Text.StringBuilder();
                    var argument = 0;
                    foreach (var part in Parts)
                    {
                        if (part.Text is not null)
                            rendered.Append(part.Text);
                        else
                        {
                            var expression = part.Value!;
                            rendered.Append(RenderInterpolatedValue(
                                arguments[argument++], expression.Type, metadata, enums, false));
                        }
                    }
                    return rendered.ToString();
                }, values.Length)));
            }
        }

        private static string RenderInterpolatedValue(
            object? value,
            Type type,
            IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
            IReadOnlyDictionary<string, ICoflowEnumMetadata> enums,
            bool nested)
        {
            if (value is null) throw new InvalidOperationException("Coflow values cannot be null.");
            if (type == typeof(string))
                return nested ? $"\"{EscapeInterpolatedString((string)value)}\"" : (string)value;
            if (type == typeof(long)) return ((long)value).ToString(CultureInfo.InvariantCulture);
            if (type == typeof(double)) return ((double)value).ToString("R", CultureInfo.InvariantCulture);
            if (type == typeof(bool)) return (bool)value ? "true" : "false";
            if (type == typeof(Unit)) return "()";
            if (type.IsEnum)
            {
                var enumMetadata = enums.Values.Single(item => item.RuntimeType == type);
                return enumMetadata.Variants.FirstOrDefault(item => Equals(item.Value, value)).Key
                    ?? Convert.ToInt64(value, CultureInfo.InvariantCulture)
                        .ToString(CultureInfo.InvariantCulture);
            }
            if (type.IsGenericType)
            {
                var definition = type.GetGenericTypeDefinition();
                var arguments = type.GetGenericArguments();
                if (definition == typeof(Option<>))
                {
                    var hasValue = (bool)type.GetProperty("HasValue")!.GetValue(value)!;
                    return hasValue
                        ? $"Some({RenderInterpolatedValue(type.GetProperty("Value")!.GetValue(value), arguments[0], metadata, enums, true)})"
                        : "None";
                }
                if (definition == typeof(Result<,>))
                {
                    var isOk = (bool)type.GetProperty("IsOk")!.GetValue(value)!;
                    var property = type.GetProperty(isOk ? "Value" : "Error")!;
                    return $"{(isOk ? "Ok" : "Err")}({RenderInterpolatedValue(property.GetValue(value), arguments[isOk ? 0 : 1], metadata, enums, true)})";
                }
                if (definition == typeof(IReadOnlyList<>))
                {
                    var items = ((System.Collections.IEnumerable)value).Cast<object?>()
                        .Select(item => RenderInterpolatedValue(
                            item, arguments[0], metadata, enums, true));
                    return $"[{string.Join(", ", items)}]";
                }
                if (definition == typeof(IReadOnlyDictionary<,>))
                {
                    var entries = ((System.Collections.IEnumerable)value).Cast<object>().Select(entry =>
                    {
                        var entryType = entry.GetType();
                        var key = entryType.GetProperty("Key")!.GetValue(entry);
                        var item = entryType.GetProperty("Value")!.GetValue(entry);
                        return $"{RenderInterpolatedValue(key, arguments[0], metadata, enums, true)}: {RenderInterpolatedValue(item, arguments[1], metadata, enums, true)}";
                    });
                    return $"{{ {string.Join(", ", entries)} }}";
                }
            }
            var objectMetadata = metadata.Values.SingleOrDefault(item => item.RuntimeType == type)
                ?? throw new InvalidOperationException($"type `{type}` has no interpolation renderer");
            if (objectMetadata.IsRecord)
            {
                var key = objectMetadata.GetKey(value);
                var renderedKey = key is string text
                    ? text
                    : enums.Values.Single(item => item.RuntimeType == key.GetType())
                        .Variants.Single(item => Equals(item.Value, key)).Key;
                if (renderedKey.Length != 0)
                    return $"&{objectMetadata.DeclaredType}::{renderedKey}";
            }
            var fields = objectMetadata.FieldNames.Select(field =>
                $"{field}: {RenderInterpolatedValue(objectMetadata.GetField(value, field), objectMetadata.GetFieldType(field), metadata, enums, true)}");
            return $"{objectMetadata.DeclaredType} {{ {string.Join(", ", fields)} }}";
        }

        private static string EscapeInterpolatedString(string value) => value
            .Replace("\\", "\\\\", StringComparison.Ordinal)
            .Replace("\"", "\\\"", StringComparison.Ordinal)
            .Replace("\n", "\\n", StringComparison.Ordinal)
            .Replace("\r", "\\r", StringComparison.Ordinal)
            .Replace("\t", "\\t", StringComparison.Ordinal);

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
                foreach (var field in Fields) field.Value.Emit(parser);
                var names = Fields.Select(field => field.Name).ToArray();
                parser.Emit(CoflowOpCode.Native, parser.Constant(new CoflowNativeCall(values =>
                {
                    var supplied = new Dictionary<string, object?>(StringComparer.Ordinal);
                    for (var index = 0; index < names.Length; index++)
                        supplied.Add(names[index], values[index]);
                    return Metadata.CreateObject(Context, supplied);
                }, Fields.Count)));
            }
        }

        private sealed record NoneExpr() : Expr(typeof(NoneMarker))
        {
            internal override Expr WithExpected(Type expected, FunctionParser parser)
            {
                if (!expected.IsGenericType || expected.GetGenericTypeDefinition() != typeof(Option<>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "`None` requires an Option result type");
                return new ConstantExpr(Activator.CreateInstance(expected), expected);
            }

            internal override void EmitCore(FunctionParser parser) =>
                throw new InvalidOperationException("None must be resolved against an expected type.");
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
                parser.Emit(CoflowOpCode.Construct, parser.Constant(ValueFactories.OptionSome(Value.Type)));
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
                parser.Emit(CoflowOpCode.Construct, parser.Constant(
                    IsOk
                        ? ValueFactories.ResultOk(arguments[0], arguments[1])
                        : ValueFactories.ResultErr(arguments[0], arguments[1])));
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
                parser.Emit(CoflowOpCode.Construct, parser.Constant(
                    new Func<object?, object?>(value => value is not null && TargetType.IsInstanceOfType(value))));
            }
        }

        private sealed record FunctionReferenceExpr(CoflowFunctionSlot Slot) : Expr(DelegateType(Slot.Signature))
        {
            internal override CoflowFunctionSignature CallableSignature => Slot.Signature;
            internal override void EmitCore(FunctionParser parser) =>
                parser.Emit(CoflowOpCode.Constant, parser.Constant(Slot));
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
                var program = parser.CompileLambda(Signature, Captures.Count, Body);
                parser.Emit(CoflowOpCode.MakeClosure,
                    parser.Constant(new CoflowClosureTemplate(program, Captures.Count)));
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
                    parser.Emit(CoflowOpCode.Call,
                        parser.Constant(new CoflowCallSite(direct.Slot, Arguments.Count)));
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
                    parser.Emit(CoflowOpCode.TailCall,
                        parser.Constant(new CoflowCallSite(direct.Slot, Arguments.Count)));
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
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(
                        ValueFactories.Array(ArrayType.GetGenericArguments()[0]), Values.Count)));
            }
        }

        private sealed record DictionaryExpr(
            IReadOnlyList<(Expr Key, Expr Value)> Entries,
            Type DictionaryType) : Expr(DictionaryType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                foreach (var entry in Entries) { entry.Key.Emit(parser); entry.Value.Emit(parser); }
                var types = DictionaryType.GetGenericArguments();
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(
                        ValueFactories.Dictionary(types[0], types[1]), Entries.Count * 2)));
            }
        }

        private sealed record IndexExpr(Expr Receiver, Expr Index, Type ResultType, Func<object?[], object?> Invoke)
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
                        typeof(Option<>).MakeGenericType(arguments[0]), ValueFactories.ArrayIndex(arguments[0]));
                }
                if (definition == typeof(IReadOnlyDictionary<,>))
                {
                    index.WithExpected(arguments[0], parser);
                    return new IndexExpr(receiver, index,
                        typeof(Option<>).MakeGenericType(arguments[1]), ValueFactories.DictionaryIndex(arguments[0], arguments[1]));
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
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(Invoke, 2)));
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

        private sealed record TransformExpr(Expr Receiver, Type ResultType, Func<object?, object?> Transform)
            : Expr(ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Receiver.Emit(parser);
                parser.Emit(CoflowOpCode.Construct,
                    parser.Constant(new Func<object?, object?>(Transform)));
            }
        }

        private sealed record PropagateExpr(
            Expr Operand,
            Type ValueType,
            Func<object?, CoflowPropagationResult> Propagate) : Expr(ValueType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Operand.Emit(parser);
                parser.Emit(CoflowOpCode.Propagate, parser.Constant(Propagate));
            }
        }

        private sealed record MatchArm(MatchPattern Pattern, int? BindingLocal, Expr Body);

        private sealed record MatchExpr(
            Expr Subject,
            int SubjectLocal,
            IReadOnlyList<MatchArm> Arms,
            bool LastIsComplement) : Expr(Arms[0].Body.Type)
        {
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
                        parser.Emit(CoflowOpCode.Construct, parser.Constant(arm.Pattern.Test));
                        next = parser.Emit(CoflowOpCode.JumpIfFalse);
                    }
                    if (arm.BindingLocal is { } binding)
                    {
                        parser.Emit(CoflowOpCode.Local, SubjectLocal);
                        parser.Emit(CoflowOpCode.Construct, parser.Constant(arm.Pattern.Extract!));
                        parser.Emit(CoflowOpCode.StoreLocal, binding);
                    }
                    arm.Body.Emit(parser);
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
            Func<object?, object?> Test,
            Func<object?, object?>? Extract)
        {
            internal static MatchPattern CatchAll(string kind, string? name, Type? type) =>
                new(kind, true, name, type, static _ => true, static value => value);
            internal static MatchPattern Literal(string kind, object value) =>
                new(kind, false, null, null, candidate => Equals(candidate, value), null);
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
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(Builtin.Invoke, Arguments.Count + 1)));
            }
        }

        private sealed record HigherOrderExpr(
            Expr Receiver,
            IReadOnlyList<Expr> Arguments,
            CoflowHigherOrderOperation Operation) : Expr(Operation.ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Receiver.Emit(parser);
                foreach (var argument in Arguments) argument.Emit(parser);
                parser.Emit(CoflowOpCode.HigherOrder, parser.Constant(Operation));
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
                if (!Body.AlwaysTerminates) parser.Emit(CoflowOpCode.Pop);
                parser.Emit(CoflowOpCode.Jump, start);
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
            Expr Body,
            CoflowLoopAccess Access) : Expr(typeof(Unit))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Collection.Emit(parser);
                parser.Emit(CoflowOpCode.Construct, parser.Constant(Access.Prepare));
                parser.Emit(CoflowOpCode.StoreLocal, CollectionLocal);
                parser.Emit(CoflowOpCode.Constant, parser.Constant(0L));
                parser.Emit(CoflowOpCode.StoreLocal, IndexLocal);
                var condition = parser._instructions.Count;
                parser.Emit(CoflowOpCode.Local, IndexLocal);
                parser.Emit(CoflowOpCode.Local, CollectionLocal);
                parser.Emit(CoflowOpCode.Construct, parser.Constant(Access.Count));
                parser.Emit(CoflowOpCode.LessInt);
                var done = parser.Emit(CoflowOpCode.JumpIfFalse);

                parser.Emit(CoflowOpCode.Local, CollectionLocal);
                parser.Emit(CoflowOpCode.Local, IndexLocal);
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(Access.First, 2)));
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
                        parser.Emit(CoflowOpCode.Native,
                            parser.Constant(new CoflowNativeCall(Access.Second!, 2)));
                    }
                    parser.Emit(CoflowOpCode.StoreLocal, second);
                }

                var loop = new LoopEmitContext(-1);
                parser._loops.Push(loop);
                Body.Emit(parser);
                parser._loops.Pop();
                if (!Body.AlwaysTerminates) parser.Emit(CoflowOpCode.Pop);
                var increment = parser._instructions.Count;
                loop.ContinueTarget = increment;
                foreach (var jump in loop.ContinueJumps) parser.Patch(jump, increment);
                parser.Emit(CoflowOpCode.Local, IndexLocal);
                parser.Emit(CoflowOpCode.Constant, parser.Constant(1L));
                parser.Emit(CoflowOpCode.AddInt);
                parser.Emit(CoflowOpCode.StoreLocal, IndexLocal);
                parser.Emit(CoflowOpCode.Jump, condition);
                var end = parser._instructions.Count;
                parser.Patch(done, end);
                foreach (var jump in loop.BreakJumps) parser.Patch(jump, end);
            }
        }

        private sealed record RangeExpr(Expr Start, Expr End, bool Inclusive) : Expr(typeof(CoflowRange))
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Start.Emit(parser);
                End.Emit(parser);
                parser.Emit(CoflowOpCode.Native, parser.Constant(new CoflowNativeCall(
                    values => new CoflowRange((long)values[0]!, (long)values[1]!, Inclusive), 2)));
            }
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
                foreach (var statement in Statements) statement.Emit(parser);
                Result.Emit(parser);
            }


            internal override void EmitTailCore(FunctionParser parser)
            {
                foreach (var statement in Statements) statement.Emit(parser);
                Result.EmitTail(parser);
            }
        }

        private sealed record IfExpr(Expr Condition, Expr WhenTrue, Expr WhenFalse) : Expr(WhenTrue.Type)
        {
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
                var done = parser.Emit(CoflowOpCode.Jump);
                parser.Patch(otherwise, parser._instructions.Count);
                WhenFalse.Emit(parser);
                parser.Patch(done, parser._instructions.Count);
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
            private static readonly System.Reflection.MethodInfo OptionSomeMethod =
                typeof(ValueFactories).GetMethod(nameof(CreateOptionSome),
                    System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
            private static readonly System.Reflection.MethodInfo ResultOkMethod =
                typeof(ValueFactories).GetMethod(nameof(CreateResultOk),
                    System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
            private static readonly System.Reflection.MethodInfo ResultErrMethod =
                typeof(ValueFactories).GetMethod(nameof(CreateResultErr),
                    System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
            private static readonly System.Reflection.MethodInfo ArrayMethod = Method(nameof(CreateArray));
            private static readonly System.Reflection.MethodInfo DictionaryMethod = Method(nameof(CreateDictionary));
            private static readonly System.Reflection.MethodInfo ArrayIndexMethod = Method(nameof(IndexArray));
            private static readonly System.Reflection.MethodInfo DictionaryIndexMethod = Method(nameof(IndexDictionary));
            private static readonly System.Reflection.MethodInfo PrepareArrayLoopMethod = Method(nameof(PrepareArrayLoop));
            private static readonly System.Reflection.MethodInfo ArrayLoopCountMethod = Method(nameof(ArrayLoopCount));
            private static readonly System.Reflection.MethodInfo ArrayLoopItemMethod = Method(nameof(ArrayLoopItem));
            private static readonly System.Reflection.MethodInfo PrepareDictionaryLoopMethod = Method(nameof(PrepareDictionaryLoop));
            private static readonly System.Reflection.MethodInfo ArrayCountMethod = Method(nameof(ArrayCount));
            private static readonly System.Reflection.MethodInfo ArrayItemMethod = Method(nameof(ArrayItem));
            private static readonly System.Reflection.MethodInfo OptionPropagationMethod = Method(nameof(PropagateOption));
            private static readonly System.Reflection.MethodInfo ResultPropagationMethod = Method(nameof(PropagateResult));
            private static readonly System.Reflection.MethodInfo OptionMatchMethod = Method(nameof(MatchOption));
            private static readonly System.Reflection.MethodInfo ResultMatchMethod = Method(nameof(MatchResult));

            internal static Func<object?, object?> OptionSome(Type value) =>
                (Func<object?, object?>)OptionSomeMethod.MakeGenericMethod(value)
                    .CreateDelegate(typeof(Func<object?, object?>));

            internal static Func<object?, object?> ResultOk(Type value, Type error) =>
                (Func<object?, object?>)ResultOkMethod.MakeGenericMethod(value, error)
                    .CreateDelegate(typeof(Func<object?, object?>));

            internal static Func<object?, object?> ResultErr(Type value, Type error) =>
                (Func<object?, object?>)ResultErrMethod.MakeGenericMethod(value, error)
                    .CreateDelegate(typeof(Func<object?, object?>));

            internal static Func<object?[], object?> Array(Type element) => Generic(ArrayMethod, element);
            internal static Func<object?[], object?> Dictionary(Type key, Type value) => Generic(DictionaryMethod, key, value);
            internal static Func<object?[], object?> ArrayIndex(Type element) => Generic(ArrayIndexMethod, element);
            internal static Func<object?[], object?> DictionaryIndex(Type key, Type value) => Generic(DictionaryIndexMethod, key, value);
            internal static CoflowLoopAccess ArrayLoop(Type element) => new(
                GenericUnary(PrepareArrayLoopMethod, element),
                GenericUnary(ArrayLoopCountMethod, element),
                Generic(ArrayLoopItemMethod, element),
                null);
            internal static CoflowLoopAccess DictionaryLoop(Type key, Type value) => new(
                GenericUnary(PrepareDictionaryLoopMethod, key, value),
                static entries => (long)((object?[][])entries!).Length,
                static values => ((object?[][])values[0]!)[checked((int)(long)values[1]!)][0],
                static values => ((object?[][])values[0]!)[checked((int)(long)values[1]!)][1]);
            internal static CoflowLoopAccess RangeLoop() => new(
                static value => value,
                static value => ((CoflowRange)value!).Count,
                static values => checked(((CoflowRange)values[0]!).Start + (long)values[1]!),
                null);
            internal static Func<object?, CoflowPropagationResult> OptionPropagation(Type value, Type result) =>
                GenericPropagation(OptionPropagationMethod, value, result);
            internal static Func<object?, CoflowPropagationResult> ResultPropagation(Type value, Type error, Type result) =>
                GenericPropagation(ResultPropagationMethod, value, error, result);
            internal static CoflowHigherOrderOperation HigherOrder(
                string name, Type element, Type outputElement, Type resultType) => new(
                    name,
                    resultType,
                    (Func<object?, int>)ArrayCountMethod.MakeGenericMethod(element)
                        .CreateDelegate(typeof(Func<object?, int>)),
                    (Func<object?, int, object?>)ArrayItemMethod.MakeGenericMethod(element)
                        .CreateDelegate(typeof(Func<object?, int, object?>)),
                    Array(outputElement),
                    OptionSome(outputElement),
                    Activator.CreateInstance(typeof(Option<>).MakeGenericType(outputElement)));
            internal static MatchPattern MatchNone(Type subject, FunctionParser parser)
            {
                if (!subject.IsGenericType || subject.GetGenericTypeDefinition() != typeof(Option<>))
                    parser.Error("COFLOW-FUNCTION-TYPE", "None pattern requires Option");
                var access = GenericMatch(OptionMatchMethod, subject.GetGenericArguments()[0]);
                return new MatchPattern("None", false, null, null,
                    value => !(bool)access(value)[0]!, null);
            }
            internal static MatchPattern MatchBranch(Type subject, string kind, string binding, FunctionParser parser)
            {
                if (subject.IsGenericType && subject.GetGenericTypeDefinition() == typeof(Option<>) && kind == "Some")
                {
                    var type = subject.GetGenericArguments()[0];
                    var access = GenericMatch(OptionMatchMethod, type);
                    return new MatchPattern(kind, false, binding, type,
                        value => (bool)access(value)[0]!, value => access(value)[1]);
                }
                if (subject.IsGenericType && subject.GetGenericTypeDefinition() == typeof(Result<,>) && kind is "Ok" or "Err")
                {
                    var types = subject.GetGenericArguments();
                    var access = GenericMatch(ResultMatchMethod, types);
                    var ok = kind == "Ok";
                    return new MatchPattern(kind, false, binding, types[ok ? 0 : 1],
                        value => (bool)access(value)[0]! == ok,
                        value => access(value)[ok ? 1 : 2]);
                }
                parser.Error("COFLOW-FUNCTION-TYPE", $"{kind} pattern does not match subject type");
                return null!;
            }

            private static object CreateOptionSome<T>(object? value) =>
                Option<T>.Some(CoflowFunctionDelegates.Adapt<T>(value));
            private static object CreateResultOk<T, TError>(object? value) =>
                Result<T, TError>.Ok(CoflowFunctionDelegates.Adapt<T>(value));
            private static object CreateResultErr<T, TError>(object? error) =>
                Result<T, TError>.Err(CoflowFunctionDelegates.Adapt<TError>(error));
            private static object CreateArray<T>(object?[] values) =>
                System.Array.AsReadOnly(values.Select(CoflowFunctionDelegates.Adapt<T>).ToArray());
            private static object CreateDictionary<TKey, TValue>(object?[] values) where TKey : notnull
            {
                var result = new Dictionary<TKey, TValue>();
                for (var index = 0; index < values.Length; index += 2)
                    result.Add(
                        CoflowFunctionDelegates.Adapt<TKey>(values[index]),
                        CoflowFunctionDelegates.Adapt<TValue>(values[index + 1]));
                return new System.Collections.ObjectModel.ReadOnlyDictionary<TKey, TValue>(result);
            }
            private static object IndexArray<T>(object?[] values)
            {
                var array = (IReadOnlyList<T>)values[0]!;
                var index = (long)values[1]!;
                return index >= 0 && index < array.Count ? Option<T>.Some(array[(int)index]) : Option<T>.None;
            }
            private static object IndexDictionary<TKey, TValue>(object?[] values) where TKey : notnull
            {
                var dictionary = (IReadOnlyDictionary<TKey, TValue>)values[0]!;
                return dictionary.TryGetValue((TKey)values[1]!, out var value)
                    ? Option<TValue>.Some(value) : Option<TValue>.None;
            }
            private static object PrepareArrayLoop<T>(object? value) =>
                (IReadOnlyList<T>)value!;
            private static object ArrayLoopCount<T>(object? value) =>
                (long)((IReadOnlyList<T>)value!).Count;
            private static object? ArrayLoopItem<T>(object?[] values) =>
                ((IReadOnlyList<T>)values[0]!)[checked((int)(long)values[1]!)];
            private static int ArrayCount<T>(object? value) =>
                ((IReadOnlyList<T>)value!).Count;
            private static object? ArrayItem<T>(object? value, int index) =>
                ((IReadOnlyList<T>)value!)[index];
            private static object PrepareDictionaryLoop<TKey, TValue>(object? value) where TKey : notnull =>
                ((IReadOnlyDictionary<TKey, TValue>)value!)
                    .Select(entry => new object?[] { entry.Key, entry.Value }).ToArray();
            private static CoflowPropagationResult PropagateOption<T, TResult>(object? value)
            {
                var option = (Option<T>)value!;
                return option.HasValue
                    ? new CoflowPropagationResult(true, option.Value)
                    : new CoflowPropagationResult(false, Option<TResult>.None);
            }
            private static CoflowPropagationResult PropagateResult<T, TError, TResult>(object? value)
            {
                var result = (Result<T, TError>)value!;
                return result.IsOk
                    ? new CoflowPropagationResult(true, result.Value)
                    : new CoflowPropagationResult(false, Result<TResult, TError>.Err(result.Error));
            }
            private static object?[] MatchOption<T>(object? value)
            {
                var option = (Option<T>)value!;
                return new object?[] { option.HasValue, option.HasValue ? option.Value : default };
            }
            private static object?[] MatchResult<T, TError>(object? value)
            {
                var result = (Result<T, TError>)value!;
                return new object?[] { result.IsOk, result.IsOk ? result.Value : default, result.IsErr ? result.Error : default };
            }

            private static System.Reflection.MethodInfo Method(string name) =>
                typeof(ValueFactories).GetMethod(name,
                    System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!;
            private static Func<object?[], object?> Generic(System.Reflection.MethodInfo method, params Type[] arguments) =>
                (Func<object?[], object?>)method.MakeGenericMethod(arguments)
                    .CreateDelegate(typeof(Func<object?[], object?>));
            private static Func<object?, object?> GenericUnary(System.Reflection.MethodInfo method, params Type[] arguments) =>
                (Func<object?, object?>)method.MakeGenericMethod(arguments)
                    .CreateDelegate(typeof(Func<object?, object?>));
            private static Func<object?, CoflowPropagationResult> GenericPropagation(
                System.Reflection.MethodInfo method, params Type[] arguments) =>
                (Func<object?, CoflowPropagationResult>)method.MakeGenericMethod(arguments)
                    .CreateDelegate(typeof(Func<object?, CoflowPropagationResult>));
            private static Func<object?, object?[]> GenericMatch(
                System.Reflection.MethodInfo method, params Type[] arguments) =>
                (Func<object?, object?[]>)method.MakeGenericMethod(arguments)
                    .CreateDelegate(typeof(Func<object?, object?[]>));
        }

        private sealed record UnaryExpr(string Operation, Expr Operand, Type ResultType, CoflowOpCode Code) : Expr(ResultType)
        {
            internal static Expr Create(string operation, Expr operand, FunctionParser parser)
            {
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
                parser.Emit(CoflowOpCode.Construct, parser.Constant(
                    new Func<object?, object?>(value => Metadata.FromInt64(~Convert.ToInt64(value)))));
            }
        }

        private sealed record EnumBinaryExpr(
            string Operation,
            Expr Left,
            Expr Right,
            Type ResultType,
            Func<object?[], object?> Invoke) : Expr(ResultType)
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
                    return new EnumBinaryExpr(operation, left, right, left.Type, values =>
                    {
                        var lhs = Convert.ToInt64(values[0]);
                        var rhs = Convert.ToInt64(values[1]);
                        return metadata.FromInt64(operation switch { "&" => lhs & rhs, "|" => lhs | rhs, _ => lhs ^ rhs });
                    });
                }
                if (operation is "==" or "!=")
                    return new EnumBinaryExpr(operation, left, right, typeof(bool), values =>
                        operation == "==" ? Equals(values[0], values[1]) : !Equals(values[0], values[1]));
                if (operation is "<" or "<=" or ">" or ">=")
                    return new EnumBinaryExpr(operation, left, right, typeof(bool), values =>
                    {
                        var order = Convert.ToInt64(values[0]).CompareTo(Convert.ToInt64(values[1]));
                        return operation switch { "<" => order < 0, "<=" => order <= 0, ">" => order > 0, _ => order >= 0 };
                    });
                parser.Error("COFLOW-FUNCTION-TYPE", $"operator `{operation}` cannot be applied to enum");
                return null!;
            }

            internal override void EmitCore(FunctionParser parser)
            {
                Left.Emit(parser);
                Right.Emit(parser);
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(Invoke, 2)));
            }
        }

        private sealed record ConversionExpr(Expr Value, Type ResultType, Func<object?, object?> Convert)
            : Expr(ResultType)
        {
            internal override void EmitCore(FunctionParser parser)
            {
                Value.Emit(parser);
                parser.Emit(CoflowOpCode.Construct, parser.Constant(Convert));
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
                return new BinaryExpr(operation, left, right, result, code);
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
                var type = Left.Type;
                var metadata = Metadata.Values.ToArray();
                parser.Emit(CoflowOpCode.Native,
                    parser.Constant(new CoflowNativeCall(values =>
                    {
                        var equal = CoflowValueEquality.Equal(values[0], values[1], type, metadata);
                        return Negated ? !equal : equal;
                    }, 2)));
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

    private static List<Token> Lex(string source)
    {
        var tokens = new List<Token>();
        var index = 0;
        while (index < source.Length)
        {
            var character = source[index];
            if (char.IsWhiteSpace(character)) { index++; continue; }
            if (character == '#')
            {
                while (index < source.Length && source[index] != '\n') index++;
                continue;
            }
            var start = index;
            var identifierEnd = index;
            if (CfdIdentifiers.TryRead(source, ref identifierEnd))
            {
                index = identifierEnd;
                tokens.Add(new Token(TokenKind.Identifier, source[start..index], start));
                continue;
            }
            if (char.IsDigit(character))
            {
                index++;
                while (index < source.Length && char.IsDigit(source[index])) index++;
                var numberKind = TokenKind.Integer;
                if (index < source.Length && source[index] == '.' && index + 1 < source.Length && char.IsDigit(source[index + 1]))
                {
                    numberKind = TokenKind.Float;
                    index++;
                    while (index < source.Length && char.IsDigit(source[index])) index++;
                }
                if (index < source.Length && source[index] is 'e' or 'E')
                {
                    numberKind = TokenKind.Float;
                    index++;
                    if (index < source.Length && source[index] is '+' or '-') index++;
                    while (index < source.Length && char.IsDigit(source[index])) index++;
                }
                tokens.Add(new Token(numberKind, source[start..index], start));
                continue;
            }
            if (character == '"')
            {
                index++;
                var value = new System.Text.StringBuilder();
                var pieces = new List<Token>();
                var textOffset = index;
                var interpolated = false;
                while (index < source.Length && source[index] != '"')
                {
                    if (source[index] == '\\')
                    {
                        index++;
                        if (index >= source.Length) throw new FunctionCompileException(
                            "COFLOW-FUNCTION-SYNTAX", "unterminated string escape", start);
                        value.Append(source[index++] switch
                        {
                            '"' => '"', '\\' => '\\', 'n' => '\n', 'r' => '\r', 't' => '\t',
                            var escape => throw new FunctionCompileException(
                                "COFLOW-FUNCTION-SYNTAX", $"unknown string escape `\\{escape}`", start),
                        });
                        continue;
                    }
                    if (source[index] == '{')
                    {
                        if (index + 1 < source.Length && source[index + 1] == '{')
                        {
                            value.Append('{');
                            index += 2;
                            continue;
                        }
                        interpolated = true;
                        if (value.Length != 0)
                        {
                            pieces.Add(new Token(TokenKind.String, value.ToString(), textOffset));
                            value.Clear();
                        }
                        var open = index;
                        var close = FindInterpolationEnd(source, open + 1);
                        if (close < 0) throw new FunctionCompileException(
                            "COFLOW-FUNCTION-SYNTAX", "unterminated string interpolation", open);
                        pieces.Add(new Token(TokenKind.InterpolationStart, "{", open));
                        var expressionStart = open + 1;
                        var expressionSource = source[expressionStart..close];
                        List<Token> expressionTokens;
                        try { expressionTokens = Lex(expressionSource); }
                        catch (FunctionCompileException error)
                        {
                            throw new FunctionCompileException(
                                error.Code,
                                error.Message,
                                expressionStart + (error.Offset ?? 0));
                        }
                        foreach (var expressionToken in expressionTokens.Take(expressionTokens.Count - 1))
                            pieces.Add(expressionToken with
                            {
                                Offset = expressionStart + expressionToken.Offset,
                            });
                        pieces.Add(new Token(TokenKind.InterpolationEnd, "}", close));
                        index = close + 1;
                        textOffset = index;
                        continue;
                    }
                    if (source[index] == '}')
                    {
                        if (index + 1 < source.Length && source[index + 1] == '}')
                        {
                            value.Append('}');
                            index += 2;
                            continue;
                        }
                        throw new FunctionCompileException(
                            "COFLOW-FUNCTION-SYNTAX",
                            "unmatched `}` in string literal; use `}}` for a literal brace",
                            index);
                    }
                    value.Append(source[index++]);
                }
                if (index >= source.Length) throw new FunctionCompileException(
                    "COFLOW-FUNCTION-SYNTAX", "unterminated string literal", start);
                index++;
                if (!interpolated)
                {
                    tokens.Add(new Token(TokenKind.String, value.ToString(), start));
                    continue;
                }
                if (value.Length != 0)
                    pieces.Add(new Token(TokenKind.String, value.ToString(), textOffset));
                tokens.Add(new Token(TokenKind.InterpolatedStringStart, "\"", start));
                tokens.AddRange(pieces);
                tokens.Add(new Token(TokenKind.InterpolatedStringEnd, "\"", index - 1));
                continue;
            }

            var three = index + 2 < source.Length ? source.Substring(index, 3) : string.Empty;
            if (three == "..=")
            {
                tokens.Add(new Token(TokenKind.DotDotEqual, three, start));
                index += 3;
                continue;
            }
            var two = index + 1 < source.Length ? source.Substring(index, 2) : string.Empty;
            var pairKind = two switch
            {
                "->" => TokenKind.Arrow, "::" => TokenKind.DoubleColon, "//" => TokenKind.DoubleSlash,
                "==" => TokenKind.EqualEqual, "!=" => TokenKind.BangEqual, "<=" => TokenKind.LessEqual,
                ">=" => TokenKind.GreaterEqual, "&&" => TokenKind.AndAnd, "||" => TokenKind.OrOr,
                "=>" => TokenKind.FatArrow,
                "**" => TokenKind.Power, "<<" => TokenKind.ShiftLeft, ">>" => TokenKind.ShiftRight,
                ".." => TokenKind.DotDot,
                "+=" => TokenKind.PlusEqual, "-=" => TokenKind.MinusEqual,
                "*=" => TokenKind.StarEqual, "/=" => TokenKind.SlashEqual,
                _ => TokenKind.End,
            };
            if (pairKind != TokenKind.End)
            {
                tokens.Add(new Token(pairKind, two, start));
                index += 2;
                continue;
            }
            var singleKind = character switch
            {
                '(' => TokenKind.LeftParen, ')' => TokenKind.RightParen,
                '{' => TokenKind.LeftBrace, '}' => TokenKind.RightBrace,
                '[' => TokenKind.LeftBracket, ']' => TokenKind.RightBracket,
                '<' => TokenKind.Less, '>' => TokenKind.Greater,
                ',' => TokenKind.Comma, ':' => TokenKind.Colon, ';' => TokenKind.Semicolon,
                '.' => TokenKind.Dot,
                '=' => TokenKind.Equal,
                '&' => TokenKind.Ampersand, '+' => TokenKind.Plus, '-' => TokenKind.Minus,
                '*' => TokenKind.Star, '/' => TokenKind.Slash, '%' => TokenKind.Percent,
                '!' => TokenKind.Bang,
                '~' => TokenKind.Tilde, '|' => TokenKind.Pipe, '^' => TokenKind.Caret,
                '?' => TokenKind.Question,
                '$' => TokenKind.Dollar,
                _ => throw new FunctionCompileException(
                    "COFLOW-FUNCTION-SYNTAX", $"unexpected character `{character}`", start),
            };
            tokens.Add(new Token(singleKind, character.ToString(), start));
            index++;
        }
        tokens.Add(new Token(TokenKind.End, "end of function", source.Length));
        return tokens;
    }

    private static int FindInterpolationEnd(string source, int start)
    {
        var depth = 0;
        for (var index = start; index < source.Length; index++)
        {
            if (source[index] == '"')
            {
                index++;
                while (index < source.Length && source[index] != '"')
                {
                    if (source[index] == '\\') index++;
                    index++;
                }
                continue;
            }
            if (source[index] == '#')
            {
                while (index < source.Length && source[index] != '\n') index++;
                continue;
            }
            if (source[index] == '{')
            {
                depth++;
                continue;
            }
            if (source[index] != '}') continue;
            if (depth == 0) return index;
            depth--;
        }
        return -1;
    }
}
