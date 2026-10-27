using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
using System.Globalization;
using System.ComponentModel;
using System.Text;

namespace Coflow.Loader
{

/// <summary>Schema-free parser shared by generated target models.</summary>
internal static class CfdParser
{
    public static CfdDocument Parse(CfdSource source)
    {
        var parser = new Parser(source);
        return parser.ParseDocument();
    }

    public static CfdValueNode ParseValue(CfdSource source)
    {
        var parser = new Parser(source);
        return parser.ParseSingleValue();
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
        private readonly HashSet<(string Type, string Id)> _recordIds = new();
        private int _index;
        private int _depth;
        private int _line = 1;
        private int _column = 1;

        public Parser(CfdSource source)
        {
            _source = source;
        }

        public CfdDocument ParseDocument()
        {
            var records = new List<CfdRecordNode>();
            var dimensions = new List<CfdDimensionBlock>();
            SkipTrivia();
            while (!End)
            {
                SkipTrivia();
                if (End) break;
                var start = Cursor;
                var firstWasQuoted = Peek() == '"';
                var first = ParseKey("record id or type");
                SkipTrivia();
                if (Match(':'))
                {
                    var type = ParseTypeName("record type");
                    AddRecord(records, ParseRecord(first, type, start));
                }
                else if (Match('['))
                {
                    if (firstWasQuoted || !IsIdentifier(first))
                        Error("CFD-SYNTAX-DIMENSION", "dimension must be an identifier", SpanFrom(start));
                    var variant = ParseName("dimension variant");
                    if (!IsIdentifier(variant))
                        Error("CFD-SYNTAX-DIMENSION", "variant must be an identifier", SpanFrom(start));
                    SkipTrivia();
                    Expect(']', "expected `]` after variant");
                    SkipTrivia();
                    Expect('{', "expected `{` after dimension variant");
                    var overrides = new List<CfdRecordNode>();
                    while (true)
                    {
                        SkipTrivia();
                        if (Match('}')) break;
                        if (End) Error("CFD-SYNTAX-DIMENSION", "unterminated dimension block", SpanFrom(start));
                        var recordStart = Cursor;
                        var key = ParseKey("override record id");
                        SkipTrivia();
                        Expect(':', "expected `:` after override record id");
                        var type = ParseTypeName("override record type");
                        overrides.Add(ParseRecord(key, type, recordStart));
                        SkipTrivia();
                        Match(',');
                    }
                    dimensions.Add(new CfdDimensionBlock(first, variant, overrides, SpanFrom(start)));
                }
                else if (Match('{'))
                {
                    if (firstWasQuoted)
                        Error("CFD-SYNTAX-007", "group type must be an unquoted name", SpanFrom(start));
                    if (first.Contains("::", StringComparison.Ordinal))
                        Error("CFD-SYNTAX-007", "group type must be a single identifier", SpanFrom(start));
                    ParseGroupedRecords(records, first, start);
                }
                else
                {
                    Error("CFD-SYNTAX-001", "expected `:` or `{` after record declaration", CurrentSpan());
                    RecoverTopLevel();
                }
            }
            return new CfdDocument(_source.Path, records, dimensions);
        }

        public CfdValueNode ParseSingleValue()
        {
            var value = ParseValue();
            SkipTrivia();
            if (!End) Error("CFD-SYNTAX-DEFAULT", "unexpected text after default value", CurrentSpan());
            return value;
        }

        private void ParseGroupedRecords(List<CfdRecordNode> records, string groupType, Position start)
        {
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) return;
                var keyStart = Cursor;
                var key = ParseKey("record id");
                SkipTrivia();
                string type = groupType;
                if (Match(':')) type = ParseTypeName("record type");
                AddRecord(records, ParseRecord(key, type, keyStart, groupType));
                SkipTrivia();
                Match(',');
            }
            Error("CFD-SYNTAX-002", "unterminated grouped record", SpanFrom(start));
        }

        private void AddRecord(List<CfdRecordNode> records, CfdRecordNode record)
        {
            if (!IsRecordId(record.Id))
                Error("CFD-SYNTAX-RECORD-ID", $"invalid record id `{record.Id}`", record.Span);
            if (!_recordIds.Add((record.DeclaredType, record.Id)))
                Error("CFD-SYNTAX-DUPLICATE-RECORD", $"record `{record.DeclaredType}.{record.Id}` is declared more than once", record.Span);
            records.Add(record);
        }

        private static bool IsRecordId(string value)
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

        private CfdValueNode ParseValue()
        {
            // 解析深度有限，恶意嵌套不得耗尽宿主调用栈。
            EnterNesting();
            try { return ParseValueInner(); } finally { _depth--; }
        }

        private CfdValueNode ParseValueInner()
        {
            SkipTrivia();
            var start = Cursor;
            if (End) { Error("CFD-SYNTAX-004", "expected a value", CurrentSpan()); return new CfdInvalidValue(CurrentSpan()); }
            if (Match("f\""))
            {
                var value = ParseString();
                return ParseFormat(value, SpanFrom(start));
            }
            if (Match('"')) return new CfdStringValue(ParseString(), SpanFrom(start));
            if (Match('&'))
            {
                var reference = ParseName("reference id");
                var separator = reference.LastIndexOf("::", StringComparison.Ordinal);
                var typeName = separator < 0 ? null : reference[..separator];
                var key = separator < 0 ? reference : reference[(separator + 2)..];
                if (!IsRecordId(key) || (typeName is not null && !IsQualifiedName(typeName)))
                    Error("CFD-SYNTAX-RECORD-ID", $"invalid record id `{key}`", SpanFrom(start));
                return new CfdReferenceValue(typeName, key, SpanFrom(start));
            }
            if (Match('[')) return ParseArray(start);
            if (Match('{')) return ParseDictionary(start);
            if (MatchKeyword("None")) return new CfdNoneValue(SpanFrom(start));
            if (MatchKeyword("Some"))
            {
                SkipTrivia();
                Expect('(', "expected `(` after Some");
                var value = ParseValue();
                SkipTrivia();
                Expect(')', "expected `)` after Some value");
                return new CfdSomeValue(value, SpanFrom(start));
            }

            // A type marker followed by a block is an inline object. Rewind and
            // parse a bit expression for all other unquoted values.
            if (Peek() != '(')
            {
                var save = (_index, _line, _column);
                var token = ParseName("value");
                SkipTrivia();
                if (Match('{'))
                {
                    if (!IsQualifiedName(token))
                        Error("CFD-SYNTAX-007", "invalid object type", SpanFrom(start));
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

        private static (string? TypeName, string? Id, IReadOnlyList<string> Path) ParseFormatReferenceText(string expression)
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

        private CfdValueNode ParseFormat(string value, CfdSpan span)
        {
            var segments = new List<CfdFormatSegment>();
            var text = new StringBuilder();
            for (var i = 0; i < value.Length; i++)
            {
                if (i + 1 < value.Length && value[i] == '{' && value[i + 1] == '{') { text.Append('{'); i++; continue; }
                if (i + 1 < value.Length && value[i] == '}' && value[i + 1] == '}') { text.Append('}'); i++; continue; }
                if (value[i] == '}') Error("CFD-SYNTAX-013", "unescaped closing brace", span);
                if (value[i] != '{') { text.Append(value[i]); continue; }
                var end = value.IndexOf('}', i + 1);
                if (end < 0) Error("CFD-SYNTAX-013", "unterminated interpolation", span);
                var expression = value[(i + 1)..end].Trim();
                var parsed = ParseFormatReferenceText(expression);
                if (!IsValidFormatReference(expression, parsed.TypeName, parsed.Id, parsed.Path))
                {
                    Error("CFD-SYNTAX-013", "interpolation requires `self.field` or `&Type::id.field`", span);
                }
                if (text.Length != 0) { segments.Add(new CfdFormatText(text.ToString())); text.Clear(); }
                segments.Add(new CfdFormatReference(parsed.TypeName, parsed.Id, parsed.Path));
                i = end;
            }
            if (text.Length != 0) segments.Add(new CfdFormatText(text.ToString()));
            return new CfdFormattedStringValue(value, segments, span);
        }

        private static bool IsValidFormatReference(
            string expression,
            string? typeName,
            string? key,
            IReadOnlyList<string> path) =>
            expression.Length != 0 && !expression.Any(char.IsWhiteSpace) && path.Count != 0 && path.Count <= 128 &&
            (expression.StartsWith('&') ? key is not null && typeName is not null : key is null && path.Count > 1 && path[0] == "self") &&
            (typeName is null || expression.StartsWith('&')) &&
            (typeName is null || IsQualifiedName(typeName)) &&
            (key is null || IsReferenceName(key)) && path.All(IsReferenceName);

        private static bool IsReferenceName(string value) => CfdIdentifiers.IsIdentifierName(value);

        private static bool IsQualifiedName(string value) => value.Split(new[] { "::" }, StringSplitOptions.None).All(IsIdentifier);

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
                var span = new CfdSpan(left.Span.StartLine, left.Span.StartColumn, right.Span.EndLine, right.Span.EndColumn);
                if (_depth + Math.Max(left.Depth, right.Depth) + 1 > 256) Error("CFD-LIMIT", "bit expression depth exceeds 256 levels", span);
                left = CfdBitExpression.Binary(operation, left, right, span);
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
                // 括号表达式和复合值共享深度预算，递归前检查而非返回后检查。
                EnterNesting();
                try
                {
                    var (expression, _) = ParseBitOrExpression();
                    SkipTrivia();
                    Expect(')', "expected `)` after bit expression");
                    return (expression.WithSpan(SpanFrom(start)), true);
                }
                finally { _depth--; }
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

        private string ParseTypeName(string expected)
        {
            var start = Cursor;
            var name = ParseName(expected);
            if (!IsQualifiedName(name))
                Error("CFD-SYNTAX-007", $"invalid {expected}", SpanFrom(start));
            return name;
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

        private void EnterNesting()
        {
            if (++_depth > 256) Error("CFD-LIMIT", "value nesting exceeds 256 levels", CurrentSpan());
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
        private readonly struct Position
        {
            internal Position(int line, int column) { Line = line; Column = column; }
            internal int Line { get; }
            internal int Column { get; }
        }
        private Position Cursor => new(_line, _column);
        private CfdSpan CurrentSpan() => new(_line, _column, _line, _column);
        private CfdSpan SpanFrom(Position start) => new(start.Line, start.Column, _line, _column);
        private void Error(string code, string message, CfdSpan span) => throw new CfdParseException(new[] { new CfdDiagnostic(code, message, _source.Path, span) });
    }
}
}
