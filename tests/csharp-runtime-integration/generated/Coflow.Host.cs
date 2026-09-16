#nullable enable
using System;
using Coflow;
namespace @Game.@Config { public static class GeneratedHostBindings {
public static RuntimeBuilder BindHost(this RuntimeBuilder builder, global::@Game.@Config.@IHostServices host) => builder.BindHost(new Adapter0(host));
private sealed class Adapter0 : HostBinding { private readonly global::@Game.@Config.@IHostServices host; public Adapter0(global::@Game.@Config.@IHostServices host) : base("HostServices") { this.host = host ?? throw new ArgumentNullException(nameof(host)); }
public override string MemberType(string field) { switch(field) {
case "environment": return "string";
case "favorite": return "Character?";
case "mood": return "Mood";
case "log": return "fn(message: string) -> ()";
default: throw new CoflowException("Unknown Host member."); } }
public override object? Read(string field) { switch(field) {
case "environment": return host.@environment;
case "favorite": return host.@favorite;
case "mood": return new HostEnum("Mood", (uint)host.@mood);
default: throw new CoflowException("Host function members cannot be read as data."); } }
public override void Call(string field, HostCall call) { switch(field) {
case "log": call.Return(ValueCodecs.UnitInvocation, host.@log(call.Argument(ValueCodecs.StringInvocation))); return;
default: throw new CoflowException("Unknown Host function."); } } }
} }
namespace @Game.@Config { public interface @IHostServices {
string @environment { get; }
global::@Game.@Config.@Character? @favorite { get; }
global::@Game.@Config.@Mood @mood { get; }
Unit @log(string @message);
} }
