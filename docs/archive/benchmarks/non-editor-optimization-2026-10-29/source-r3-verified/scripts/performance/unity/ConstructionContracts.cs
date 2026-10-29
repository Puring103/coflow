#nullable enable
using System;
using System.Collections.Generic;
using System.Text;
using System.Threading;
using Coflow.Loader;

namespace Coflow.Benchmarks
{
    // 此组只执行语义断言，不进入正式测量。默认线程不指定栈大小，验证宿主的真实构造边界。
    internal static class ConstructionContracts
    {
        private sealed class Row : ICfdRecord
        {
            public CfdReader Reader { get; set; } = null!;
            internal Option<Row> Next;
            internal string Name = null!;
        }

        internal static int Run()
        {
            RunCases();
            Exception? failure = null;
            var workerId = 0;
            var worker = new Thread(() =>
            {
                workerId = Thread.CurrentThread.ManagedThreadId;
                try { RunCases(); }
                catch (Exception error) { failure = error; }
            });
            worker.Start();
            worker.Join();
            if (failure != null) throw new InvalidOperationException("Default-thread construction contract failed.", failure);
            return workerId;
        }

        private static void RunCases()
        {
            References(256, true, false);
            References(257, false, false);
            References(300, false, true);
            Templates(256, false);
            Templates(257, false);
            Templates(96, true);
            Collections(false);
            Collections(true);
            DiagnosticOrder();
        }

        private static CfdType<Row> ReferenceType(bool dimension)
        {
            var next = CfdValues.Optional(CfdValues.Reference<Row>("Row"));
            return new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
                new CfdField<Option<Row>>("next", dimension ? CfdValues.Dimension("links", next) : next, CfdDefaults.None()),
            }, _ => new Row(), (row, reader) => row.Next = reader.Get<Option<Row>>("next"), null);
        }

        private static void References(int count, bool cycle, bool dimension)
        {
            var text = new StringBuilder();
            if (dimension)
            {
                for (var i = 0; i < count; i++) text.Append('r').Append(i).Append(": Row {} ");
                text.Append("links[long] {");
            }
            for (var i = 0; i < count; i++)
                text.Append('r').Append(i).Append(": Row { next: ")
                    .Append(i + 1 < count ? "&Row::r" + (i + 1) : cycle ? "&Row::r0" : "None").Append(" } ");
            if (dimension) text.Append('}');
            var result = CfdLoader.Load(new[] { new CfdSource("chain.cfd", text.ToString()) },
                new CfdType[] { ReferenceType(dimension) }, data => data.Table<Row>());
            if (count > 256) { Diagnostic(result, "CFD-LIMIT"); return; }
            Program.Ensure(result.Success, "256 reference levels must succeed.");
            var start = result.Data["r0"];
            var cursor = start;
            for (var i = 0; i < count; i++) cursor = cursor.Next.Value;
            Program.Ensure(ReferenceEquals(start, cursor), "Deep reference cycle lost object identity.");
        }

        private static void Templates(int count, bool cycle)
        {
            var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] { new CfdField<string>("name", CfdValues.String) },
                _ => new Row(), (row, reader) => row.Name = reader.Get<string>("name"), null);
            var text = new StringBuilder();
            for (var i = 0; i < count; i++)
            {
                text.Append('r').Append(i).Append(": Row { name: ");
                if (i + 1 == count && !cycle) text.Append("\"end\"");
                else text.Append("f\"{&Row::r").Append((i + 1) % count).Append(".name}\"");
                text.Append(" } ");
            }
            var result = CfdLoader.Load(new[] { new CfdSource("templates.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
            if (cycle) Diagnostic(result, "CFD-INTERPOLATION-CYCLE");
            else if (count > 256) Diagnostic(result, "CFD-LIMIT");
            else
            {
                Program.Ensure(result.Success, "256 template levels must succeed.");
                foreach (var row in result.Data.Values) Program.Ensure(row.Name == "end", "Deep template value mismatch.");
            }
        }

        private sealed class Payload { internal Option<Row> Next; }
        private sealed class Container { internal object Items = null!; }

        private static void Collections(bool dictionary)
        {
            var constructed = 0;
            var payload = CfdValues.Object<Payload>("Payload");
            CfdField field = dictionary
                ? new CfdField<IReadOnlyDictionary<string, Payload>>("items", CfdValues.Dictionary(CfdValues.String, payload))
                : new CfdField<IReadOnlyList<IReadOnlyList<Payload>>>("items", CfdValues.List(CfdValues.List(payload)));
            var types = new CfdType[] {
                new CfdType<Payload>("Payload", CfdTypeKind.Data, new CfdField[] {
                    new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row")), CfdDefaults.None()),
                }, null, null, reader => { constructed++; return new Payload { Next = reader.Get<Option<Row>>("next") }; }),
                new CfdType<Container>("Container", CfdTypeKind.Table, new[] { field }, _ => new Container(),
                    (row, reader) => row.Items = dictionary ? reader.Get<IReadOnlyDictionary<string, Payload>>("items") : reader.Get<IReadOnlyList<IReadOnlyList<Payload>>>("items"), null),
                ReferenceType(false),
            };
            var text = new StringBuilder(dictionary
                ? "root: Container { items: { \"one\": {}, \"two\": {}, \"three\": {}, \"four\": { next: &Row::r0 } } } "
                : "root: Container { items: [[{}, {}], [{}, { next: &Row::r0 }]] } ");
            for (var i = 0; i < 96; i++) text.Append('r').Append(i).Append(": Row { next: ").Append(i == 95 ? "None" : "&Row::r" + (i + 1)).Append(" } ");
            var result = CfdLoader.Load(new[] { new CfdSource("collections.cfd", text.ToString()) }, types, data => data.Table<Container>());
            Program.Ensure(result.Success && constructed == 4, "Collection resumption repeated a completed construction.");
            var last = dictionary ? ((IReadOnlyDictionary<string, Payload>)result.Data["root"].Items)["four"]
                : ((IReadOnlyList<IReadOnlyList<Payload>>)result.Data["root"].Items)[1][1];
            var cursor = last.Next;
            var length = 0;
            while (cursor.IsSome) { length++; cursor = cursor.Value.Next; }
            Program.Ensure(length == 96, "Resumed collection reference chain is incomplete.");
        }

        private static void DiagnosticOrder()
        {
            var type = new CfdType<Row>("Row", CfdTypeKind.Table, new CfdField[] {
                new CfdField<Option<Row>>("next", CfdValues.Optional(CfdValues.Reference<Row>("Row"))),
                new CfdField<int>("price", CfdValues.Int),
            }, _ => new Row(), (row, reader) => row.Next = reader.Get<Option<Row>>("next"), null);
            var text = new StringBuilder();
            for (var i = 0; i < 96; i++) text.Append('r').Append(i).Append(": Row { next: ")
                .Append(i == 95 ? "&Row::absent" : "&Row::r" + (i + 1)).Append(", price: \"wrong\" } ");
            var result = CfdLoader.Load(new[] { new CfdSource("order.cfd", text.ToString()) }, new CfdType[] { type }, data => data.Table<Row>());
            Diagnostic(result, "CFD-REFERENCE");
            Program.Ensure(result.Diagnostics[0].Message.Contains("absent"), "DFS diagnostic target changed.");
        }

        private static void Diagnostic<T>(LoadResult<T> result, string code) where T : class
        {
            Program.Ensure(!result.Success && result.Diagnostics.Count == 1 && result.Diagnostics[0].Code == code,
                "Expected " + code + " construction diagnostic.");
        }
    }
}
