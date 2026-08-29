namespace CoflowRuntime;

internal static partial class CoflowCompiler
{
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
