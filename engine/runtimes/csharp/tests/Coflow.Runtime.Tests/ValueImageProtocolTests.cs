using System;
using System.IO;
using Coflow;
using Xunit;

public sealed class ValueImageProtocolTests
{
    private static byte[] Image(Action<BinaryWriter> nodes, uint count = 1)
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);
        writer.Write(0x49564643U); writer.Write(1U); writer.Write(count);
        nodes(writer); return stream.ToArray();
    }
    [Fact]
    public void EveryTruncatedPrefixFailsBeforePublishingANodeGraph()
    {
        var bytes = Image(writer => { writer.Write(1UL); writer.Write((byte)2); writer.Write(-7); });
        Assert.Equal(unchecked((uint)-7), ValueImage.Read(bytes).Get(1).Bits);
        for (int length = 0; length < bytes.Length; ++length) {
            var prefix = new byte[length]; Array.Copy(bytes, prefix, length);
            Assert.Throws<CoflowException>(() => ValueImage.Read(prefix));
        }
    }
    [Fact]
    public void InvalidLengthsEdgesIdentitiesAndPresenceAreRejected()
    {
        var cases = new[] {
            Image(writer => { writer.Write(1UL); writer.Write((byte)7); writer.Write(uint.MaxValue); }),
            Image(writer => { writer.Write(1UL); writer.Write((byte)7); writer.Write(1U); writer.Write(2UL); }),
            Image(writer => { writer.Write(0UL); writer.Write((byte)0); }),
            Image(writer => { writer.Write(1UL); writer.Write((byte)1); writer.Write((byte)2); }),
            Image(writer => { writer.Write(1UL); writer.Write((byte)0); writer.Write(1UL); writer.Write((byte)0); }, 2),
            Image(writer => { writer.Write(1UL); writer.Write((byte)11); writer.Write(1UL); writer.Write(1U); writer.Write(0U); writer.Write(1UL); writer.Write((byte)2); }),
        };
        foreach (var bytes in cases) Assert.Throws<CoflowException>(() => ValueImage.Read(bytes));
    }
    [Fact]
    public void BoundedByteMutationsNeverEscapeAsDecoderOrCollectionExceptions()
    {
        var original = Image(writer => { writer.Write(1UL); writer.Write((byte)2); writer.Write(123); });
        for (int index = 0; index < original.Length; ++index) {
            for (int bit = 0; bit < 8; ++bit) {
                var bytes = (byte[])original.Clone(); bytes[index] ^= (byte)(1 << bit);
                try { ValueImage.Read(bytes); }
                catch (CoflowException) { }
            }
        }
    }
}
