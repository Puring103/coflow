using System.Globalization;
using System.Text;

namespace Coflow.Cfd.Runtime;

/// <summary>Schema-free parser shared by generated target models.</summary>
public static class CfdParser
{
    public static CfdDocument Parse(CfdSource source, CfdLoadOptions? options = null)
    {
        var parser = new Parser(source, options ?? new CfdLoadOptions());
        return parser.ParseDocument();
    }

    public static IReadOnlyList<CfdDocument> ParseAll(IEnumerable<CfdSource> sources, CfdLoadOptions? options = null)
    {
        var documents = new List<CfdDocument>();
        foreach (var source in sources)
            documents.Add(Parse(source, options));
        return documents;
    }

    private sealed class Parser
    {
        private readonly CfdSource _source;
        private readonly CfdLoadOptions _options;
        private readonly List<CfdDiagnostic> _errors = new();
        private readonly HashSet<(string Type, string Key)> _recordKeys = new();
        private int _index;
        private int _line = 1;
        private int _column = 1;
        private int _nodes;
        private int _depth;

        public Parser(CfdSource source, CfdLoadOptions options)
        {
            _source = source;
            _options = options;
            if (Encoding.UTF8.GetByteCount(source.Text) > options.MaxSourceBytes)
                Error("CFD-LIMIT-SOURCE", "source exceeds the configured byte limit", CurrentSpan());
        }

        public CfdDocument ParseDocument()
        {
            var records = new List<CfdRecordNode>();
            while (!End)
            {
                SkipTrivia();
                if (End) break;
                var start = Cursor;
                var firstWasQuoted = Peek() == '"';
                var first = ParseKey("record key or type");
                SkipTrivia();
                if (Match(':'))
                {
                    var type = ParseName("record type");
                    AddRecord(records, ParseRecord(first, type, start));
                    if (records.Count > _options.MaxRecords)
                        Error("CFD-LIMIT-RECORDS", "source exceeds the record limit", CurrentSpan());
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
            return new CfdDocument(_source.Path, records);
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
                if (records.Count > _options.MaxRecords)
                    Error("CFD-LIMIT-RECORDS", "source exceeds the record limit", CurrentSpan());
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
            if (string.IsNullOrEmpty(value) || value is "_" or "id" or "Id" or "ID" or "const" or
                "enum" or "type" or "abstract" or "sealed" or "check" or "when" or "all" or
                "any" or "none" or "in" or "is" or "true" or "false" or "null" or "int" or
                "float" or "bool" or "string" or "len" or "contains" or "isUnique" or "min" or
                "max" or "sum" or "keys" or "values" or "matches" or "if" or "else" or "match" or
                "case" or "for" or "while" or "let" or "module" or "import" or "export" or "from" or
                "as" or "use")
                return false;
            var index = 0;
            if (!ReadIdentifierCodePoint(value, ref index, true)) return false;
            while (index < value.Length)
                if (!ReadIdentifierCodePoint(value, ref index, false)) return false;
            return true;
        }

        private static bool ReadIdentifierCodePoint(string value, ref int index, bool start)
        {
            int codePoint = value[index];
            var width = 1;
            if (char.IsHighSurrogate(value[index]) &&
                index + 1 < value.Length &&
                char.IsLowSurrogate(value[index + 1]))
            {
                codePoint = char.ConvertToUtf32(value[index], value[index + 1]);
                width = 2;
            }
            else if (char.IsSurrogate(value[index]))
            {
                index++;
                return false;
            }

            var category = CharUnicodeInfo.GetUnicodeCategory(value, index);
            index += width;
            var identifierStart = category is
                UnicodeCategory.UppercaseLetter or
                UnicodeCategory.LowercaseLetter or
                UnicodeCategory.TitlecaseLetter or
                UnicodeCategory.ModifierLetter or
                UnicodeCategory.OtherLetter or
                UnicodeCategory.LetterNumber ||
                codePoint is 0x1885 or 0x1886 or 0x2118 or 0x212e or 0x309b or 0x309c;
            if (start) return codePoint == '_' || identifierStart;
            return codePoint == '_' || identifierStart || category is
                UnicodeCategory.NonSpacingMark or
                UnicodeCategory.SpacingCombiningMark or
                UnicodeCategory.DecimalDigitNumber or
                UnicodeCategory.ConnectorPunctuation ||
                codePoint is 0x00b7 or 0x0387 or 0x1369 or 0x136a or 0x136b or 0x136c or
                    0x136d or 0x136e or 0x136f or 0x1370 or 0x1371 or 0x19da;
        }

        private CfdRecordNode ParseRecord(
            string key,
            string type,
            Position start,
            string? groupType = null)
        {
            SkipTrivia();
            Expect('{', "expected `{` after record declaration");
            Enter();
            var fields = ParseFields();
            Exit();
            var record = new CfdRecordNode(key, type, fields, SpanFrom(start), groupType);
            ChargeNode(record.Span); // Record field block.
            ChargeNode(record.Span); // Record.
            return record;
        }

        private List<CfdFieldNode> ParseFields()
        {
            var fields = new List<CfdFieldNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) return fields;
                var start = Cursor;
                var name = ParseKey("field name");
                Expect(':', "expected `:` after field name");
                var value = ParseValue();
                if (fields.Any(field => field.Name == name))
                    Error("CFD-SYNTAX-DUPLICATE-FIELD", $"field `{name}` is declared more than once", SpanFrom(start));
                var field = new CfdFieldNode(name, value, SpanFrom(start));
                fields.Add(field);
                ChargeNode(field.Span);
                SkipTrivia();
                if (Match(','))
                    continue;
                if (!End && Peek() != '}')
                    Error("CFD-SYNTAX-009", "expected `,` or `}` after field", CurrentSpan());
            }
            Error("CFD-SYNTAX-003", "unterminated record", CurrentSpan());
            return fields;
        }

        private CfdValueNode ParseValue()
        {
            var value = ParseValueInner();
            ChargeNode(value.Span);
            return value;
        }

        private CfdValueNode ParseValueInner()
        {
            SkipTrivia();
            var start = Cursor;
            if (End) { Error("CFD-SYNTAX-004", "expected a value", CurrentSpan()); return new CfdNullValue(CurrentSpan()); }
            if (Match("f\"")) return ParseFormattedString(start, true);
            if (Match('"'))
            {
                var value = ParseString();
                var span = SpanFrom(start);
                return ParseAutomaticFormat(value, span) ?? new CfdStringValue(value, span);
            }
            if (Match('&'))
            {
                var quoted = !End && Peek() == '"';
                var key = ParseKey("reference key");
                if (quoted || !IsRecordKey(key))
                    Error("CFD-SYNTAX-RECORD-KEY", $"invalid record key `{key}`", SpanFrom(start));
                return new CfdReferenceValue(key, SpanFrom(start));
            }
            if (Match('[')) return ParseArray(start);
            if (Match('{')) return ParseDictionary(start);
            if (MatchKeyword("null")) return new CfdNullValue(SpanFrom(start));

            // A type marker followed by a block is an inline object. Rewind and
            // parse a bit expression for all other unquoted values.
            if (Peek() != '(')
            {
                var save = (_index, _line, _column);
                var token = ParseName("value");
                SkipTrivia();
                if (Match('{'))
                {
                    Enter();
                    var fields = ParseFields();
                    Exit();
                    var value = new CfdObjectValue(token, fields, SpanFrom(start));
                    ChargeNode(value.Span); // Inline object block.
                    return value;
                }
                (_index, _line, _column) = save;
            }
            var (expression, isExpression) = ParseBitOrExpression();
            return isExpression
                ? new CfdBitExpressionValue(expression, SpanFrom(start))
                : new CfdScalarValue(((CfdBitExpressionKind.Value)expression.Kind).Text, SpanFrom(start));
        }

        private CfdValueNode ParseDictionary(Position start)
        {
            Enter();
            var entries = new List<CfdEntryNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match('}'))
                {
                    Exit();
                    var dictionary = new CfdDictionaryValue(entries, SpanFrom(start));
                    ChargeNode(dictionary.Span); // Dictionary block.
                    return dictionary;
                }
                var entryStart = Cursor;
                var key = ParseDictionaryKey();
                Expect(':', "expected `:` after dictionary key");
                var value = ParseValue();
                var entry = new CfdEntryNode(key, value, SpanFrom(entryStart));
                entries.Add(entry);
                ChargeNode(entry.Span);
                SkipTrivia();
                if (Match(','))
                    continue;
                if (!End && Peek() != '}')
                    Error("CFD-SYNTAX-010", "expected `,` or `}` after dictionary entry", CurrentSpan());
            }
            Error("CFD-SYNTAX-005", "unterminated dictionary", CurrentSpan());
            Exit();
            var unterminated = new CfdDictionaryValue(entries, SpanFrom(start));
            ChargeNode(unterminated.Span);
            return unterminated;
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
            ChargeNode(key.Span);
            return key;
        }

        private CfdValueNode ParseArray(Position start)
        {
            Enter();
            var values = new List<CfdValueNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match(']')) { Exit(); return new CfdArrayValue(values, SpanFrom(start)); }
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
            Exit();
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
                var separator = reference.IndexOf("::", StringComparison.Ordinal);
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
            (typeName is null || IsReferenceName(typeName)) &&
            (key is null || IsReferenceName(key)) && path.All(IsReferenceName);

        private static bool IsReferenceName(string value) =>
            !string.IsNullOrEmpty(value) &&
            (value[0] == '_' || char.IsLetter(value[0])) &&
            value.Skip(1).All(character => character == '_' || char.IsLetterOrDigit(character));

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
                ChargeNode(left.Span);
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
                Enter();
                var (expression, _) = ParseBitOrExpression();
                Expect(')', "expected `)` after bit expression");
                Exit();
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

        private void Enter()
        {
            _depth++;
            if (_depth > _options.MaxDepth) Error("CFD-LIMIT-DEPTH", "nested value exceeds the depth limit", CurrentSpan());
        }

        private void ChargeNode(CfdSpan span)
        {
            _nodes++;
            if (_nodes > _options.MaxNodes) Error("CFD-LIMIT-NODES", "source exceeds the node limit", span);
        }

        private void Exit() => _depth = Math.Max(0, _depth - 1);
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
