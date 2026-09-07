namespace Coflow.Runtime.CompilerServices;

internal static partial class CoflowFunctionFrontend
{
    /// <summary>token 游标只负责顺序读取，不持有绑定、类型或控制流状态。</summary>
    internal sealed class FunctionTokenCursor(IReadOnlyList<Token> tokens)
    {
        internal IReadOnlyList<Token> Tokens { get; } = tokens;
        internal int Index { get; private set; }
        internal Token Peek() => Tokens[Index];
        internal Token Advance() => Tokens[Index++];

        internal bool Match(TokenKind kind)
        {
            if (Peek().Kind != kind) return false;
            Index++;
            return true;
        }

        internal Token Expect(TokenKind kind, string message)
        {
            if (Peek().Kind != kind) Error(message);
            return Advance();
        }

        internal void ExpectIdentifier(string value)
        {
            var token = Expect(TokenKind.Identifier, $"expected `{value}`");
            if (token.Text != value) Error($"expected `{value}`");
        }

        internal string ParseTypeName()
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

        private void Error(string message) => throw new FunctionCompileException(
            "COFLOW-FUNCTION-SYNTAX", message, Peek().Offset);
    }

    internal sealed record SyntaxParameter(string Name, string TypeName, int Offset);

    /// <summary>函数语法阶段只保存源码声明与正文 token，不访问 Schema 或 CLR 类型。</summary>
    internal sealed class SyntaxFunction
    {
        internal SyntaxFunction(
            SyntaxParameter[] parameters,
            string returnTypeName,
            Token[] bodyTokens)
        {
            Parameters = CoflowFrozenArray<SyntaxParameter>.CopyOf(parameters);
            ReturnTypeName = returnTypeName;
            BodyTokens = CoflowFrozenArray<Token>.CopyOf(bodyTokens);
        }

        internal CoflowFrozenArray<SyntaxParameter> Parameters { get; }
        internal string ReturnTypeName { get; }
        internal CoflowFrozenArray<Token> BodyTokens { get; }
    }

    internal sealed record BoundParameter(string Name, int Index, Type Type);

    /// <summary>绑定阶段将声明名称关联到 CFT 签名；正文类型分析只消费该不可变结果。</summary>
    internal sealed class BoundFunction
    {
        internal BoundFunction(
            CoflowFunctionEntry entry,
            SyntaxFunction syntax,
            BoundParameter[] parameters)
        {
            Entry = entry;
            Syntax = syntax;
            Parameters = CoflowFrozenArray<BoundParameter>.CopyOf(parameters);
        }

        internal CoflowFunctionEntry Entry { get; }
        internal SyntaxFunction Syntax { get; }
        internal CoflowFrozenArray<BoundParameter> Parameters { get; }
    }

    internal static class FunctionSyntaxParser
    {
        internal static SyntaxFunction Parse(string source)
        {
            var cursor = new FunctionTokenCursor(FunctionLexer.Lex(source));
            cursor.ExpectIdentifier("fn");
            cursor.Expect(TokenKind.LeftParen, "expected `(` after `fn`");
            var parameters = new List<SyntaxParameter>();
            if (!cursor.Match(TokenKind.RightParen))
            {
                do
                {
                    var name = cursor.Expect(TokenKind.Identifier, "expected a parameter name");
                    cursor.Expect(TokenKind.Colon, "expected `:` after the parameter name");
                    parameters.Add(new SyntaxParameter(name.Text, cursor.ParseTypeName(), name.Offset));
                } while (cursor.Match(TokenKind.Comma));
                cursor.Expect(TokenKind.RightParen, "expected `)` after function parameters");
            }
            cursor.Expect(TokenKind.Arrow, "expected `->` after function parameters");
            var returnTypeName = cursor.ParseTypeName();
            cursor.Expect(TokenKind.LeftBrace, "expected a function body");

            var bodyStart = cursor.Index;
            var depth = 0;
            while (true)
            {
                var token = cursor.Advance();
                if (token.Kind == TokenKind.End)
                    throw new FunctionCompileException("COFLOW-FUNCTION-SYNTAX", "unterminated block", token.Offset);
                if (token.Kind == TokenKind.LeftBrace) depth++;
                if (token.Kind != TokenKind.RightBrace) continue;
                if (depth != 0)
                {
                    depth--;
                    continue;
                }
                break;
            }
            cursor.Expect(TokenKind.End, "unexpected content after the function body");
            var body = cursor.Tokens.Skip(bodyStart).Take(cursor.Index - bodyStart - 1).ToList();
            body.Add(cursor.Tokens[cursor.Tokens.Count - 1]);
            return new SyntaxFunction(parameters.ToArray(), returnTypeName, body.ToArray());
        }
    }

    internal static class FunctionBinder
    {
        internal static BoundFunction Bind(
            CoflowFunctionEntry entry,
            SyntaxFunction syntax,
            CoflowCompilerCatalog catalog)
        {
            var ownerFields = catalog.Metadata.TryGetValue(entry.Identity.DeclaredType, out var owner)
                ? owner.Fields.Select(field => field.Name).ToHashSet(StringComparer.Ordinal)
                : new HashSet<string>(StringComparer.Ordinal);
            if (syntax.Parameters.Length != entry.Signature.ParameterTypes.Count)
                Error(entry, null,
                    $"CFD function declares {syntax.Parameters.Length} parameters but CFT requires {entry.Signature.ParameterTypes.Count}");
            var parameters = new BoundParameter[syntax.Parameters.Length];
            var names = new HashSet<string>(StringComparer.Ordinal);
            for (var index = 0; index < parameters.Length; index++)
            {
                var parameter = syntax.Parameters[index];
                if (!CfdIdentifiers.IsIdentifier(parameter.Name))
                    throw new FunctionCompileException("COFLOW-FUNCTION-NAME",
                        $"`{parameter.Name}` is reserved and cannot be used as a binding", parameter.Offset);
                if (ownerFields.Contains(parameter.Name))
                    throw new FunctionCompileException("COFLOW-FUNCTION-NAME",
                        $"binding `{parameter.Name}` conflicts with a field on `{entry.Identity.DeclaredType}`", parameter.Offset);
                if (!names.Add(parameter.Name))
                    throw new FunctionCompileException("COFLOW-FUNCTION-NAME",
                        $"parameter `{parameter.Name}` is declared more than once", parameter.Offset);
                var expected = entry.Signature.ParameterTypes[index];
                var declared = CoflowTypeNameResolver.Resolve(parameter.TypeName, catalog);
                if (declared != expected)
                    throw new FunctionCompileException("COFLOW-FUNCTION-SIGNATURE",
                        $"parameter `{parameter.Name}` has type `{parameter.TypeName}` but CFT requires `{CoflowTypeNameResolver.Format(expected, catalog)}`",
                        parameter.Offset);
                parameters[index] = new BoundParameter(parameter.Name, index, expected);
            }
            var result = CoflowTypeNameResolver.Resolve(syntax.ReturnTypeName, catalog);
            if (result != entry.Signature.ResultType)
                Error(entry, null,
                    $"function returns `{syntax.ReturnTypeName}` but CFT requires `{CoflowTypeNameResolver.Format(entry.Signature.ResultType, catalog)}`");
            return new BoundFunction(entry, syntax, parameters);
        }

        private static void Error(CoflowFunctionEntry entry, int? offset, string message) =>
            throw new FunctionCompileException("COFLOW-FUNCTION-SIGNATURE", message, offset);
    }
}

internal static class CoflowTypeNameResolver
{
    internal static Type Resolve(string name, CoflowCompilerCatalog catalog)
    {
        if (name.StartsWith("&", StringComparison.Ordinal)) name = name[1..];
        if (name == "int") return typeof(long);
        if (name == "float") return typeof(double);
        if (name == "bool") return typeof(bool);
        if (name == "string") return typeof(string);
        if (name == "()") return typeof(Unit);
        if (catalog.Metadata.TryGetValue(name, out var schemaType)) return schemaType.RuntimeType;
        if (catalog.Enums.TryGetValue(name, out var schemaEnum)) return schemaEnum.RuntimeType;
        if (name.StartsWith("[", StringComparison.Ordinal) && name.EndsWith(']'))
            return typeof(IReadOnlyList<>).MakeGenericType(Resolve(name[1..^1], catalog));
        if (name.StartsWith("{", StringComparison.Ordinal) && name.EndsWith('}'))
        {
            var parts = SplitArguments(name[1..^1], ':');
            return typeof(IReadOnlyDictionary<,>).MakeGenericType(
                Resolve(parts[0], catalog), Resolve(parts[1], catalog));
        }
        if (name.StartsWith("Option<", StringComparison.Ordinal))
            return typeof(Option<>).MakeGenericType(Resolve(name[7..^1], catalog));
        if (name.StartsWith("Result<", StringComparison.Ordinal))
        {
            var parts = SplitArguments(name[7..^1], ',');
            return typeof(Result<,>).MakeGenericType(
                Resolve(parts[0], catalog), Resolve(parts[1], catalog));
        }
        if (name.StartsWith("fn(", StringComparison.Ordinal))
        {
            var arrow = FindFunctionParameterEnd(name);
            var parameterText = name[3..arrow];
            var parameters = parameterText.Length == 0
                ? Array.Empty<Type>()
                : SplitArguments(parameterText, ',').Select(value => Resolve(value, catalog)).ToArray();
            return FunctionType(new CoflowFunctionSignature(
                Resolve(name[(arrow + 3)..], catalog), parameters));
        }
        throw new CoflowFunctionFrontend.FunctionCompileException(
            "COFLOW-FUNCTION-TYPE", $"unknown type `{name}`");
    }

    internal static string Format(Type type, CoflowCompilerCatalog catalog)
    {
        if (type == typeof(long) || type == typeof(int)) return "int";
        if (type == typeof(double) || type == typeof(float)) return "float";
        if (type == typeof(bool)) return "bool";
        if (type == typeof(string)) return "string";
        if (type == typeof(Unit)) return "()";
        if (catalog.SchemaTypeNames.TryGetValue(type, out var schemaName)) return schemaName;
        if (CoflowFunctionHandle.IsFunctionType(type))
        {
            var signature = type.GetGenericArguments();
            return $"fn({string.Join(",", signature[..^1].Select(value => Format(value, catalog)))})" +
                $"->{Format(signature[^1], catalog)}";
        }
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments().Select(value => Format(value, catalog)).ToArray();
            if (definition == typeof(Option<>)) return $"Option<{arguments[0]}>";
            if (definition == typeof(Result<,>)) return $"Result<{arguments[0]},{arguments[1]}>";
            if (definition == typeof(IReadOnlyList<>)) return $"[{arguments[0]}]";
            if (definition == typeof(IReadOnlyDictionary<,>)) return $"{{{arguments[0]}:{arguments[1]}}}";
        }
        return type.Name;
    }

    private static Type FunctionType(CoflowFunctionSignature signature)
    {
        var arguments = signature.ParameterTypes.Append(signature.ResultType).ToArray();
        var definition = arguments.Length switch
        {
            1 => typeof(CoflowFunction<>), 2 => typeof(CoflowFunction<,>),
            3 => typeof(CoflowFunction<,,>), 4 => typeof(CoflowFunction<,,,>),
            5 => typeof(CoflowFunction<,,,,>), 6 => typeof(CoflowFunction<,,,,,>),
            7 => typeof(CoflowFunction<,,,,,,>), 8 => typeof(CoflowFunction<,,,,,,,>),
            9 => typeof(CoflowFunction<,,,,,,,,>),
            _ => throw new InvalidOperationException("Coflow functions support at most eight parameters."),
        };
        return definition.MakeGenericType(arguments);
    }

    private static int FindFunctionParameterEnd(string name)
    {
        var depth = 0;
        for (var index = 2; index < name.Length; index++)
        {
            if (name[index] == '(') depth++;
            else if (name[index] == ')' && --depth == 0 &&
                name.AsSpan(index).StartsWith(")->", StringComparison.Ordinal)) return index;
        }
        throw new InvalidOperationException($"invalid function type `{name}`");
    }

    private static string[] SplitArguments(string value, char separator)
    {
        var result = new List<string>();
        var depth = 0;
        var start = 0;
        for (var index = 0; index < value.Length; index++)
        {
            depth += value[index] is '<' or '[' or '{' or '(' ? 1 : 0;
            depth -= value[index] is ']' or '}' or ')' ||
                value[index] == '>' && (index == 0 || value[index - 1] != '-') ? 1 : 0;
            if (value[index] != separator || depth != 0) continue;
            result.Add(value[start..index]);
            start = index + 1;
        }
        result.Add(value[start..]);
        return result.ToArray();
    }
}
