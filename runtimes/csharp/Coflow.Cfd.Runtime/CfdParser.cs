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
                var start = Position;
                var first = ParseIdentifier("record key or type");
                if (Match(':'))
                {
                    var type = ParseIdentifier("record type");
                    AddRecord(records, ParseRecord(first, type, start));
                    if (records.Count > _options.MaxRecords)
                        Error("CFD-LIMIT-RECORDS", "source exceeds the record limit", CurrentSpan());
                }
                else if (Match('{'))
                {
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
            Enter();
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) { Exit(); return; }
                var keyStart = Position;
                var key = ParseIdentifier("record key");
                string type = groupType;
                if (Match(':')) type = ParseIdentifier("record type");
                AddRecord(records, ParseRecord(key, type, keyStart));
                if (records.Count > _options.MaxRecords)
                    Error("CFD-LIMIT-RECORDS", "source exceeds the record limit", CurrentSpan());
            }
            Error("CFD-SYNTAX-002", "unterminated grouped record", SpanFrom(start));
            Exit();
        }

        private void AddRecord(List<CfdRecordNode> records, CfdRecordNode record)
        {
            if (!_recordKeys.Add((record.DeclaredType, record.Key)))
                Error("CFD-SYNTAX-DUPLICATE-RECORD", $"record `{record.DeclaredType}.{record.Key}` is declared more than once", record.Span);
            records.Add(record);
        }

        private CfdRecordNode ParseRecord(string key, string type, Position start)
        {
            Expect('{', "expected `{` after record declaration");
            Enter();
            var fields = ParseFields();
            Exit();
            return new CfdRecordNode(key, type, fields, SpanFrom(start));
        }

        private List<CfdFieldNode> ParseFields()
        {
            var fields = new List<CfdFieldNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) return fields;
                var start = Position;
                var name = ParseIdentifier("field name");
                Expect(':', "expected `:` after field name");
                var value = ParseValue();
                if (fields.Any(field => field.Name == name))
                    Error("CFD-SYNTAX-DUPLICATE-FIELD", $"field `{name}` is declared more than once", SpanFrom(start));
                fields.Add(new CfdFieldNode(name, value, SpanFrom(start)));
                Match(',');
            }
            Error("CFD-SYNTAX-003", "unterminated record", CurrentSpan());
            return fields;
        }

        private CfdValueNode ParseValue()
        {
            SkipTrivia();
            var start = Position;
            if (End) { Error("CFD-SYNTAX-004", "expected a value", CurrentSpan()); return new CfdNullValue(CurrentSpan()); }
            if (Match('"')) return new CfdStringValue(ParseString(), SpanFrom(start));
            if (Match('&')) return new CfdReferenceValue(ParseIdentifier("reference key"), SpanFrom(start));
            if (Match('[')) return ParseArray(start);
            if (Match('{')) return ParseDictionary(start);
            if (Match("null")) return new CfdNullValue(SpanFrom(start));
            var token = ParseIdentifier("value");
            SkipTrivia();
            if (Match('{'))
            {
                Enter();
                var fields = ParseFields();
                Exit();
                return new CfdObjectValue(token, fields, SpanFrom(start));
            }
            return new CfdScalarValue(token, SpanFrom(start));
        }

        private CfdValueNode ParseDictionary(Position start)
        {
            Enter();
            var entries = new List<CfdEntryNode>();
            while (!End)
            {
                SkipTrivia();
                if (Match('}')) { Exit(); return new CfdDictionaryValue(entries, SpanFrom(start)); }
                var entryStart = Position;
                var key = ParseValue();
                Expect(':', "expected `:` after dictionary key");
                var value = ParseValue();
                entries.Add(new CfdEntryNode(key, value, SpanFrom(entryStart)));
                Match(',');
            }
            Error("CFD-SYNTAX-005", "unterminated dictionary", CurrentSpan());
            Exit();
            return new CfdDictionaryValue(entries, SpanFrom(start));
        }

        private CfdValueNode ParseArray(Position start)
        {
            Enter();
            var values = new List<CfdValueNode>();
            var entries = new List<CfdEntryNode>();
            var dictionary = false;
            while (!End)
            {
                SkipTrivia();
                if (Match(']')) { Exit(); return new CfdArrayValue(values, SpanFrom(start)); }
                var entryStart = Position;
                var key = ParseValue();
                SkipTrivia();
                if (Match(':'))
                {
                    dictionary = true;
                    entries.Add(new CfdEntryNode(key, ParseValue(), SpanFrom(entryStart)));
                }
                else values.Add(key);
                Match(',');
            }
            Error("CFD-SYNTAX-005", "unterminated array or dictionary", CurrentSpan());
            Exit();
            return dictionary ? new CfdDictionaryValue(entries, SpanFrom(start)) : new CfdArrayValue(values, SpanFrom(start));
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
                    'u' => ParseUnicodeEscape(), _ => escaped,
                });
            }
            Error("CFD-SYNTAX-006", "unterminated string", CurrentSpan());
            return builder.ToString();
        }

        private char ParseUnicodeEscape()
        {
            var digits = new string(Enumerable.Range(0, 4).Select(_ => End ? '0' : Read()).ToArray());
            return ushort.TryParse(digits, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var value) ? (char)value : '?';
        }

        private string ParseIdentifier(string expected)
        {
            SkipTrivia();
            var builder = new StringBuilder();
            while (!End)
            {
                var character = Peek();
                if (char.IsWhiteSpace(character) || "{}[],:#".Contains(character)) break;
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
            _nodes++;
            if (_depth > _options.MaxDepth) Error("CFD-LIMIT-DEPTH", "nested value exceeds the depth limit", CurrentSpan());
            if (_nodes > _options.MaxNodes) Error("CFD-LIMIT-NODES", "source exceeds the node limit", CurrentSpan());
        }

        private void Exit() => _depth = Math.Max(0, _depth - 1);
        private void Expect(char character, string message) { if (!Match(character)) Error("CFD-SYNTAX-008", message, CurrentSpan()); }
        private bool Match(char character) { if (!End && Peek() == character) { Read(); return true; } return false; }
        private bool Match(string value) { var save = (_index, _line, _column); foreach (var character in value) if (End || Peek() != character) { (_index, _line, _column) = save; return false; } else Read(); return true; }
        private char Peek() => _source.Text[_index];
        private char Read() { var value = _source.Text[_index++]; if (value == '\n') { _line++; _column = 1; } else _column++; return value; }
        private bool End => _index >= _source.Text.Length;
        private Position Position => new(_line, _column);
        private CfdSpan CurrentSpan() => new(_line, _column, _line, _column);
        private CfdSpan SpanFrom(Position start) => new(start.Line, start.Column, _line, _column);
        private void Error(string code, string message, CfdSpan span) => _errors.Add(new CfdDiagnostic(code, message, _source.Path, span));
        private void ThrowIfErrors() { if (_errors.Count != 0) throw new CfdParseException(_errors); }
        private readonly record struct Position(int Line, int Column);
    }
}
