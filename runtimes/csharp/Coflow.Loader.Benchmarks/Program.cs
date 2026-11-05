using Coflow.Loader;

if (args.Contains("--case")) { Environment.ExitCode = Coflow.Benchmarks.Program.Run(args); return; }

if (args.Contains("--help"))
{
    Console.WriteLine("Coflow.Loader.Benchmarks [records=500] [samples=7] [count-descriptors] " +
        "[--suite wide,parser,variants,realistic,for,tables|all] [--records N] [--samples N] " +
        "[--warmups N] [--variant-counts 3,12] [--for-iterations 100] [--smoke]");
    return;
}

var options = BenchmarkOptions.Parse(args);
var runner = new BenchmarkRunner(options);
foreach (var suite in options.Suites)
{
    switch (suite)
    {
        case "wide":
            // 宽表覆盖字段数与源覆盖密度，加载时填充最终字段数组。
            foreach (var fields in new[] { 32, 128, 512 })
            foreach (var variants in new[] { 0, 3 })
                runner.LoadPhases(Fixtures.Wide(options.Records, fields, variants, false, options.CountDescriptors), suite);
            break;
        case "parser":
            foreach (var fields in new[] { 32, 128, 512 })
                runner.Parser(Fixtures.Wide(options.Records, fields, 0, false, options.CountDescriptors), suite);
            foreach (var dense in new[] { false, true })
                runner.Parser(Fixtures.Realistic(options.Records, 3, dense), suite);
            break;
        case "variants":
            foreach (var variants in options.VariantCounts)
            foreach (var dense in new[] { false, true })
                runner.LoadPhases(Fixtures.Wide(options.Records, 128, variants, dense, options.CountDescriptors), suite);
            break;
        case "realistic":
            runner.LoadPhases(Fixtures.Realistic(options.Records, 0, false), suite);
            foreach (var variants in options.VariantCounts)
            foreach (var dense in new[] { false, true })
                runner.LoadPhases(Fixtures.Realistic(options.Records, variants, dense), suite);
            break;
        case "for":
            foreach (var dense in new[] { false, true })
                runner.Views(Fixtures.Realistic(options.Records, 3, dense), suite);
            break;
        case "tables":
            foreach (var typeCount in new[] { 1, 16, 64 })
            {
                var fixture = Fixtures.ManyTables(options.Records, typeCount);
                runner.LoadPhases(fixture, suite);
                runner.TableRegistration(fixture, suite);
            }
            break;
    }
}

sealed record BenchmarkOptions(int Records, int Samples, int Warmups, int ForIterations,
    int[] VariantCounts, string[] Suites, bool CountDescriptors)
{
    internal static BenchmarkOptions Parse(string[] args)
    {
        var records = 500;
        var samples = 7;
        var warmups = 1;
        var forIterations = 100;
        var variants = new[] { 3, 12 };
        var suites = new[] { "wide" };
        var countDescriptors = false;
        var smoke = false;
        var positional = 0;
        for (var index = 0; index < args.Length; index++)
        {
            string Value() => ++index < args.Length ? args[index] : throw new ArgumentException("Missing option value.");
            switch (args[index])
            {
                case "--records": records = int.Parse(Value()); break;
                case "--samples": samples = int.Parse(Value()); break;
                case "--warmups": warmups = int.Parse(Value()); break;
                case "--for-iterations": forIterations = int.Parse(Value()); break;
                case "--variant-counts": variants = Value().Split(',').Select(int.Parse).ToArray(); break;
                case "--suite": suites = Value().Split(','); break;
                case "count-descriptors":
                case "--count-descriptors": countDescriptors = true; break;
                case "--smoke": smoke = true; break;
                default:
                    if (args[index].StartsWith('-') || positional > 1) throw new ArgumentException($"Unknown argument: {args[index]}");
                    if (positional++ == 0) records = int.Parse(args[index]);
                    else samples = int.Parse(args[index]);
                    break;
            }
        }
        var available = new[] { "wide", "parser", "variants", "realistic", "for", "tables" };
        if (suites.Contains("all")) suites = available;
        if (suites.Any(suite => !available.Contains(suite))) throw new ArgumentException("Unknown benchmark suite.");
        if (records < 1 || samples < 1 || warmups < 1 || forIterations < 1 || variants.Length == 0 || variants.Any(value => value < 1))
            throw new ArgumentException("Record, sample, warmup, iteration and variant counts must be positive.");
        // 冒烟只验证输入与断言，输出显式标记不足七次，不能混入正式性能报告。
        if (smoke) { records = Math.Min(records, 8); samples = 1; warmups = 1; forIterations = 3; }
        return new BenchmarkOptions(records, samples, warmups, forIterations, variants.Distinct().ToArray(), suites.Distinct().ToArray(), countDescriptors);
    }
}
