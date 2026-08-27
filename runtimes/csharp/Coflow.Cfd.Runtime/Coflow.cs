using CoflowRuntime;

public static class Coflow
{
    public static CoflowData LoadData(string cfd) => CoflowLoader.LoadData(cfd);
    public static CoflowData LoadData(string[] cfdSources) => CoflowLoader.LoadData(cfdSources);
    public static CoflowData LoadAndCompile(string cfd) => CoflowLoader.LoadAndCompile(cfd);
    public static CoflowData LoadAndCompile(string[] cfdSources) => CoflowLoader.LoadAndCompile(cfdSources);

    internal static CoflowData LoadData(string[] cfdSources, ICoflowGeneratedModule module) =>
        CoflowLoader.LoadData(cfdSources, module);

    internal static CoflowData LoadAndCompile(string[] cfdSources, ICoflowGeneratedModule module) =>
        CoflowLoader.LoadAndCompile(cfdSources, module);
}
