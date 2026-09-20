using System;
using System.IO;
using System.Threading;
using Coflow;
using Game.Config;
using UnityEngine;

public sealed class CoflowSmoke : MonoBehaviour
{
    private sealed class Service : IHostServices
    {
        public Character Hero;
        public int Calls;
        public string environment { get { Calls++; return "Unity"; } }
        public Character favorite => Hero;
        public Mood mood => Mood.Happy;
        public Unit log(string message) { if (Hero.score(2) != 12) throw new Exception("Host reentry"); Calls++; return new Unit(); }
    }
    private static void Require(bool value, string message) { if (!value) throw new Exception(message); }
    private void Start()
    {
        try { Run(); Debug.Log("coflow-unity-smoke-ok"); Application.Quit(0); }
        catch (Exception error) { Debug.LogException(error); Application.Quit(1); }
    }
    internal static void Run()
    {
        using var contract = Generated.LoadContract(Resources.Load<TextAsset>("coflow").bytes);
        var host = new Service();
        using var builder = new RuntimeBuilder(contract).BindHost(host).AddSource("hero: Hero { name: \"Hero\", stats: Stats { health: 10, weights: [1, 2] }, friend: &hero } RuntimeSettings: RuntimeSettings {}");
        using var runtime = builder.Build();
        Require(host.Calls == 0, "Projection invoked Host");
        var hero = runtime.Table<Character>()["hero"]; host.Hero = hero;
        Require(ReferenceEquals(hero, hero.friend) && hero.score(2) == 12, "Record identity or VM");
        Require(hero.Rendertext() == "Hero", "Template");
        var services = runtime.Singleton<HostServices>();
        Require(services.environment == "Unity" && services.favorite == hero && services.mood == Mood.Happy, "Host data");
        services.log("smoke"); Require(host.Calls == 2, "Host call or reentry");
        var source = new float[] { 3, 4 };
        var data = new Stats(20, new RuntimeArray<float>(source), null); source[0] = 99;
        Require(hero.roundtrip(data).weights[0] == 3, "Immutable typed import");
        var closure = hero.closure(5);
        Require(hero.callbacks(new RuntimeArray<RuntimeFunction<int>>(new[] { closure }))[0].Invoke() == 15, "Returned closure graph");
        GC.Collect(); GC.WaitForPendingFinalizers(); Require(closure.Invoke() == 15, "Lease lifetime");
        Exception failure = null;
        var thread = new Thread(() => { try { Require(hero.stats.health == 10, "Cross-thread snapshot"); try { hero.score(0); throw new Exception("Cross-thread execution accepted"); } catch (CoflowException) {} } catch (Exception error) { failure = error; } });
        thread.Start(); thread.Join(); if (failure != null) throw failure;
        runtime.Dispose(); Require(hero.stats.health == 10 && hero.name == "Hero", "Disposed snapshot");
        try { closure.Invoke(); throw new Exception("Disposed execution accepted"); } catch (ObjectDisposedException) {}
    }
}
