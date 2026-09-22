using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace Coflow
{
    public sealed class CheckOptions
    {
        public static CheckOptions Default { get; } = new CheckOptions();
        public IReadOnlyList<CoflowObject>? Records { get; }
        public IReadOnlyList<string> Names { get; }
        public bool IncludeGlobal { get; }
        public ulong MaxWork { get; }
        public ulong MaxIterations { get; }
        public CheckOptions(IReadOnlyList<CoflowObject>? records = null, IReadOnlyList<string>? names = null,
            bool includeGlobal = true, ulong maxWork = 10_000_000, ulong maxIterations = 1_000_000)
        {
            Records = records;
            Names = names ?? Array.Empty<string>();
            IncludeGlobal = includeGlobal;
            MaxWork = maxWork;
            MaxIterations = maxIterations;
        }
    }
    public sealed class CheckDiagnostic
    {
        public string Code { get; }
        public string Message { get; }
        public string? SourceName { get; }
        public ulong? StartOffset { get; }
        public ulong? EndOffset { get; }
        public IReadOnlyList<string> CheckNames { get; }
        internal CheckDiagnostic(string code, string message, string? sourceName, ulong? start, ulong? end, string[] names)
        {
            Code = code; Message = message; SourceName = sourceName; StartOffset = start; EndOffset = end;
            CheckNames = Array.AsReadOnly(names);
        }
    }
    public sealed class CheckStatistics
    {
        public ulong RequestedTasks { get; }
        public ulong ExecutedTasks { get; }
        public ulong RejectedTasks { get; }
        public ulong WorkUsed { get; }
        public ulong DimensionProjectedRecords { get; }
        internal CheckStatistics(BinaryReader reader)
        {
            RequestedTasks = reader.ReadUInt64(); ExecutedTasks = reader.ReadUInt64();
            RejectedTasks = reader.ReadUInt64(); WorkUsed = reader.ReadUInt64();
            DimensionProjectedRecords = reader.ReadUInt64();
        }
    }
    public sealed class CheckResult
    {
        public bool Success { get; }
        public CheckStatistics Statistics { get; }
        public IReadOnlyList<CheckDiagnostic> Diagnostics { get; }
        private CheckResult(bool success, CheckStatistics statistics, CheckDiagnostic[] diagnostics)
        { Success = success; Statistics = statistics; Diagnostics = Array.AsReadOnly(diagnostics); }
        internal static CheckResult Run(Runtime runtime, CheckOptions options)
        {
            if (options.MaxWork == 0 || options.MaxIterations == 0) throw new ArgumentOutOfRangeException(nameof(options));
            byte[] request;
            using (var stream = new MemoryStream())
            using (var writer = new BinaryWriter(stream, Encoding.UTF8))
            {
                writer.Write(options.MaxWork); writer.Write(options.MaxIterations); writer.Write(options.IncludeGlobal);
                writer.Write(checked((uint)options.Names.Count));
                foreach (var name in options.Names) WriteText(writer, name ?? throw new ArgumentException("Check name cannot be null.", nameof(options)));
                if (options.Records == null) writer.Write(uint.MaxValue);
                else
                {
                    writer.Write(checked((uint)options.Records.Count));
                    foreach (var record in options.Records)
                    {
                        if (record == null) throw new ArgumentException("Check record cannot be null.", nameof(options));
                        var value = record.ArgumentProjection;
                        if (!ReferenceEquals(value.Owner, runtime)) throw new CoflowException("Check record belongs to a different Runtime.");
                        writer.Write(value.Id);
                    }
                }
                request = stream.ToArray();
            }
            var bytes = Native.ReadBuffer(runtime.Execute(NativeOperation.RunChecks, data: request));
            using var resultStream = new MemoryStream(bytes, false);
            using var reader = new BinaryReader(resultStream, Encoding.UTF8);
            bool success = reader.ReadByte() != 0;
            var statistics = new CheckStatistics(reader);
            var diagnostics = new CheckDiagnostic[checked((int)reader.ReadUInt32())];
            for (int i = 0; i < diagnostics.Length; ++i)
            {
                string code = ReadText(reader), message = ReadText(reader);
                string? source = null; ulong? start = null, end = null;
                if (reader.ReadByte() != 0) { source = ReadText(reader); start = reader.ReadUInt64(); end = reader.ReadUInt64(); }
                var names = new string[checked((int)reader.ReadUInt32())];
                for (int j = 0; j < names.Length; ++j) names[j] = ReadText(reader);
                diagnostics[i] = new CheckDiagnostic(code, message, source, start, end, names);
            }
            if (resultStream.Position != resultStream.Length) throw new CoflowException("Invalid check result payload.");
            return new CheckResult(success, statistics, diagnostics);
        }
        private static void WriteText(BinaryWriter writer, string value)
        {
            var bytes = Encoding.UTF8.GetBytes(value); writer.Write(checked((uint)bytes.Length)); writer.Write(bytes);
        }
        private static string ReadText(BinaryReader reader) => Encoding.UTF8.GetString(reader.ReadBytes(checked((int)reader.ReadUInt32())));
    }
}
