using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace Coflow
{
    public sealed class Diagnostic
    {
        public string Code { get; }
        public string? SourceName { get; }
        public string Message { get; }
        public ulong? StartOffset { get; }
        public ulong? EndOffset { get; }
        internal Diagnostic(string code, string source, string message, ulong? start, ulong? end)
        { Code = code; SourceName = source.Length == 0 ? null : source; Message = message; StartOffset = start; EndOffset = end; }
    }
    public sealed class BuildException : CoflowException
    {
        public IReadOnlyList<Diagnostic> Diagnostics { get; }
        private BuildException(Diagnostic[] diagnostics) : base(diagnostics.Length == 0 ? "Build failed." : diagnostics[0].Message)
        { Diagnostics = Array.AsReadOnly(diagnostics); }
        internal static BuildException Decode(byte[] bytes)
        {
            // 仅解码核心提供的诊断，不在 C# 重新分析语言或构建数据。
            using var stream = new MemoryStream(bytes, false);
            using var reader = new BinaryReader(stream, Encoding.UTF8);
            var diagnostics = new Diagnostic[checked((int)reader.ReadUInt32())];
            string Text() => Encoding.UTF8.GetString(reader.ReadBytes(checked((int)reader.ReadUInt32())));
            for (int i = 0; i < diagnostics.Length; ++i)
            {
                var code = Text(); var source = Text(); var message = Text();
                bool hasSpan = reader.ReadByte() != 0;
                ulong start = reader.ReadUInt64(), end = reader.ReadUInt64();
                diagnostics[i] = new Diagnostic(code, source, message, hasSpan ? start : (ulong?)null, hasSpan ? end : (ulong?)null);
            }
            return new BuildException(diagnostics);
        }
    }
}
