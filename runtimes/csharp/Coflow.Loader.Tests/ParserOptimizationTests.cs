using System;
using System.Linq;
using Coflow.Loader;
using Xunit;

namespace Coflow.Loader.Tests;

public sealed class ParserOptimizationTests
{
    [Theory]
    [InlineData("42")]
    [InlineData("-12.5e+4")]
    [InlineData("true")]
    [InlineData("Flags::Read")]
    [InlineData("名字𐐀")]
    public void PlainScalarsRetainTextAndTriviaSpans(string token)
    {
        var value = Assert.IsType<CfdScalarValue>(Parse("  " + token + " # trailing\n "));
        Assert.Equal(token, value.Value);
        Assert.Equal(new CfdSpan(1, 3, 2, 2), value.Span);
    }

    [Fact]
    public void UnicodeNamesAndInlineObjectsRetainTheirBoundaries()
    {
        var document = CfdParser.Parse(new CfdSource("unicode.cfd", "𐐀名: 空间::记录 { 字段: 空间::数据 { 值: 42 } }"));
        var record = Assert.Single(document.Records);
        Assert.Equal("𐐀名", record.Id);
        Assert.Equal("空间::记录", record.DeclaredType);
        var field = Assert.Single(record.Fields);
        Assert.Equal("字段", field.Name);
        var value = Assert.IsType<CfdObjectValue>(field.Value);
        Assert.Equal("空间::数据", value.DeclaredType);
        Assert.Equal("42", Assert.IsType<CfdScalarValue>(Assert.Single(value.Fields).Value).Value);
    }

    [Theory]
    [InlineData("\"\"", "")]
    [InlineData("\"plain 中文 𐐀\"", "plain 中文 𐐀")]
    [InlineData("\"prefix\\n中\\t\\\"\\\\suffix\"", "prefix\n中\t\"\\suffix")]
    [InlineData("\"\\r\\n\\t\\\"\\\\\"", "\r\n\t\"\\")]
    public void StringsDecodeEscapesAndPreserveSourceColumns(string source, string expected)
    {
        var value = Assert.IsType<CfdStringValue>(Parse(source));
        Assert.Equal(expected, value.Value);
        Assert.Equal(new CfdSpan(1, 1, 1, source.Length + 1), value.Span);
    }

    [Fact]
    public void LiteralNewlinesAndEscapedSegmentsUseSourceLineNumbers()
    {
        var plain = Assert.IsType<CfdStringValue>(Parse("\"first\r\n第二行\""));
        Assert.Equal("first\r\n第二行", plain.Value);
        Assert.Equal(new CfdSpan(1, 1, 2, 5), plain.Span);
        var escaped = Assert.IsType<CfdStringValue>(Parse("\"first\\t\n后\\n尾\""));
        Assert.Equal("first\t\n后\n尾", escaped.Value);
        Assert.Equal(new CfdSpan(1, 1, 2, 6), escaped.Span);
    }

    [Theory]
    [InlineData("A | B ^ C & D | E", "(| (| A (^ B (& C D))) E)")]
    [InlineData("A ^ B ^ C", "(^ (^ A B) C)")]
    [InlineData("A & B & C", "(& (& A B) C)")]
    [InlineData("(A | B) & (C ^ D)", "(& (| A B) (^ C D))")]
    [InlineData("(A)", "A")]
    public void BitExpressionsRetainPrecedenceAndLeftAssociativity(string source, string expected)
    {
        var value = Assert.IsType<CfdBitExpressionValue>(Parse(source));
        Assert.Equal(expected, Shape(value.Expression));
        Assert.Equal(new CfdSpan(1, 1, 1, source.Length + 1), value.Span);
    }

    [Fact]
    public void ExpressionNodesExcludeTrailingTriviaWhileTheValueIncludesIt()
    {
        var value = Assert.IsType<CfdBitExpressionValue>(Parse(" A # first\n | B # final\n "));
        Assert.Equal(new CfdSpan(1, 2, 3, 2), value.Span);
        Assert.Equal(new CfdSpan(1, 2, 2, 5), value.Expression.Span);
        var binary = Assert.IsType<CfdBitExpressionKind.Binary>(value.Expression.Kind);
        Assert.Equal(new CfdSpan(1, 2, 1, 3), binary.Left.Span);
        Assert.Equal(new CfdSpan(2, 4, 2, 5), binary.Right.Span);
    }

    [Theory]
    [InlineData("\"first\nbad\\q\"", "CFD-SYNTAX-006", "unsupported string escape `\\q`", 2, 6)]
    [InlineData("\"first\r\nsecond\\", "CFD-SYNTAX-006", "unterminated string", 2, 8)]
    [InlineData("Read |", "CFD-SYNTAX-007", "expected flag expression operand", 1, 7)]
    [InlineData("Read & )", "CFD-SYNTAX-007", "expected flag expression operand", 1, 8)]
    public void InvalidInputRetainsDiagnosticDetails(string source, string code, string message, int line, int column)
    {
        var error = Assert.Throws<CfdParseException>(() => Parse(source));
        var diagnostic = Assert.Single(error.Diagnostics);
        Assert.Equal(code, diagnostic.Code);
        Assert.Equal(message, diagnostic.Message);
        Assert.Equal("value.cfd", diagnostic.Path);
        Assert.Equal(new CfdSpan(line, column, line, column), diagnostic.Span);
    }

    [Theory]
    [InlineData(" | ")]
    [InlineData(" ^ ")]
    [InlineData(" & ")]
    public void BitExpressionDepthAcceptsTheBoundaryAndRejectsTheNextNode(string separator)
    {
        Assert.IsType<CfdBitExpressionValue>(Parse(string.Join(separator, Enumerable.Repeat("Read", 255))));
        var error = Assert.Throws<CfdParseException>(() => Parse(string.Join(separator, Enumerable.Repeat("Read", 256))));
        Assert.Equal("CFD-LIMIT", Assert.Single(error.Diagnostics).Code);
    }

    [Fact]
    public void ParenthesesShareTheValueNestingBudgetAtTheBoundary()
    {
        Assert.IsType<CfdBitExpressionValue>(Parse(new string('(', 255) + "Read" + new string(')', 255)));
        var error = Assert.Throws<CfdParseException>(() => Parse(new string('(', 256) + "Read" + new string(')', 256)));
        Assert.Equal("CFD-LIMIT", Assert.Single(error.Diagnostics).Code);
        error = Assert.Throws<CfdParseException>(() => Parse(new string('(', 254) + "Read | Write" + new string(')', 254)));
        Assert.Equal("CFD-LIMIT", Assert.Single(error.Diagnostics).Code);
    }

    private static CfdValueNode Parse(string source) => CfdParser.ParseValue(new CfdSource("value.cfd", source));

    private static string Shape(CfdBitExpression expression) => expression.Kind switch
    {
        CfdBitExpressionKind.Value value => value.Text,
        CfdBitExpressionKind.Binary binary => $"({Symbol(binary.Operator)} {Shape(binary.Left)} {Shape(binary.Right)})",
        _ => throw new InvalidOperationException(),
    };

    private static char Symbol(CfdBitOperator operation) => operation switch
    {
        CfdBitOperator.Or => '|',
        CfdBitOperator.Xor => '^',
        CfdBitOperator.And => '&',
        _ => throw new InvalidOperationException(),
    };
}
