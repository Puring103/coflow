namespace Coflow.Runtime.CompilerServices;

using System.Globalization;
using System.Diagnostics.CodeAnalysis;

internal static partial class CoflowFunctionFrontend
{
    internal sealed class FunctionParser
    {
        private readonly CoflowFunctionEntry _entry;
        private readonly IReadOnlyDictionary<string, ICoflowTypeMetadata> _metadata;
        private readonly IReadOnlyDictionary<string, ICoflowEnumMetadata> _enums;
        private readonly IReadOnlyDictionary<Type, string> _schemaTypeNames;
        private readonly IReadOnlyDictionary<Type, ICoflowTypeMetadata> _metadataByRuntimeType;
        private readonly IReadOnlyDictionary<Type, ICoflowEnumMetadata> _enumsByRuntimeType;
        private readonly FunctionTokenCursor _tokens;
        private readonly FunctionTypeChecker _typeChecker;
        private readonly CoflowCompilerCatalog _catalog;
        private readonly CoflowRecordCatalog _records;
        private readonly CfdLoadContext _context;
        private readonly HashSet<string> _ownerFieldNames;
        private readonly ParseState _parse;

        internal FunctionParser(
            BoundFunction function,
            CoflowCompilerCatalog catalog,
            CoflowRecordCatalog records,
            CfdLoadContext context)
        {
            var entry = function.Entry;
            _entry = entry;
            _metadata = catalog.Metadata;
            _enums = catalog.Enums;
            _schemaTypeNames = catalog.SchemaTypeNames;
            _metadataByRuntimeType = catalog.MetadataByRuntimeType;
            _enumsByRuntimeType = catalog.EnumsByRuntimeType;
            _tokens = new FunctionTokenCursor(function.Syntax.BodyTokens);
            _typeChecker = new FunctionTypeChecker(catalog, () => _tokens.Peek().Offset);
            _catalog = catalog;
            _records = records;
            _context = context;
            _ownerFieldNames = catalog.Metadata.TryGetValue(entry.Identity.DeclaredType, out var owner)
                ? owner.Fields.Select(field => field.Name).ToHashSet(StringComparer.Ordinal)
                : new HashSet<string>(StringComparer.Ordinal);
            _parse = new ParseState
            {
                ReturnType = entry.Signature.ResultType,
            };
            foreach (var parameter in function.Parameters)
                _parse.Parameters.Add(parameter.Name, (parameter.Index, parameter.Type));
        }

        /// <summary>单次解析和类型检查的全部可变状态；阶段结束后不再由 lowering 读取。</summary>
        private sealed class ParseState
        {
            internal Dictionary<string, (int Index, Type Type)> Parameters { get; } =
                new(StringComparer.Ordinal);
            internal List<Dictionary<string, (int Index, Type Type)>> LocalScopes { get; } = new();
            internal Stack<LambdaParseContext> LambdaContexts { get; } = new();
            internal Stack<IReadOnlyDictionary<string, Type>> Narrowings { get; } = new();
            internal HashSet<int> MutableLocals { get; } = new();
            internal List<CoflowBindingDependency> BindingDependencies { get; } = new();
            internal Type ReturnType { get; set; } = typeof(Unit);
            internal int LoopDepth { get; set; }
            internal int LocalCount { get; set; }
        }

        /// <summary>解析并检查已完成声明绑定的函数正文；后续阶段继续拆开正文 syntax 与 typing。</summary>
        internal TypedFunction ParseAndTypeBody()
        {
            var expectedResult = FormatType(_entry.Signature.ResultType);
            var expression = ParseBlockContents().WithExpected(_entry.Signature.ResultType, _typeChecker);
            Expect(TokenKind.End, "unexpected content after the function body");
            if (expression.Type != _entry.Signature.ResultType && !expression.AlwaysTerminates)
                Error("COFLOW-FUNCTION-RETURN",
                    $"body has type `{FormatType(expression.Type)}` but function returns `{expectedResult}`");
            return new TypedFunction(expression, _parse.BindingDependencies.ToArray());
        }

        private Expr ParseBlockContents(Dictionary<string, (int Index, Type Type)>? initialScope = null)
        {
            _parse.LocalScopes.Add(initialScope ?? new Dictionary<string, (int Index, Type Type)>(StringComparer.Ordinal));
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
                        var value = ParseExpression().WithExpected(_parse.ReturnType, _typeChecker);
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
                        if (_parse.LoopDepth == 0)
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
                _parse.LocalScopes.RemoveAt(_parse.LocalScopes.Count - 1);
            }
            return new BlockExpr(statements, result ?? new ConstantExpr(Unit.Value, typeof(Unit)));
        }

        private Expr ParseWhile()
        {
            var condition = ParseExpression();
            Expect(TokenKind.LeftBrace, "expected `{` after while condition");
            _parse.LoopDepth++;
            Expr body;
            try { body = ParseBlockContents(); }
            finally { _parse.LoopDepth--; }
            return _typeChecker.While(condition, body);
        }

        private Expr ParseFor()
        {
            var firstName = ExpectBindingIdentifier("expected a loop binding after `for`").Text;
            string? secondName = null;
            if (Match(TokenKind.Comma))
                secondName = ExpectBindingIdentifier("expected a second loop binding").Text;
            ExpectIdentifier("in");
            var collection = ParseExpression();
            var range = collection as RangeExpr;
            var typedCollection = _typeChecker.ForCollectionType(collection, secondName is not null);
            if (secondName == firstName)
                Error("COFLOW-FUNCTION-NAME", $"loop binding `{firstName}` is declared more than once");

            var collectionLocal = typedCollection.IsRange ? -1 : _parse.LocalCount++;
            var indexLocal = typedCollection.IsRange && secondName is null ? -1 : _parse.LocalCount++;
            var firstLocal = _parse.LocalCount++;
            var rangeEndLocal = typedCollection.IsRange ? _parse.LocalCount++ : -1;
            int? secondLocal = secondName is null ? null : typedCollection.IsRange ? indexLocal : _parse.LocalCount++;
            var scope = new Dictionary<string, (int Index, Type Type)>(StringComparer.Ordinal);
            scope.Add(firstName, (firstLocal, typedCollection.FirstType));
            if (secondName is not null)
                scope.Add(secondName, (secondLocal!.Value, typedCollection.SecondType!));
            Expect(TokenKind.LeftBrace, "expected `{` after for collection");
            _parse.LoopDepth++;
            Expr body;
            try { body = ParseBlockContents(scope); }
            finally { _parse.LoopDepth--; }
            _typeChecker.RequireLoopBody(body);
            if (range is not null)
                return new RangeForExpr(
                    range.Start,
                    range.End,
                    range.Inclusive,
                    firstLocal,
                    rangeEndLocal,
                    secondLocal,
                    body);
            return new ForExpr(collection, typedCollection.IsArray, collectionLocal, indexLocal,
                firstLocal, secondLocal, typedCollection.FirstType, typedCollection.SecondType, body);
        }

        private Expr ParseVariable()
        {
            var name = ExpectBindingIdentifier("expected a local variable name").Text;
            string? declaredType = null;
            if (Match(TokenKind.Colon)) declaredType = ParseTypeName();
            Expect(TokenKind.Equal, "expected `=` in local variable declaration");
            var value = ParseExpression();
            if (declaredType is not null)
                value = value.WithExpected(ResolveTypeName(declaredType), _typeChecker);
            var scope = _parse.LocalScopes[^1];
            if (scope.ContainsKey(name) ||
                (_parse.LambdaContexts.Count == 0 ? _parse.Parameters.ContainsKey(name) : _parse.LambdaContexts.Peek().Parameters.ContainsKey(name)))
                Error("COFLOW-FUNCTION-NAME", $"name `{name}` is already declared in this scope");
            var local = (_parse.LocalCount++, value.Type);
            scope.Add(name, local);
            _parse.MutableLocals.Add(local.Item1);
            return new StoreLocalExpr(local.Item1, value);
        }

        private Expr ParseExpression()
        {
            var offset = Peek().Offset;
            return ParseExpressionCore().At(offset);
        }

        private Expr ParseExpressionCore()
        {
            var left = ParseBinary(0);
            if (Peek().Kind is TokenKind.DotDot or TokenKind.DotDotEqual)
            {
                var inclusive = Advance().Kind == TokenKind.DotDotEqual;
                var end = ParseBinary(0);
                left = new RangeExpr(
                    left.WithExpected(typeof(long), _typeChecker),
                    end.WithExpected(typeof(long), _typeChecker),
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
            if (!_parse.MutableLocals.Contains(local.Index))
                Error("COFLOW-FUNCTION-ASSIGN", "only a local `var` can be assigned");
            var right = ParseExpression();
            if (assignment == TokenKind.Equal)
                return new AssignLocalExpr(local.Index, right.WithExpected(local.Type, _typeChecker));
            var operation = assignment switch
            {
                TokenKind.PlusEqual => "+", TokenKind.MinusEqual => "-",
                TokenKind.StarEqual => "*", TokenKind.SlashEqual => "/",
                _ => throw new InvalidOperationException(),
            };
            return new AssignLocalExpr(local.Index,
                BinaryExpr.Create(operation, local, right, _typeChecker).WithExpected(local.Type, _typeChecker));
        }

        private Expr ParseBinary(int minimumPrecedence)
        {
            var left = ParseUnary();
            while (true)
            {
                if (Peek().Kind == TokenKind.Identifier && Peek().Text == "is" && minimumPrecedence <= 7)
                {
                    Advance();
                    var target = ResolveTypeName(ParseTypeName());
                    if (!_schemaTypeNames.ContainsKey(target))
                        Error("COFLOW-FUNCTION-TYPE", "`is` target must be a schema object type");
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
                    left = chain.Append(operation, right, _typeChecker);
                else if (IsComparison(operation) && left is BinaryExpr previous && IsComparison(previous.Operation))
                    left = ComparisonChainExpr.Create(previous.Left, previous.Right,
                        previous.Operation, operation, right, _typeChecker);
                else
                    left = CreateBinary(operation, left, right);
            }
            return left;
        }

        private static bool IsComparison(string operation) => operation is "<" or "<=" or ">" or ">=";

        private Expr CreateBinary(string operation, Expr left, Expr right) =>
            left.Type.IsEnum || right.Type.IsEnum
                ? EnumBinaryExpr.Create(operation, left, right, _typeChecker, _typeChecker.EnumMetadata(left.Type))
                : BinaryExpr.Create(operation, left, right, _typeChecker);

        private Expr ParseUnary()
        {
            if (Match(TokenKind.Minus)) return UnaryExpr.Create("-", ParseUnary(), _typeChecker);
            if (Match(TokenKind.Bang)) return UnaryExpr.Create("!", ParseUnary(), _typeChecker);
            if (Match(TokenKind.Tilde)) return UnaryExpr.Create("~", ParseUnary(), _typeChecker);
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
                    expression = IndexExpr.Create(expression, index, _typeChecker);
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
                    _metadataByRuntimeType.TryGetValue(expression.Type, out var metadata);
                    if (metadata?.Find(field) is not null)
                        expression = Field(expression, field);
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

        private Expr ParseBuiltin(Expr receiver, string name)
        {
            var arguments = new List<Expr>();
            if (!Match(TokenKind.RightParen))
            {
                do arguments.Add(ParseExpression()); while (Match(TokenKind.Comma));
                Expect(TokenKind.RightParen, "expected `)` after built-in arguments");
            }
            return _typeChecker.Builtin(receiver, name, arguments);
        }

        private Expr ParsePropagation(Expr operand)
            => _typeChecker.Propagate(operand, _parse.ReturnType);

        private Expr ParseCall(Expr target)
        {
            var arguments = new List<Expr>();
            if (!Match(TokenKind.RightParen))
            {
                do arguments.Add(ParseExpression()); while (Match(TokenKind.Comma));
                Expect(TokenKind.RightParen, "expected `)` after function arguments");
            }
            return _typeChecker.Call(target, arguments);
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
                        var enumInteger = ParseExpression().WithExpected(typeof(long), _typeChecker);
                        Expect(TokenKind.RightParen, "expected `)` after enum integer value");
                        return new ConversionExpr(enumInteger, enumConstructor.RuntimeType);
                    }
                    if (_parse.LambdaContexts.TryPeek(out var lambda) &&
                        lambda.Parameters.TryGetValue(token.Text, out var lambdaParameter))
                        return new ArgumentExpr(lambdaParameter.Index, lambdaParameter.Type);
                    for (var scope = _parse.LocalScopes.Count - 1; scope >= 0; scope--)
                    {
                        if (_parse.LocalScopes[scope].TryGetValue(token.Text, out var local))
                        {
                            if (lambda is null || scope >= lambda.ScopeBase)
                                return new LocalExpr(local.Index, NarrowedType(token.Text) ?? local.Type, token.Text);
                            return lambda.Capture($"L:{local.Index}", new LocalExpr(local.Index, NarrowedType(token.Text) ?? local.Type, token.Text));
                        }
                    }
                    if (_parse.Parameters.TryGetValue(token.Text, out var parameter))
                    {
                        var argument = new ArgumentExpr(parameter.Index, NarrowedType(token.Text) ?? parameter.Type, token.Text);
                        return lambda is null ? argument : lambda.Capture($"A:{parameter.Index}", argument);
                    }
                    if (OwnerMember(token.Text, lambda) is { } ownerMember)
                        return ownerMember;
                    if (DeclaredConstant(token.Text) is { } constant)
                        return constant;
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
                if (!_typeChecker.IsInterpolatable(value.Type))
                    Error("COFLOW-FUNCTION-INTERPOLATION",
                        $"values of type `{FormatType(value.Type)}` cannot be interpolated");
                parts.Add(new InterpolationPart(null, value));
            }
            return new InterpolatedStringExpr(parts);
        }

        private bool StartsObjectConstructor(string first)
        {
            return _tokens.Index < _tokens.Tokens.Count &&
                   _tokens.Peek().Kind == TokenKind.LeftBrace &&
                   _metadata.ContainsKey(first);
        }

        private Expr ParseObjectConstructor(Token first)
        {
            var declaredName = first.Text;
            if (!_metadata.TryGetValue(declaredName, out var metadata))
                Error("COFLOW-FUNCTION-OBJECT", $"unknown object type `{declaredName}`");
            if (metadata.IsAbstract)
                Error("COFLOW-FUNCTION-OBJECT", $"abstract type `{declaredName}` cannot be constructed");
            if (metadata is ICoflowHostMetadata || metadata.IsSingleton)
                Error("COFLOW-FUNCTION-OBJECT", $"singleton type `{declaredName}` cannot be constructed as a value");
            Expect(TokenKind.LeftBrace, "expected `{` after an object type");
            var fields = new List<(string Name, Expr Value)>();
            var seen = new HashSet<string>(StringComparer.Ordinal);
            while (!Match(TokenKind.RightBrace))
            {
                var field = Expect(TokenKind.Identifier, "expected an object field name");
                if (!seen.Add(field.Text))
                    Error("COFLOW-FUNCTION-OBJECT", $"object field `{field.Text}` is specified more than once");
                if (metadata.Find(field.Text) is null)
                    Error("COFLOW-FUNCTION-OBJECT", $"type `{declaredName}` has no field `{field.Text}`");
                var fieldType = metadata.Require(field.Text).Binding.RuntimeType;
                if (metadata.Require(field.Text).Binding.IsFunction)
                    Error("COFLOW-FUNCTION-OBJECT", $"function field `{field.Text}` cannot be supplied by an object constructor");
                Expect(TokenKind.Colon, "expected `:` after an object field name");
                fields.Add((field.Text, ParseExpression().WithExpected(fieldType, _typeChecker)));
                if (!Match(TokenKind.Comma))
                {
                    Expect(TokenKind.RightBrace, "expected `,` or `}` after an object field");
                    break;
                }
                if (Match(TokenKind.RightBrace)) break;
            }
            foreach (var field in metadata.Fields)
            {
                if (!field.Binding.IsFunction &&
                    !seen.Contains(field.Name) &&
                    !field.HasDefault)
                    Error("COFLOW-FUNCTION-OBJECT", $"object `{declaredName}` is missing required field `{field.Name}`");
            }
            return new ObjectExpr(metadata, _context, fields);
        }

        private Expr ParseContextMetadata()
        {
            var name = Expect(TokenKind.Identifier, "expected metadata name after `$`").Text;
            var value = name switch
            {
                "id" => _entry.Identity.RecordKey,
                "path" => $"{_entry.Identity.DeclaredType}::{_entry.Identity.RecordKey}",
                "type" => _entry.Identity.DeclaredType,
                "field" => _entry.Identity.FieldName,
                "function" => _entry.Identity.FieldName,
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
                    $"`${name}` requires a schema record receiver");
            return new TransformExpr(receiver, typeof(string),
                CoflowFormatting.RecordMetadata(receiver.Type, name, _metadata, _enums));
        }

        private Expr ParseStaticValue(Token first)
        {
            var segments = new List<string> { first.Text };
            while (Match(TokenKind.DoubleColon))
                segments.Add(Expect(TokenKind.Identifier, "expected a name after `::`").Text);
            if (segments.Count != 2)
                Error("COFLOW-FUNCTION-NAME", $"invalid static path `{first.Text}`");
            return StaticValue(segments);
        }


        private Type? NarrowedType(string name)
        {
            foreach (var narrowing in _parse.Narrowings)
                if (narrowing.TryGetValue(name, out var type)) return type;
            return null;
        }

        private Expr ParseNumericConversion(string target)
        {
            Expect(TokenKind.LeftParen, $"expected `(` after `{target}`");
            var value = ParseExpression();
            Expect(TokenKind.RightParen, "expected `)` after numeric conversion");
            return _typeChecker.NumericConversion(target, value);
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
            var context = new LambdaParseContext(_parse.LocalScopes.Count, parameters, parameterTypes.Count);
            _parse.LambdaContexts.Push(context);
            var previousReturn = _parse.ReturnType;
            _parse.ReturnType = resultType;
            Expr body;
            try { body = ParseBlockContents().WithExpected(resultType, _typeChecker); }
            finally
            {
                _parse.ReturnType = previousReturn;
                _parse.LambdaContexts.Pop();
            }
            return new LambdaExpr(
                new CoflowFunctionSignature(resultType, parameterTypes),
                context.Captures,
                body);
        }

        private Expr ParseMatchExpression()
        {
            var subject = ParseExpression();
            Expect(TokenKind.LeftBrace, "expected `{` after match value");
            var subjectLocal = _parse.LocalCount++;
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
                    bindingLocal = _parse.LocalCount++;
                    scope.Add(binding, (bindingLocal.Value, pattern.BindingType!));
                }
                Expr body;
                if (Match(TokenKind.LeftBrace))
                {
                    body = ParseBlockContents(scope);
                }
                else
                {
                    _parse.LocalScopes.Add(scope);
                    try { body = ParseExpression(); }
                    finally { _parse.LocalScopes.RemoveAt(_parse.LocalScopes.Count - 1); }
                }
                arms.Add(new MatchArm(pattern, bindingLocal, body));
                if (!Match(TokenKind.Comma))
                {
                    Expect(TokenKind.RightBrace, "expected `,` or `}` after match arm");
                    break;
                }
                else if (Match(TokenKind.RightBrace))
                    break;
            }
            if (arms.Count == 0) Error("COFLOW-FUNCTION-MATCH", "match requires at least one arm");
            var exhaustive = hasCatchAll || IsExhaustiveMatch(subject.Type, patternKinds);
            if (!exhaustive) Error("COFLOW-FUNCTION-MATCH", "match is not exhaustive");
            return _typeChecker.Match(subject, subjectLocal, arms, !hasCatchAll);
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
                return ValueFactories.MatchBranch(subjectType, token.Text, binding, _typeChecker);
            }
            if (token.Kind == TokenKind.Identifier && token.Text == "None")
                return ValueFactories.MatchNone(subjectType, _typeChecker);
            if (token.Kind == TokenKind.Identifier)
            {
                var segments = new List<string> { token.Text };
                while (Match(TokenKind.DoubleColon))
                    segments.Add(Expect(TokenKind.Identifier, "expected a name after `::`").Text);
                if (segments.Count > 2)
                    Error("COFLOW-FUNCTION-MATCH", "enum pattern must use `Enum::Variant`");
                if (segments.Count == 2)
                {
                    var enumName = segments[0];
                    var variant = segments[1];
                    if (_enums.TryGetValue(enumName, out var enumMetadata))
                    {
                        if (!enumMetadata.Variants.TryGetValue(variant, out var enumValue))
                            Error("COFLOW-FUNCTION-MATCH", $"enum `{enumName}` has no variant `{variant}`");
                        if (enumMetadata.RuntimeType != subjectType)
                            Error("COFLOW-FUNCTION-TYPE", "match enum literal type does not match subject type");
                        return MatchPattern.Literal($"{enumName}::{variant}", enumValue);
                    }
                }

                var typeName = segments[0];
                _metadata.TryGetValue(typeName, out var schemaType);
                if (schemaType is not null && Peek().Kind == TokenKind.Identifier)
                {
                    var binding = ExpectBindingIdentifier("expected a type pattern binding").Text;
                    if (!subjectType.IsAssignableFrom(schemaType.RuntimeType))
                        Error("COFLOW-FUNCTION-TYPE", $"type pattern `{typeName}` is not assignable to match subject");
                    return new MatchPattern($"type:{typeName}", false, binding, schemaType.RuntimeType,
                        TypeTarget: schemaType.RuntimeType);
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
            if (subjectType == typeof(bool))
                return kinds.Contains("literal:System.Boolean:True") &&
                    kinds.Contains("literal:System.Boolean:False");
            _enumsByRuntimeType.TryGetValue(subjectType, out var subjectEnum);
            if (subjectEnum is not null)
                return subjectEnum.Variants.Keys.All(variant =>
                    kinds.Contains($"{subjectEnum.DeclaredType}::{variant}"));
            if (!subjectType.IsGenericType)
            {
                _metadataByRuntimeType.TryGetValue(subjectType, out var subject);
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
            if (segments.Count > 2)
                Error("COFLOW-FUNCTION-REFERENCE", "record reference must use `&Type::key.field`");
            var fullName = segments[0];
            if (_metadata.TryGetValue(fullName, out var singletonMetadata) && singletonMetadata.IsSingleton)
            {
                declaredType = fullName;
                key = string.Empty;
            }
            else if (segments.Count > 1)
            {
                declaredType = segments[0];
                key = segments[1];
            }
            Expect(TokenKind.Dot, "record references in functions must select a field");
            var fieldName = Expect(TokenKind.Identifier, "expected a field name after record reference").Text;
            return RecordFieldReference(
                declaredType, key, fieldName, _parse.BindingDependencies);
        }

        private Expr ParseArrayLiteral()
        {
            var values = new List<Expr>();
            if (!Match(TokenKind.RightBracket))
            {
                do values.Add(ParseExpression()); while (Match(TokenKind.Comma));
                Expect(TokenKind.RightBracket, "expected `]` after array literal");
            }
            return _typeChecker.ArrayLiteral(values);
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
            return _typeChecker.DictionaryLiteral(entries);
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
            Expect(TokenKind.LeftBrace, "expected `{` after if condition");
            if (condition is TypeIsExpr { NarrowName: { } name } typeIs)
                _parse.Narrowings.Push(new Dictionary<string, Type>(StringComparer.Ordinal) { [name] = typeIs.TargetType });
            var whenTrue = ParseBlockContents();
            if (condition is TypeIsExpr { NarrowName: not null }) _parse.Narrowings.Pop();
            Expr? whenFalse = null;
            if (Peek().Kind == TokenKind.Identifier && Peek().Text == "else")
            {
                Advance();
                Expect(TokenKind.LeftBrace, "expected `{` after `else`");
                whenFalse = ParseBlockContents();
            }
            return _typeChecker.IfExpression(condition, whenTrue, whenFalse);
        }

        private string ParseTypeName()
            => _tokens.ParseTypeName();

        private Type ResolveTypeName(string name)
        {
            try { return CoflowTypeNameResolver.Resolve(name, _catalog); }
            catch (FunctionCompileException error) when (error.Offset is null)
            {
                Error(error.Code, error.Message);
            }
            return null!;
        }

        private string FormatType(Type type) => CoflowTypeNameResolver.Format(type, _catalog);

        private Expr Field(Expr receiver, string fieldName)
        {
            _catalog.MetadataByRuntimeType.TryGetValue(receiver.Type, out var metadata);
            if (metadata?.Find(fieldName) is null)
                Error("COFLOW-FUNCTION-FIELD", $"type `{FormatType(receiver.Type)}` has no field `{fieldName}`");
            var binding = metadata!.Require(fieldName).Binding;
            return new FieldExpr(receiver, binding.RuntimeType, CoflowFieldAccess.Bind(metadata, binding));
        }


        private Expr? OwnerMember(string name, LambdaParseContext? lambda)
        {
            if (!_catalog.Metadata.TryGetValue(_entry.Identity.DeclaredType, out var metadata) ||
                metadata.Find(name) is null) return null;
            Expr receiver = new ArgumentExpr(_entry.Signature.ParameterTypes.Count, metadata.RuntimeType);
            if (lambda is not null) receiver = lambda.Capture("R", receiver);
            var binding = metadata.Require(name).Binding;
            return binding.IsFunction
                ? new FunctionReferenceExpr(_context.ResolveFunction(
                    _entry.Identity.DeclaredType, _entry.Identity.RecordKey, name), receiver)
                : new FieldExpr(receiver, binding.RuntimeType, CoflowFieldAccess.Bind(metadata, binding));
        }

        private ConstantExpr? DeclaredConstant(string name) =>
            _catalog.Constants.TryGetValue(name, out var constant) ? ConstantReference(constant) : null;

        private Expr StaticValue(IReadOnlyList<string> segments)
        {
            var path = string.Join("::", segments);
            if (_catalog.Constants.TryGetValue(path, out var constant)) return ConstantReference(constant);
            var owner = segments[0];
            var member = segments[^1];
            if (!_catalog.Enums.TryGetValue(owner, out var metadata))
                Error("COFLOW-FUNCTION-NAME", $"unknown static owner `{owner}`");
            if (!metadata.Variants.TryGetValue(member, out var value))
                Error("COFLOW-FUNCTION-NAME", $"enum `{owner}` has no variant `{member}`");
            return new ConstantExpr(value, metadata.RuntimeType);
        }

        private bool TryResolveEnum(string name, out ICoflowEnumMetadata metadata) =>
            _catalog.Enums.TryGetValue(name, out metadata!);

        private Expr RecordFieldReference(string? declaredType, string key, string fieldName,
            ICollection<CoflowBindingDependency> dependencies)
        {
            CoflowRecord? match = null;
            foreach (var candidate in _records.WithKey(key))
            {
                if (declaredType is not null && candidate.DeclaredType != declaredType) continue;
                if (_catalog.Metadata[candidate.DeclaredType].Find(fieldName) is null) continue;
                if (match is not null)
                    Error("COFLOW-FUNCTION-REFERENCE", $"record reference `{key}.{fieldName}` is ambiguous");
                match = candidate;
            }
            if (match is null && declaredType is not null &&
                _catalog.Metadata[declaredType] is ICoflowHostMetadata host &&
                host.Require(fieldName).Binding.IsFunction)
                return new FunctionReferenceExpr(_context.ResolveFunction(declaredType, key, fieldName), null);
            if (match is null)
                Error("COFLOW-FUNCTION-REFERENCE", $"record `{key}` with field `{fieldName}` was not found");
            var selected = match!.Value;
            dependencies.Add(new CoflowBindingDependency(declaredType, key, fieldName, selected.DeclaredType));
            var metadata = _catalog.Metadata[selected.DeclaredType];
            var reference = new CoflowRecordReferenceTemplate(selected.DeclaredType, key);
            var binding = metadata.Require(fieldName).Binding;
            if (binding.IsFunction)
            {
                if (metadata is ICoflowHostMetadata)
                    return new FunctionReferenceExpr(_context.ResolveFunction(
                        selected.Value, selected.DeclaredType, fieldName), null);
                return new FunctionReferenceExpr(_context.ResolveFunction(
                    selected.Value, selected.DeclaredType, fieldName),
                    new ConstantExpr(reference, metadata.RuntimeType));
            }
            return new FieldExpr(new ConstantExpr(reference, metadata.RuntimeType),
                binding.RuntimeType, CoflowFieldAccess.Bind(metadata, binding));
        }

        private void ValidateBindingIdentifier(Token token)
        {
            if (!CfdIdentifiers.IsIdentifier(token.Text))
                Error("COFLOW-FUNCTION-NAME", $"`{token.Text}` is reserved and cannot be used as a binding");
            if (_ownerFieldNames.Contains(token.Text))
                Error("COFLOW-FUNCTION-NAME",
                    $"binding `{token.Text}` conflicts with a field on `{_entry.Identity.DeclaredType}`");
        }

        private ConstantExpr ConstantReference(CoflowConstant constant) =>
            new(_context.ResolveConstant(constant), constant.RuntimeType)
            { TemplateValue = new CoflowConstantReferenceTemplate(constant) };


        private Token Peek() => _tokens.Peek();
        private Token Advance() => _tokens.Advance();
        private bool Match(TokenKind kind) => _tokens.Match(kind);
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
        private void ExpectIdentifier(string value)
        {
            var token = Expect(TokenKind.Identifier, $"expected `{value}`");
            if (token.Text != value) Error("COFLOW-FUNCTION-SYNTAX", $"expected `{value}`");
        }
        [DoesNotReturn]
        internal void Error(string code, string message)
        {
            throw new FunctionCompileException(code, message, CurrentOffset());
        }

        private int CurrentOffset() => _tokens.Index < _tokens.Tokens.Count
            ? _tokens.Peek().Offset
            : _tokens.Tokens[_tokens.Tokens.Count - 1].Offset;

        [DoesNotReturn]
        private static void ErrorAt(int offset, string code, string message) =>
            throw new FunctionCompileException(code, message, offset);


    }


}

