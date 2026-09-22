using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace Coflow
{
    internal readonly struct ValueKey : IEquatable<ValueKey>
    {
        internal readonly byte Kind; internal readonly uint Bits; internal readonly string Text;
        internal ValueKey(byte kind, uint bits = 0, string text = "") { Kind = kind; Bits = bits; Text = text; }
        public bool Equals(ValueKey other) => Kind == other.Kind && Bits == other.Bits && Text == other.Text;
        public override bool Equals(object? other) => other is ValueKey key && Equals(key);
        public override int GetHashCode() => unchecked((Kind * 397 ^ (int)Bits) * 397 ^ StringComparer.Ordinal.GetHashCode(Text));
    }
    // 节点与引用分两阶段解码；发布前验证所有边，读取不依赖原生实例存活。
    // 动态对象身份仅在返回图内缓存，图及其循环对象可被托管 GC 整体回收。
    internal sealed class MaterializationCache
    {
        internal readonly Dictionary<ulong, object> Values = new Dictionary<ulong, object>();
        internal readonly Dictionary<ulong, ulong[]> PendingAliases = new Dictionary<ulong, ulong[]>();
    }
    internal sealed class ValueImage
    {
        internal MaterializationCache Objects = new MaterializationCache();
        internal sealed class Node
        {
            internal byte Kind;
            internal byte[]? Identity;
            internal Dictionary<ValueKey, int>? Lookup;
            internal ValueKey ScalarKey => Kind == 1 || Kind == 2 ? new ValueKey(Kind, Bits)
                : Kind == 4 ? new ValueKey(Kind, text: Text) : Kind == 5 ? new ValueKey(Kind, Bits, Type)
                : throw new CoflowException("Invalid dictionary key.");
            internal Projection[]? Children;
            internal Dictionary<string, Projection>? Members;
            internal uint Bits;
            internal string Text = "";
            internal string Type = "";
            internal string? Key;
            internal ulong Default;
            internal ulong[] Items = Array.Empty<ulong>();
            // 标量节点共用只读空元数据；仅对象和维度解码时分配对应索引。
            private static readonly IReadOnlyDictionary<string, ulong> EmptyFields = new System.Collections.ObjectModel.ReadOnlyDictionary<string, ulong>(new Dictionary<string, ulong>());
            private static readonly IReadOnlyDictionary<string, bool> EmptyPresence = new System.Collections.ObjectModel.ReadOnlyDictionary<string, bool>(new Dictionary<string, bool>());
            internal IReadOnlyDictionary<string, ulong> Fields = EmptyFields;
            internal IReadOnlyDictionary<string, ulong> Bases = EmptyFields;
            internal IReadOnlyDictionary<string, bool> Explicit = EmptyPresence;
        }
        // 整个动态返回图共享一个拥有型根，子包装保留图即保留执行环境。
        internal bool NeedsLease()
        {
            foreach (var pair in nodes)
                if (pair.Value.Kind == 9 || pair.Value.Kind == 10) return true;
            return false;
        }
        private readonly Dictionary<ulong, Node> nodes = new Dictionary<ulong, Node>();
        internal bool Contains(ulong id) => nodes.ContainsKey(id);
        internal Node Get(ulong id) => nodes.TryGetValue(id, out var node) ? node : throw new CoflowException("Value is missing from the decoded image.");
        internal static ValueImage Read(byte[] bytes)
        {
            try
            {
                using var stream = new MemoryStream(bytes, false);
                using var reader = new BinaryReader(stream, new UTF8Encoding(false, true));
                if (reader.ReadUInt32() != 0x49564643 || reader.ReadUInt32() != 1) throw new CoflowException("Unsupported value image protocol.");
                var result = new ValueImage();
                int count = Count(reader, 9);
                for (int i = 0; i < count; ++i)
                {
                    ulong id = reader.ReadUInt64(); var node = new Node { Kind = reader.ReadByte() };
                    if (id == 0) throw new CoflowException("Invalid value identity.");
                    switch (node.Kind)
                    {
                        case 0: case 12: break;
                        case 13:
                            node.Type = Text(reader); byte referenceHasKey = reader.ReadByte();
                            if (referenceHasKey == 1) node.Key = Text(reader); else if (referenceHasKey != 0) throw new CoflowException("Invalid record reference marker.");
                            node.Bases = Fields(reader);
                            break;
                        case 1: node.Bits = reader.ReadByte(); if (node.Bits > 1) throw new CoflowException("Invalid boolean."); break;
                        case 2: case 3: node.Bits = reader.ReadUInt32(); break;
                        case 4: case 9: case 10: node.Text = Text(reader); break;
                        case 5: node.Type = Text(reader); node.Bits = reader.ReadUInt32(); break;
                        case 6:
                            node.Type = Text(reader); byte hasKey = reader.ReadByte();
                            if (hasKey == 1) node.Key = Text(reader); else if (hasKey != 0) throw new CoflowException("Invalid record marker.");
                            node.Fields = Fields(reader); node.Bases = Fields(reader); break;
                        case 7: node.Items = Ids(reader); break;
                        case 8:
                            int pairs = Count(reader, 16); node.Items = new ulong[checked(pairs * 2)];
                            for (int j = 0; j < node.Items.Length; ++j) node.Items[j] = reader.ReadUInt64(); break;
                        case 11:
                            node.Default = reader.ReadUInt64(); int variants = Count(reader, 13);
                            var fields = new Dictionary<string, ulong>(variants, StringComparer.Ordinal);
                            var presence = new Dictionary<string, bool>(StringComparer.Ordinal);
                            node.Fields = fields; node.Explicit = presence;
                            for (int j = 0; j < variants; ++j)
                            {
                                var name = Text(reader); fields.Add(name, reader.ReadUInt64());
                                byte present = reader.ReadByte(); if (present == 1) presence.Add(name, true); else if (present != 0) throw new CoflowException("Invalid presence bit.");
                            }
                            break;
                        default: throw new CoflowException("Invalid value kind.");
                    }
                    result.nodes.Add(id, node);
                }
                if (stream.Position != stream.Length) throw new CoflowException("Trailing value image bytes.");
                foreach (var node in result.nodes.Values)
                {
                    foreach (var id in node.Items) result.Get(id);
                    foreach (var id in node.Fields.Values) result.Get(id);
                    foreach (var id in node.Bases.Values) result.Get(id);
                    if (node.Kind == 11) result.Get(node.Default);
                    if (node.Kind == 8) {
                        node.Lookup = new Dictionary<ValueKey, int>();
                        for (int i = 0; i < node.Items.Length; i += 2) node.Lookup.Add(result.Get(node.Items[i]).ScalarKey, i / 2);
                    }
                }
                return result;
            }
            catch (Exception error) when (error is EndOfStreamException || error is OverflowException || error is ArgumentException)
            { throw new CoflowException("Malformed value image: " + error.Message); }
        }
        private static int Count(BinaryReader reader, int minimumBytes)
        {
            uint count = reader.ReadUInt32();
            if (count > int.MaxValue || (ulong)count * (uint)minimumBytes > (ulong)(reader.BaseStream.Length - reader.BaseStream.Position)) throw new CoflowException("Invalid value image length.");
            return (int)count;
        }
        private static string Text(BinaryReader reader)
        {
            int count = Count(reader, 1); var bytes = reader.ReadBytes(count);
            return new UTF8Encoding(false, true).GetString(bytes);
        }
        private static ulong[] Ids(BinaryReader reader)
        {
            var ids = new ulong[Count(reader, 8)];
            for (int i = 0; i < ids.Length; ++i) ids[i] = reader.ReadUInt64(); return ids;
        }
        private static IReadOnlyDictionary<string, ulong> Fields(BinaryReader reader)
        {
            int count = Count(reader, 12);
            var fields = new Dictionary<string, ulong>(count, StringComparer.Ordinal);
            for (int i = 0; i < count; ++i) fields.Add(Text(reader), reader.ReadUInt64());
            return fields;
        }
    }
}
