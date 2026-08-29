using System.Globalization;
using System.ComponentModel;
using System.Text;

namespace CoflowRuntime.Generated;

/// <summary>Schema-free parser shared by generated target models.</summary>
internal static class CfdParser
{
    public static CfdDocument Parse(CfdSource source)
    {
        var parser = new Parser(source);
        return parser.ParseDocument();
    }

    public static IReadOnlyList<CfdDocument> ParseAll(IEnumerable<CfdSource> sources)
    {
        var documents = new List<CfdDocument>();
        foreach (var source in sources)
            documents.Add(Parse(source));
        return documents;
    }

    private sealed class Parser
    {
        private readonly CfdSource _source;
        private readonly List<CfdDiagnostic> _errors = new();
        private readonly HashSet<(string Type, string Key)> _recordKeys = new();
        private int _index;
        private int _line = 1;
        private int _column = 1;

        public Parser(CfdSource source)
        {
            _source = source;
        }

        public CfdDocument ParseDocument()
        {
            string? declaredNamespace = null;
            var uses = new List<CfdUseDirective>();
            var records = new List<CfdRecordNode>();
            SkipTrivia();
            if (MatchKeyword("namespace"))
            {
                declaredNamespace = ParseQualifiedPath("namespace path", requireQualified: false);
                Expect(';', "expected `;` after namespace declaration");
                SkipTrivia();
            }
            while (MatchKeyword("use"))
            {
                var start = Cursor;
                var path = ParseQualifiedPath("use path", requireQualified: true);
                SkipTrivia();
                var localName = path[(path.LastIndexOf("::", StringComparison.Ordinal) + 2)..];
                if (MatchKeyword("as"))
                {
                    localName = ParseName("use alias");
                    if (!IsIdentifier(localName))
                        Error("CFD-SYNTAX-USE", $"invalid use alias `{localName}`", SpanFrom(start));
                }
                Expect(';', "expected `;` after use declaration");
                uses.Add(new CfdUseDirective(path, localName, SpanFrom(start)));
                SkipTrivia();
            }
            while (!End)
            {
                SkipTrivia();
                if (End) break;
                if (MatchKeyword("namespace") || MatchKeyword("use"))
                {
                    Error("CFD-SYNTAX-HEADER", "namespace and use declarations must appear before all CFD records", CurrentSpan());
                    RecoverTopLevel();
                    continue;
                }
                var start = Cursor;
                var firstWasQuoted = Peek() == '"';
                var first = ParseKey("record key or type");
                SkipTrivia();
                if (Match(':'))
                {
                    var type = ParseName("record type");
                    AddRecord(records, ParseRecord(first, type, start));
                }
                else if (Match('{'))
                {
                    if (firstWasQuoted)
                        Error("CFD-SYNTAX-007", "group type must be an unquoted name", SpanFrom(start));
                    ParseGroupedRecords(records, first, start);
                }
                else
                {
                    Error("CFD-SYNTAX-001", "expected `:` or `{` after record declaration", CurrentSpan());
                    RecoverTopLevel();
                }
            }
            ThrowIfErrors();
            return new CfdDocument(_source.Path, declaredNamespace, uses, records);
        }

        private string ParseQualifiedPath(string expected, bool requireQualified)
        {
            var start = Cursor;
            var path = ParseName(expected);
            var segments = path.Split(new[] { "::" }, StringSplitOptions.None);
            if ((requireQualified && segments.Length < 2) || segments.Any(segment => !IsIdentifier(segment)))
                Error("CFD-SYNTAX-HEADER", $"invalid {expected} `{path}`", SpanFrom(start));
            return path;
        }

        private void ParseGroupedRecords(List<CfdRecordNode> records, string groupType, Position start)
        {
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) return;
                var keyStart = Cursor;
                var key = ParseKey("record key");
                SkipTrivia();
                string type = groupType;
                if (Match(':')) type = ParseName("record type");
                AddRecord(records, ParseRecord(key, type, keyStart, groupType));
                SkipTrivia();
                Match(',');
            }
            Error("CFD-SYNTAX-002", "unterminated grouped record", SpanFrom(start));
        }

        private void AddRecord(List<CfdRecordNode> records, CfdRecordNode record)
        {
            if (!IsRecordKey(record.Key))
                Error("CFD-SYNTAX-RECORD-KEY", $"invalid record key `{record.Key}`", record.Span);
            if (!_recordKeys.Add((record.DeclaredType, record.Key)))
                Error("CFD-SYNTAX-DUPLICATE-RECORD", $"record `{record.DeclaredType}.{record.Key}` is declared more than once", record.Span);
            records.Add(record);
        }

        private static bool IsRecordKey(string value)
        {
            if (string.IsNullOrEmpty(value) || IsReservedIdentifier(value))
                return false;
            var index = 0;
            if (!ReadIdentifierCodePoint(value, ref index, true)) return false;
            while (index < value.Length)
                if (!ReadIdentifierCodePoint(value, ref index, false)) return false;
            return true;
        }

        private static bool IsReservedIdentifier(string value) => CfdIdentifiers.IsReserved(value);

        private static bool ReadIdentifierCodePoint(string value, ref int index, bool start) =>
            CfdIdentifiers.ReadCodePoint(value, ref index, start);

        private CfdRecordNode ParseRecord(
            string key,
            string type,
            Position start,
            string? groupType = null)
        {
            SkipTrivia();
            Expect('{', "expected `{` after record declaration");
            var fields = ParseFields();
            return new CfdRecordNode(key, type, fields, SpanFrom(start), groupType);
        }

        private List<CfdFieldNode> ParseFields()
        {
            var fields = new List<CfdFieldNode>();
            var fieldNames = new HashSet<string>(StringComparer.Ordinal);
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) return fields;
                var start = Cursor;
                var name = ParseKey("field name");
                Expect(':', "expected `:` after field name");
                var value = ParseValue();
                if (!fieldNames.Add(name))
                    Error("CFD-SYNTAX-DUPLICATE-FIELD", $"field `{name}` is declared more than once", SpanFrom(start));
                var field = new CfdFieldNode(name, value, SpanFrom(start));
                fields.Add(field);
                SkipTrivia();
                if (Match(','))
                    continue;
                if (!End && Peek() != '}')
                    Error("CFD-SYNTAX-009", "expected `,` or `}` after field", CurrentSpan());
            }
            Error("CFD-SYNTAX-003", "unterminated record", CurrentSpan());
            return fields;
        }

        private CfdValueNode ParseValue() => ParseValueInner();

        private CfdValueNode ParseValueInner()
        {
            SkipTrivia();
            var start = Cursor;
            var sourceStart = _index;
            if (End) { Error("CFD-SYNTAX-004", "expected a value", CurrentSpan()); return new CfdInvalidValue(CurrentSpan()); }
            if (Match("f\"")) return ParseFormattedString(start, true);
            if (Match('"'))
            {
                var value = ParseString();
                var span = SpanFrom(start);
                return ParseAutomaticFormat(value, span) ?? new CfdStringValue(value, span);
            }
            if (Match('&'))
            {
                var reference = ParseName("reference key");
                var separator = reference.LastIndexOf("::", StringComparison.Ordinal);
                var typeName = separator < 0 ? null : reference[..separator];
                var key = separator < 0 ? reference : reference[(separator + 2)..];
                if (!IsRecordKey(key) || (typeName is not null && !IsQualifiedName(typeName)))
                    Error("CFD-SYNTAX-RECORD-KEY", $"invalid record key `{key}`", SpanFrom(start));
                return new CfdReferenceValue(typeName, key, SpanFrom(start));
            }
            if (Match('[')) return ParseArray(start);
            if (Match('{')) return ParseDictionary(start);
            if (MatchKeyword("None")) return new CfdNoneValue(SpanFrom(start));
            if (MatchKeyword("Some")) return ParseValueConstructor(start, "Some", static (value, span) => new CfdSomeValue(value, span));
            if (MatchKeyword("Ok")) return ParseValueConstructor(start, "Ok", static (value, span) => new CfdOkValue(value, span));
            if (MatchKeyword("Err")) return ParseValueConstructor(start, "Err", static (value, span) => new CfdErrValue(value, span));
            if (MatchKeyword("null"))
            {
                Error("CFD-SYNTAX-NULL", "`null` is not part of the language; use `None` for Option values", SpanFrom(start));
                return new CfdInvalidValue(SpanFrom(start));
            }
            if (MatchKeyword("fn")) return ParseFunction(start, sourceStart);

            // A type marker followed by a block is an inline object. Rewind and
            // parse a bit expression for all other unquoted values.
            if (Peek() != '(')
            {
                var save = (_index, _line, _column);
                var token = ParseName("value");
                SkipTrivia();
                if (Match('{'))
                {
                    var fields = ParseFields();
                    return new CfdObjectValue(token, fields, SpanFrom(start));
                }
                (_index, _line, _column) = save;
            }
            var (expression, isExpression) = ParseBitOrExpression();
            return isExpression
                ? new CfdBitExpressionValue(expression, SpanFrom(start))
                : new CfdScalarValue(((CfdBitExpressionKind.Value)expression.Kind).Text, SpanFrom(start));
        }

        private CfdValueNode ParseValueConstructor(
            Position start,
            string name,
            Func<CfdValueNode, CfdSpan, CfdValueNode> create)
        {
            SkipTrivia();
            if (!Match('('))
                Error("CFD-SYNTAX-CONSTRUCTOR", $"expected `(` after `{name}`", CurrentSpan());
            var value = ParseValue();
            SkipTrivia();
            if (!Match(')'))
                Error("CFD-SYNTAX-CONSTRUCTOR", $"expected `)` after `{name}` value", CurrentSpan());
            return create(value, SpanFrom(start));
        }

        private CfdFunctionValue ParseFunction(Position start, int sourceStart)
        {
            SkipTrivia();
            if (!Match('('))
                Error("CFD-FUNCTION-SIGNATURE", "expected `(` after `fn`", CurrentSpan());

            var signatureDepth = 1;
            while (!End && signatureDepth != 0)
            {
                var current = Read();
                if (current == '(') signatureDepth++;
                else if (current == ')') signatureDepth--;
            }
            if (signatureDepth != 0)
                Error("CFD-FUNCTION-SIGNATURE", "unterminated function parameter list", SpanFrom(start));

            SkipTrivia();
            if (!Match("->"))
                Error("CFD-FUNCTION-SIGNATURE", "expected `->` after function parameters", CurrentSpan());
            SkipTrivia();
            var returnStarted = false;
            var angleDepth = 0;
            var bracketDepth = 0;
            while (!End)
            {
                var current = Peek();
                if (current == '{' && (angleDepth != 0 || !returnStarted))
                {
                    returnStarted = true;
                    var typeBraceDepth = 0;
                    do
                    {
                        var typeCharacter = Read();
                        if (typeCharacter == '{') typeBraceDepth++;
                        else if (typeCharacter == '}') typeBraceDepth--;
                    }
                    while (!End && typeBraceDepth != 0);
                    continue;
                }
                if (current == '{' && angleDepth == 0 && bracketDepth == 0) break;
                current = Read();
                if (!char.IsWhiteSpace(current)) returnStarted = true;
                if (current == '<') angleDepth++;
                else if (current == '>' && angleDepth > 0) angleDepth--;
                else if (current == '[') bracketDepth++;
                else if (current == ']') bracketDepth--;
            }
            if (!Match('{'))
            {
                Error("CFD-FUNCTION-BODY", "expected function body", CurrentSpan());
                return new CfdFunctionValue(_source.Text[sourceStart.._index], SpanFrom(start));
            }

            var bodyDepth = 1;
            while (!End && bodyDepth != 0)
            {
                var current = Read();
                if (current == '"')
                {
                    while (!End)
                    {
                        var text = Read();
                        if (text == '\\' && !End) Read();
                        else if (text == '"') break;
                    }
                    continue;
                }
                if (current == '#')
                {
                    while (!End && Read() != '\n') { }
                    continue;
                }
                if (current == '{') bodyDepth++;
                else if (current == '}') bodyDepth--;
            }
            if (bodyDepth != 0)
                Error("CFD-FUNCTION-BODY", "unterminated function body", SpanFrom(start));
            return new CfdFunctionValue(_source.Text[sourceStart.._index], SpanFrom(start));
        }

        private CfdValueNode ParseDictionary(Position start)
        {
            var entries = new List<CfdEntryNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match('}'))
                {
                    return new CfdDictionaryValue(entries, SpanFrom(start));
                }
                var entryStart = Cursor;
                var key = ParseDictionaryKey();
                Expect(':', "expected `:` after dictionary key");
                var value = ParseValue();
                var entry = new CfdEntryNode(key, value, SpanFrom(entryStart));
                entries.Add(entry);
                SkipTrivia();
                if (Match(','))
                    continue;
                if (!End && Peek() != '}')
                    Error("CFD-SYNTAX-010", "expected `,` or `}` after dictionary entry", CurrentSpan());
            }
            Error("CFD-SYNTAX-005", "unterminated dictionary", CurrentSpan());
            return new CfdDictionaryValue(entries, SpanFrom(start));
        }

        private CfdValueNode ParseDictionaryKey()
        {
            SkipTrivia();
            var start = Cursor;
            CfdValueNode key;
            if (Match('"'))
                key = new CfdStringValue(ParseString(), SpanFrom(start));
            else
                key = new CfdScalarValue(ParseName("dictionary key"), SpanFrom(start));
            return key;
        }

        private CfdValueNode ParseArray(Position start)
        {
            var values = new List<CfdValueNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match(']')) return new CfdArrayValue(values, SpanFrom(start));
                values.Add(ParseValue());
                SkipTrivia();
                if (Match(','))
                    continue;
                if (!End && Peek() != ']')
                {
                    Error("CFD-SYNTAX-011", "expected `,` or `]` after array item", CurrentSpan());
                    Read();
                }
            }
            Error("CFD-SYNTAX-005", "unterminated array or dictionary", CurrentSpan());
            return new CfdArrayValue(values, SpanFrom(start));
        }

        private CfdValueNode ParseFormattedString(Position start, bool prefixed)
        {
            var builder = new StringBuilder();
            var segments = new List<CfdFormatSegment>();
            var text = new StringBuilder();
            while (!End)
            {
                var character = Read();
                if (character == '"')
                {
                    if (text.Length != 0) segments.Add(new CfdFormatText(text.ToString()));
                    return new CfdFormattedStringValue(
                        (prefixed ? "f\"" : "\"") + builder + "\"",
                        segments,
                        SpanFrom(start));
                }
                if (character == '\\')
                {
                    if (End) break;
                    var escaped = Read();
                    switch (escaped)
                    {
                        case 'n': text.Append('\n'); break;
                        case 'r': text.Append('\r'); break;
                        case 't': text.Append('\t'); break;
                        case '"': text.Append('"'); break;
                        case '\\': text.Append('\\'); break;
                        default:
                            Error("CFD-SYNTAX-006", $"unsupported string escape `\\{escaped}`", CurrentSpan());
                            text.Append(escaped);
                            break;
                    }
                    builder.Append('\\').Append(escaped);
                    continue;
                }
                builder.Append(character);
                if (character == '{')
                {
                    if (!End && Peek() == '{')
                    {
                        Read();
                        builder.Append('{');
                        text.Append('{');
                        continue;
                    }
                    if (text.Length != 0) { segments.Add(new CfdFormatText(text.ToString())); text.Clear(); }
                    var referenceStart = _index;
                    var expression = ParseFormatReference();
                    builder.Append(_source.Text.Substring(referenceStart, _index - referenceStart));
                    segments.Add(expression);
                    continue;
                }
                if (character == '}')
                {
                    if (!End && Peek() == '}')
                    {
                        Read();
                        builder.Append('}');
                        text.Append('}');
                        continue;
                    }
                    Error("CFD-SYNTAX-012", "literal `}` in a formatted string must be written as `}}`", CurrentSpan());
                    continue;
                }
                text.Append(character);
            }
            Error("CFD-SYNTAX-006", "unterminated formatted string", SpanFrom(start));
            return new CfdFormattedStringValue((prefixed ? "f\"" : "\"") + builder, segments, SpanFrom(start));
        }

        private CfdFormatSegment ParseFormatReference()
        {
            var expression = new StringBuilder();
            while (!End && Peek() != '}') expression.Append(Read());
            if (!Match('}'))
            {
                Error("CFD-SYNTAX-013", "unterminated formatted string reference", CurrentSpan());
                return new CfdFormatText("{" + expression);
            }
            var value = expression.ToString().Trim();
            var (typeName, key, path) = ParseFormatReferenceText(value);
            if (!IsValidFormatReference(value, typeName, key, path))
                Error("CFD-SYNTAX-013", "formatted string reference must use `field`, `&key.field`, or `&Type::key.field`", CurrentSpan());
            return new CfdFormatReference(typeName, key, path);
        }

        private static (string? TypeName, string? Key, IReadOnlyList<string> Path) ParseFormatReferenceText(string expression)
        {
            var reference = expression.StartsWith('&') ? expression[1..] : expression;
            string? typeName = null;
            string? key = null;
            if (reference.Contains("::", StringComparison.Ordinal))
            {
                var separator = reference.LastIndexOf("::", StringComparison.Ordinal);
                typeName = reference[..separator];
                reference = reference[(separator + 2)..];
            }
            var parts = reference.Split('.');
            if (expression.StartsWith('&'))
            {
                key = parts.Length == 0 ? string.Empty : parts[0];
                parts = parts.Skip(1).ToArray();
            }
            return (typeName, key, parts);
        }

        private CfdValueNode? ParseAutomaticFormat(string value, CfdSpan span)
        {
            if (!value.Contains('{')) return null;
            var segments = new List<CfdFormatSegment>();
            var text = new StringBuilder();
            var hasReference = false;
            for (var i = 0; i < value.Length; i++)
            {
                if (i + 1 < value.Length && value[i] == '{' && value[i + 1] == '{') { text.Append('{'); i++; continue; }
                if (i + 1 < value.Length && value[i] == '}' && value[i + 1] == '}') { text.Append('}'); i++; continue; }
                if (value[i] != '{') { text.Append(value[i]); continue; }
                var end = value.IndexOf('}', i + 1);
                if (end < 0) return null;
                var expression = value[(i + 1)..end].Trim();
                var parsed = ParseFormatReferenceText(expression);
                if (!IsValidFormatReference(expression, parsed.TypeName, parsed.Key, parsed.Path))
                {
                    if (expression.StartsWith('&'))
                        Error("CFD-SYNTAX-013", "formatted string reference must use `field`, `&key.field`, or `&Type::key.field`", span);
                    return null;
                }
                if (text.Length != 0) { segments.Add(new CfdFormatText(text.ToString())); text.Clear(); }
                segments.Add(new CfdFormatReference(parsed.TypeName, parsed.Key, parsed.Path));
                hasReference = true;
                i = end;
            }
            if (text.Length != 0) segments.Add(new CfdFormatText(text.ToString()));
            return hasReference ? new CfdFormattedStringValue(value, segments, span) : null;
        }

        private static bool IsValidFormatReference(
            string expression,
            string? typeName,
            string? key,
            IReadOnlyList<string> path) =>
            expression.Length != 0 && !expression.Any(char.IsWhiteSpace) && path.Count != 0 &&
            (expression.StartsWith('&') ? key is not null : key is null) &&
            (typeName is null || expression.StartsWith('&')) &&
            (typeName is null || IsQualifiedName(typeName)) &&
            (key is null || IsReferenceName(key)) && path.All(IsReferenceName);

        private static bool IsReferenceName(string value) => CfdIdentifiers.IsIdentifierName(value);

        private static bool IsQualifiedName(string value) =>
            value.Split(new[] { "::" }, StringSplitOptions.None).All(IsIdentifier);

        private static bool IsIdentifier(string value) => CfdIdentifiers.IsIdentifier(value);

        private (CfdBitExpression Expression, bool IsExpression) ParseBitOrExpression() =>
            ParseBitBinary(ParseBitXorExpression, '|', CfdBitOperator.Or);

        private (CfdBitExpression Expression, bool IsExpression) ParseBitXorExpression() =>
            ParseBitBinary(ParseBitAndExpression, '^', CfdBitOperator.Xor);

        private (CfdBitExpression Expression, bool IsExpression) ParseBitAndExpression() =>
            ParseBitBinary(ParseBitPrimary, '&', CfdBitOperator.And);

        private (CfdBitExpression Expression, bool IsExpression) ParseBitBinary(
            Func<(CfdBitExpression Expression, bool IsExpression)> operand,
            char symbol,
            CfdBitOperator operation)
        {
            var (left, isExpression) = operand();
            while (true)
            {
                SkipTrivia();
                if (!Match(symbol)) break;
                var (right, _) = operand();
                left = CfdBitExpression.Binary(operation, left, right, new CfdSpan(left.Span.StartLine, left.Span.StartColumn, right.Span.EndLine, right.Span.EndColumn));
                isExpression = true;
            }
            return (left, isExpression);
        }

        private (CfdBitExpression Expression, bool IsExpression) ParseBitPrimary()
        {
            SkipTrivia();
            var start = Cursor;
            if (Match('('))
            {
                var (expression, _) = ParseBitOrExpression();
                Expect(')', "expected `)` after bit expression");
                return (expression.WithSpan(SpanFrom(start)), true);
            }
            var token = ParseName("flag expression operand");
            return (CfdBitExpression.Value(token, SpanFrom(start)), false);
        }

        private string ParseString()
        {
            var builder = new StringBuilder();
            while (!End)
            {
                var character = Read();
                if (character == '"') return builder.ToString();
                if (character != '\\') { builder.Append(character); continue; }
                if (End) break;
                var escaped = Read();
                builder.Append(escaped switch
                {
                    'n' => '\n', 'r' => '\r', 't' => '\t', '"' => '"', '\\' => '\\',
                    _ => UnsupportedEscape(escaped),
                });
            }
            Error("CFD-SYNTAX-006", "unterminated string", CurrentSpan());
            return builder.ToString();
        }

        private char UnsupportedEscape(char escaped)
        {
            Error("CFD-SYNTAX-006", $"unsupported string escape `\\{escaped}`", CurrentSpan());
            return escaped;
        }

        private string ParseKey(string expected)
        {
            SkipTrivia();
            if (Match('"')) return ParseString();
            return ParseName(expected);
        }

        private string ParseName(string expected)
        {
            SkipTrivia();
            if (Match('"'))
            {
                var value = ParseString();
                Error("CFD-SYNTAX-007", $"{expected} must be an unquoted name", CurrentSpan());
                return value;
            }
            var builder = new StringBuilder();
            while (!End)
            {
                var character = Peek();
                if (character == ':' && _source.Text.AsSpan(_index).StartsWith("::"))
                {
                    builder.Append(Read()).Append(Read());
                    continue;
                }
                if (char.IsWhiteSpace(character) || "{}[],:=#;|^&()@\"".Contains(character)) break;
                builder.Append(Read());
            }
            if (builder.Length == 0)
            {
                Error("CFD-SYNTAX-007", $"expected {expected}", CurrentSpan());
                return "_";
            }
            return builder.ToString();
        }

        private void RecoverTopLevel()
        {
            while (!End && Peek() != '\n') Read();
        }

        private void SkipTrivia()
        {
            while (!End)
            {
                if (char.IsWhiteSpace(Peek())) { Read(); continue; }
                if (Peek() == '#') { while (!End && Read() != '\n') { } continue; }
                break;
            }
        }

        private void Expect(char character, string message) { if (!Match(character)) Error("CFD-SYNTAX-008", message, CurrentSpan()); }
        private bool Match(char character) { if (!End && Peek() == character) { Read(); return true; } return false; }
        private bool Match(string value)
        {
            var save = (_index, _line, _column);
            foreach (var character in value)
            {
                if (End || Peek() != character)
                {
                    (_index, _line, _column) = save;
                    return false;
                }
                Read();
            }
            return true;
        }

        private bool MatchKeyword(string value)
        {
            var save = (_index, _line, _column);
            if (!Match(value)) return false;
            if (!End && !char.IsWhiteSpace(Peek()) && !"{}[],:=#;|^&()@\"".Contains(Peek()))
            {
                (_index, _line, _column) = save;
                return false;
            }
            return true;
        }
        private char Peek() => _source.Text[_index];
        private char Read() { var value = _source.Text[_index++]; if (value == '\n') { _line++; _column = 1; } else _column++; return value; }
        private bool End => _index >= _source.Text.Length;
        private Position Cursor => new(_line, _column);
        private CfdSpan CurrentSpan() => new(_line, _column, _line, _column);
        private CfdSpan SpanFrom(Position start) => new(start.Line, start.Column, _line, _column);
        private void Error(string code, string message, CfdSpan span) => _errors.Add(new CfdDiagnostic(code, message, _source.Path, span));
        private void ThrowIfErrors() { if (_errors.Count != 0) throw new CfdParseException(_errors); }
        private readonly record struct Position(int Line, int Column);
    }
}
