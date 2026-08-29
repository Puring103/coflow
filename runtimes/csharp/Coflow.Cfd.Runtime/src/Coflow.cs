using CoflowRuntime;

public static class Coflow
{
    public static CoflowModule LoadData(string cfd, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(new[] { cfd }, contract, false);
    public static CoflowModule LoadData(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(cfdSources, contract, false);
    public static CoflowModule LoadAndCompile(string cfd, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(new[] { cfd }, contract, true);
    public static CoflowModule LoadAndCompile(string[] cfdSources, ICoflowGeneratedContract contract) =>
        CoflowModule.Load(cfdSources, contract, true);
    public static CoflowModuleSet Modules(params CoflowModule[] modules) => new(modules);
}
