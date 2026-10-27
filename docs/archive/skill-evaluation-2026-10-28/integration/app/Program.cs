using Coflow.Loader;
using Game.Config;

try
{
    var dataDirectory = args.Length > 0 ? Path.GetFullPath(args[0]) : Path.Combine(AppContext.BaseDirectory, "data");
    // 一次提供全部基础与覆盖文本，逻辑路径用于加载诊断。
    var sources = Directory.EnumerateFiles(dataDirectory, "*.cfd", SearchOption.AllDirectories)
        .Select(path => new CfdSource(Path.GetRelativePath(dataDirectory, path), File.ReadAllText(path))).ToArray();
    var loaded = CoflowData.Load(sources);
    if (!loaded.Success)
    {
        foreach (var diagnostic in loaded.Diagnostics) Console.Error.WriteLine(diagnostic);
        return 1;
    }
    // 仅在成功后取得数据根；日语视图不会切换基础记录。
    var sword = loaded.Data.Table<Item>()["sword"];
    Console.WriteLine($"基础名称: {sword.name}");
    Console.WriteLine($"日语名称: {sword.For("language", "ja").name}");
    return 0;
}
catch (CfdLoadException exception)
{
    foreach (var diagnostic in exception.Diagnostics) Console.Error.WriteLine(diagnostic);
    return 1;
}
catch (IOException exception)
{
    Console.Error.WriteLine($"读取配置失败: {exception.Message}");
    return 1;
}
