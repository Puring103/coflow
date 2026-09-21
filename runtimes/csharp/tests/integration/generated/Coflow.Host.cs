#nullable enable
using System;
using Coflow;

namespace Game.Config
{
public static class GeneratedHostBindings
{
    public static RuntimeBuilder BindHost(this RuntimeBuilder builder, global::Game.Config.IHostServices host)
        => builder.BindHost(new HostServicesBinding(host));

    private sealed class HostServicesBinding : HostBinding
    {
        private readonly global::Game.Config.IHostServices host;

        public HostServicesBinding(global::Game.Config.IHostServices host) : base("HostServices")
        {
            this.host = host ?? throw new ArgumentNullException(nameof(host));
        }

        public override object? Read(string field) => field switch
        {
            "environment" => host.environment,
            "favorite" => host.favorite,
            "mood" => new HostEnum("Mood", (uint)host.mood),
            _ => throw new CoflowException("Host function members cannot be read as data."),
        };

        public override void Call(string field, HostCall call)
        {
            switch (field)
            {
                case "log":
                    call.Return(ValueCodecs.UnitInvocation, host.log(call.Argument(ValueCodecs.StringInvocation)));
                    return;
                case "echoStats":
                    call.Return(ValueCodecs.CoflowInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)), host.echoStats(call.Argument(ValueCodecs.CoflowInvocation<global::Game.Config.Stats>(v1 => new global::Game.Config.Stats(v1)))));
                    return;
                case "echoProfile":
                    call.Return(ValueCodecs.CoflowInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()), host.echoProfile(call.Argument(ValueCodecs.CoflowInvocation<global::Game.Config.Profile>(v1 => v1.Resolve<global::Game.Config.Profile>()))));
                    return;
                case "echoCharacter":
                    call.Return(ValueCodecs.CoflowInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()), host.echoCharacter(call.Argument(ValueCodecs.CoflowInvocation<global::Game.Config.Character>(v1 => v1.Resolve<global::Game.Config.Character>()))));
                    return;
                default: throw new CoflowException("Unknown Host function.");
            }
        }
    }

    public static RuntimeBuilder BindHost(this RuntimeBuilder builder, global::Game.Config.IServices host)
        => builder.BindHost(new ServicesBinding(host));

    private sealed class ServicesBinding : HostBinding
    {
        private readonly global::Game.Config.IServices host;

        public ServicesBinding(global::Game.Config.IServices host) : base("Services")
        {
            this.host = host ?? throw new ArgumentNullException(nameof(host));
        }

        public override object? Read(string field) => field switch
        {
            "environment" => host.environment,
            _ => throw new CoflowException("Host function members cannot be read as data."),
        };

        public override void Call(string field, HostCall call)
        {
            switch (field)
            {
                case "adjust":
                    call.Return(ValueCodecs.IntInvocation, host.adjust(call.Argument(ValueCodecs.IntInvocation)));
                    return;
                case "notify":
                    call.Return(ValueCodecs.UnitInvocation, host.notify(call.Argument(ValueCodecs.IntInvocation)));
                    return;
                default: throw new CoflowException("Unknown Host function.");
            }
        }
    }

}
}
