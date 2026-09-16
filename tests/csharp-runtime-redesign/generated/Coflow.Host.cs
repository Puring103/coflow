#nullable enable
using System;
using Coflow;
namespace @Coflow.@Generated { public static class GeneratedHostBindings {
public static RuntimeBuilder BindHost(this RuntimeBuilder builder, global::@Coflow.@Generated.@IServices host) => builder.BindHost(new Adapter0(host));
private sealed class Adapter0 : HostBinding { private readonly global::@Coflow.@Generated.@IServices host; public Adapter0(global::@Coflow.@Generated.@IServices host) : base("Services") { this.host = host ?? throw new ArgumentNullException(nameof(host)); }
public override string MemberType(string field) { switch(field) {
case "environment": return "string";
case "adjust": return "fn(int) -> int";
case "notify": return "fn(int) -> ()";
default: throw new CoflowException("Unknown Host member."); } }
public override object? Read(string field) { switch(field) {
case "environment": return host.@environment;
default: throw new CoflowException("Host function members cannot be read as data."); } }
public override void Call(string field, HostCall call) { switch(field) {
case "adjust": call.Return(ValueCodecs.IntInvocation, host.@adjust(call.Argument(ValueCodecs.IntInvocation))); return;
case "notify": call.Return(ValueCodecs.UnitInvocation, host.@notify(call.Argument(ValueCodecs.IntInvocation))); return;
default: throw new CoflowException("Unknown Host function."); } } }
} }
namespace @Coflow.@Generated { public interface @IServices {
string @environment { get; }
int @adjust(int @arg0);
Unit @notify(int @arg0);
} }
