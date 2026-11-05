using System.Text;
using Coflow.Loader;

namespace Coflow.Benchmarks;

internal static class Fixture
{
    internal static CfdSource[] Create(Config config)
    {
        var result = new List<CfdSource>();
        for (var file = 0; file < config.Files; file++)
        {
            var source = new StringBuilder();
            var begin = (int)((long)file * config.Records / config.Files);
            var end = (int)((long)(file + 1) * config.Records / config.Files);
            for (var i = begin; i < end; i++)
            {
                var type = config.Case == "tables32" ? "Row" + i % 32 : "Row";
                source.Append("r").Append(i).Append(": ").Append(type).Append(" { ");
                if (!config.Complex)
                {
                    for (var field = 0; field < config.Fields; field++)
                        source.Append('f').Append(field).Append(": ").Append(i + field).Append(", ");
                }
                else
                {
                    source.Append("value: ").Append(i).Append(", name: \"Row ").Append(i).Append("\", ");
                    source.Append("items: [").Append(i).Append(',').Append(i + 1).Append(',').Append(i + 2).Append(',').Append(i + 3).Append("], ");
                    source.Append("lookup: {\"a\": ").Append(i).Append(", \"b\": ").Append(i + 1).Append("}, ");
                    source.Append("payload: Payload { amount: ").Append(i).Append(", text: \"Payload ").Append(i).Append("\" }, ");
                    var next = Next(config, i);
                    source.Append("next: ").Append(next < 0 ? "None" : "Some(&Row::r" + next + ")").Append(", ");
                    if (config.Templates) source.Append("label: f\"{self.name}:{self.value}:{self.payload.text}:{&Row::r0.name}\", ");
                    else source.Append("label: \"").Append(BaseLabel(i)).Append("\", ");
                    source.Append("note: ").Append(i % 2 == 0 ? "Some(\"note " + i + "\")" : "None").Append(", ");
                    for (var field = 8; field < 20; field++) source.Append('f').Append(field).Append(": ").Append(i + field).Append(", ");
                }
                source.AppendLine("}");
            }
            result.Add(new CfdSource($"data-{file:D4}.cfd", source.ToString()));
        }
        if (config.Case == "dimensions")
        {
            for (var variant = 0; variant < config.Variants; variant++)
            {
                var source = new StringBuilder();
                foreach (var dimension in new[] { "language", "balance" })
                {
                    source.Append(dimension).Append("[v").Append(variant).AppendLine("] {");
                    for (var i = 0; i < config.Records; i++)
                    {
                        if (!Overridden(config, i)) continue;
                        source.Append('r').Append(i).Append(": Row { ");
                        if (dimension == "language") source.Append("name: \"Localized ").Append(variant).Append(' ').Append(i).Append('"');
                        else source.Append("value: ").Append(1_000_000 + variant * 100_000 + i);
                        source.AppendLine(" }");
                    }
                    source.AppendLine("}");
                }
                result.Add(new CfdSource($"dimension-{variant:D4}.cfd", source.ToString()));
            }
        }
        return result.ToArray();
    }

    internal static bool Overridden(Config config, int index) => config.Density == "dense" || index % 100 == 0;
    internal static string BaseLabel(int index) => $"Row {index}:{index}:Payload {index}:Row 0";
    internal static string ViewLabel(Config config, int index, int? language, int? balance)
    {
        if (!config.Templates) return BaseLabel(index);
        var name = language.HasValue && Overridden(config, index) ? $"Localized {language} {index}" : $"Row {index}";
        var value = balance.HasValue && Overridden(config, index) ? 1_000_000 + balance.Value * 100_000 + index : index;
        return $"{name}:{value}:Payload {index}:{(language.HasValue ? $"Localized {language} 0" : "Row 0")}";
    }

    internal static int Next(Config config, int index) => config.Topology switch
    {
        "none" => -1,
        "pairs" => index % 2 == 0 ? Math.Min(index + 1, config.Records - 1) : index - 1,
        "hub" => index == 0 ? -1 : 0,
        // 保持健康路径的递归深度在Loader契约内，按64条组成多个独立环。
        "ring" => index + 1 < Math.Min((index / 64 + 1) * 64, config.Records) ? index + 1 : index / 64 * 64,
        _ => throw new ArgumentException("Unknown topology")
    };
}
