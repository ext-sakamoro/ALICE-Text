// ALICE-Text Unity C# Bindings
// 20 FFI functions for exception-based text compression
//
// Author: Moroya Sakamoto

using System;
using System.Runtime.InteropServices;
using System.Text;

namespace AliceText
{
    // ========================================================================
    // C-compatible structs
    // ========================================================================

    [StructLayout(LayoutKind.Sequential)]
    public struct AliceTextCompressedData
    {
        public IntPtr data;
        public uint len;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct AliceTextStats
    {
        public ulong originalSize;
        public ulong compressedSize;
        public double compressionRatio;
        public double spaceSavings;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct AliceTextEntropy
    {
        public double shannonEntropy;
        public double estimatedRatio;
        public ulong estimatedSize;
        public ulong originalSize;
        public double spaceSavings;
        public double patternCoverage;
        public uint uniqueBytes;
        public double repetitionScore;
        public byte isCompressible;
    }

    // ========================================================================
    // Native methods
    // ========================================================================

    internal static class Native
    {
        const string DLL = "alice_text";

        // Lifecycle
        [DllImport(DLL)] public static extern IntPtr alice_text_create();
        [DllImport(DLL)] public static extern void alice_text_destroy(IntPtr handle);

        // Compress / Decompress
        [DllImport(DLL)] public static extern AliceTextCompressedData alice_text_compress(IntPtr handle, byte[] text);
        [DllImport(DLL)] public static extern IntPtr alice_text_decompress(IntPtr handle, IntPtr data, uint len);
        [DllImport(DLL)] public static extern AliceTextCompressedData alice_text_compress_tuned(byte[] text, byte mode);
        [DllImport(DLL)] public static extern IntPtr alice_text_decompress_tuned(IntPtr data, uint len);

        // Stats / Entropy
        [DllImport(DLL)] public static extern byte alice_text_get_stats(IntPtr handle, ref AliceTextStats stats);
        [DllImport(DLL)] public static extern byte alice_text_estimate_entropy(byte[] text, ref AliceTextEntropy entropy);

        // Dialogue
        [DllImport(DLL)] public static extern IntPtr alice_text_dialogue_create();
        [DllImport(DLL)] public static extern void alice_text_dialogue_destroy(IntPtr handle);
        [DllImport(DLL)] public static extern byte alice_text_dialogue_add(IntPtr handle, uint id, byte[] speaker, byte[] text);
        [DllImport(DLL)] public static extern IntPtr alice_text_dialogue_get(IntPtr handle, uint id);
        [DllImport(DLL)] public static extern uint alice_text_dialogue_count(IntPtr handle);
        [DllImport(DLL)] public static extern uint alice_text_dialogue_unique_chars(IntPtr handle);

        // Memory
        [DllImport(DLL)] public static extern void alice_text_data_free(IntPtr data, uint len);
        [DllImport(DLL)] public static extern void alice_text_string_free(IntPtr s);

        // Version
        [DllImport(DLL)] public static extern IntPtr alice_text_version();
    }

    // ========================================================================
    // Compression mode
    // ========================================================================

    public enum CompressionMode : byte
    {
        Fast = 0,
        Balanced = 1,
        Best = 2
    }

    // ========================================================================
    // Helper
    // ========================================================================

    internal static class Util
    {
        public static byte[] ToNullTerminated(string s)
        {
            var bytes = Encoding.UTF8.GetBytes(s);
            var result = new byte[bytes.Length + 1];
            Array.Copy(bytes, result, bytes.Length);
            return result;
        }

        public static string PtrToString(IntPtr ptr)
        {
            if (ptr == IntPtr.Zero) return null;
            int len = 0;
            while (Marshal.ReadByte(ptr, len) != 0) len++;
            var buf = new byte[len];
            Marshal.Copy(ptr, buf, 0, len);
            return Encoding.UTF8.GetString(buf);
        }
    }

    // ========================================================================
    // ALICEText compressor
    // ========================================================================

    public class Compressor : IDisposable
    {
        private IntPtr _handle;
        private bool _disposed;

        public Compressor()
        {
            _handle = Native.alice_text_create();
        }

        public byte[] Compress(string text)
        {
            var data = Native.alice_text_compress(_handle, Util.ToNullTerminated(text));
            if (data.data == IntPtr.Zero) return null;
            var result = new byte[data.len];
            Marshal.Copy(data.data, result, 0, (int)data.len);
            Native.alice_text_data_free(data.data, data.len);
            return result;
        }

        public string Decompress(byte[] compressed)
        {
            unsafe
            {
                fixed (byte* ptr = compressed)
                {
                    var result = Native.alice_text_decompress(_handle, (IntPtr)ptr, (uint)compressed.Length);
                    if (result == IntPtr.Zero) return null;
                    var str = Util.PtrToString(result);
                    Native.alice_text_string_free(result);
                    return str;
                }
            }
        }

        public AliceTextStats? GetStats()
        {
            var stats = new AliceTextStats();
            if (Native.alice_text_get_stats(_handle, ref stats) != 0)
                return stats;
            return null;
        }

        public void Dispose()
        {
            if (!_disposed && _handle != IntPtr.Zero)
            {
                Native.alice_text_destroy(_handle);
                _handle = IntPtr.Zero;
                _disposed = true;
            }
            GC.SuppressFinalize(this);
        }

        ~Compressor() { Dispose(); }
    }

    // ========================================================================
    // Static tuned compression
    // ========================================================================

    public static class TunedCompressor
    {
        public static byte[] Compress(string text, CompressionMode mode = CompressionMode.Balanced)
        {
            var data = Native.alice_text_compress_tuned(Util.ToNullTerminated(text), (byte)mode);
            if (data.data == IntPtr.Zero) return null;
            var result = new byte[data.len];
            Marshal.Copy(data.data, result, 0, (int)data.len);
            Native.alice_text_data_free(data.data, data.len);
            return result;
        }

        public static string Decompress(byte[] compressed)
        {
            unsafe
            {
                fixed (byte* ptr = compressed)
                {
                    var result = Native.alice_text_decompress_tuned((IntPtr)ptr, (uint)compressed.Length);
                    if (result == IntPtr.Zero) return null;
                    var str = Util.PtrToString(result);
                    Native.alice_text_string_free(result);
                    return str;
                }
            }
        }
    }

    // ========================================================================
    // Entropy estimator
    // ========================================================================

    public static class Entropy
    {
        public static AliceTextEntropy? Estimate(string text)
        {
            var entropy = new AliceTextEntropy();
            if (Native.alice_text_estimate_entropy(Util.ToNullTerminated(text), ref entropy) != 0)
                return entropy;
            return null;
        }
    }

    // ========================================================================
    // Dialogue table
    // ========================================================================

    public class DialogueTable : IDisposable
    {
        private IntPtr _handle;
        private bool _disposed;

        public DialogueTable()
        {
            _handle = Native.alice_text_dialogue_create();
        }

        public bool Add(uint id, string speaker, string text)
        {
            return Native.alice_text_dialogue_add(
                _handle, id,
                Util.ToNullTerminated(speaker),
                Util.ToNullTerminated(text)) != 0;
        }

        public string Get(uint id)
        {
            var ptr = Native.alice_text_dialogue_get(_handle, id);
            if (ptr == IntPtr.Zero) return null;
            var str = Util.PtrToString(ptr);
            Native.alice_text_string_free(ptr);
            return str;
        }

        public uint Count => Native.alice_text_dialogue_count(_handle);
        public uint UniqueChars => Native.alice_text_dialogue_unique_chars(_handle);

        public void Dispose()
        {
            if (!_disposed && _handle != IntPtr.Zero)
            {
                Native.alice_text_dialogue_destroy(_handle);
                _handle = IntPtr.Zero;
                _disposed = true;
            }
            GC.SuppressFinalize(this);
        }

        ~DialogueTable() { Dispose(); }
    }

    // ========================================================================
    // Version
    // ========================================================================

    public static class Version
    {
        public static string Get()
        {
            var ptr = Native.alice_text_version();
            return Util.PtrToString(ptr);
        }
    }
}
