using CoflowRuntime;

public static class Coflow
{
    public static CoflowModule LoadData(string cfd) => CoflowLoader.LoadData(new[] { cfd });
    public static CoflowModule LoadData(string[] cfdSources) => CoflowLoader.LoadData(cfdSources);
    public static CoflowModule LoadAndCompile(string cfd) => CoflowLoader.LoadAndCompile(new[] { cfd });
    public static CoflowModule LoadAndCompile(string[] cfdSources) => CoflowLoader.LoadAndCompile(cfdSources);
    public static CoflowModuleSet Modules(params CoflowModule[] modules) => new(modules);

    internal static CoflowModule LoadData(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowLoader.LoadData(cfdSources, contract);

    internal static CoflowModule LoadAndCompile(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowLoader.LoadAndCompile(cfdSources, contract);
}
