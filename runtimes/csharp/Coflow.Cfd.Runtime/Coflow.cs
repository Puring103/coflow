using CoflowRuntime;

public static class Coflow
{
    public static CoflowModule LoadData(string cfd) => CoflowLoader.LoadData(cfd);
    public static CoflowModule LoadData(string cfd, params CoflowModule[] children) =>
        CoflowLoader.LoadData(new[] { cfd }, children);
    public static CoflowModule LoadData(string[] cfdSources) => CoflowLoader.LoadData(cfdSources);
    public static CoflowModule LoadData(string[] cfdSources, params CoflowModule[] children) =>
        CoflowLoader.LoadData(cfdSources, children);
    public static CoflowModule LoadAndCompile(string cfd) => CoflowLoader.LoadAndCompile(cfd);
    public static CoflowModule LoadAndCompile(string cfd, params CoflowModule[] children) =>
        CoflowLoader.LoadAndCompile(new[] { cfd }, children);
    public static CoflowModule LoadAndCompile(string[] cfdSources) => CoflowLoader.LoadAndCompile(cfdSources);
    public static CoflowModule LoadAndCompile(string[] cfdSources, params CoflowModule[] children) =>
        CoflowLoader.LoadAndCompile(cfdSources, children);
    public static CoflowModule Combine(params CoflowModule[] modules) => CoflowModule.Combine(modules);

    internal static CoflowModule LoadData(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowLoader.LoadData(cfdSources, contract);

    internal static CoflowModule LoadAndCompile(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowLoader.LoadAndCompile(cfdSources, contract);
}
