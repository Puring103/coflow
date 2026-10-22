using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Loader
{

using System.Globalization;

internal static class CfdIdentifiers
{
    internal static bool IsReserved(string value) => value is
        "_" or "const" or "enum" or "type" or "table" or "data" or "singleton" or
        "abstract" or "sealed" or "check" or "when" or "all" or "any" or "none" or
        "in" or "is" or "true" or "false" or "None" or "Some";

    internal static bool IsIdentifier(string value) =>
        IsIdentifierName(value) && !IsReserved(value);

    internal static bool IsIdentifierName(string value)
    {
        if (string.IsNullOrEmpty(value)) return false;
        var index = 0;
        if (!ReadCodePoint(value, ref index, start: true)) return false;
        while (index < value.Length)
            if (!ReadCodePoint(value, ref index, start: false)) return false;
        return true;
    }

    internal static bool TryRead(string source, ref int index)
    {
        var current = index;
        if (current >= source.Length || !ReadCodePoint(source, ref current, start: true)) return false;
        while (current < source.Length)
        {
            var next = current;
            if (!ReadCodePoint(source, ref next, start: false)) break;
            current = next;
        }
        index = current;
        return true;
    }

    internal static bool ReadCodePoint(string value, ref int index, bool start)
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
        var identifierStart = category is
            UnicodeCategory.UppercaseLetter or
            UnicodeCategory.LowercaseLetter or
            UnicodeCategory.TitlecaseLetter or
            UnicodeCategory.ModifierLetter or
            UnicodeCategory.OtherLetter or
            UnicodeCategory.LetterNumber;
        var valid = start
            ? codePoint == '_' || identifierStart
            : codePoint == '_' || identifierStart || category is
                UnicodeCategory.NonSpacingMark or
                UnicodeCategory.SpacingCombiningMark or
                UnicodeCategory.DecimalDigitNumber or
                UnicodeCategory.ConnectorPunctuation or
                UnicodeCategory.Format;
        index += width;
        return valid;
    }
}
}
