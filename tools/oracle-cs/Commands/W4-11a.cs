using System.Reflection;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.Marshalling;
using System.Text.Json.Nodes;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-11a
//     "wic-interop-constants" => Commands.WicInteropConstants(inputs),
//     "wic-hresult-exception" => Commands.WicHresultException(inputs),
//     "wic-managed-istream" => Commands.WicManagedIStream(inputs),
// Wiring needs (the wiring step, not this draft): OracleCs.csproj gets a <Reference Include="PhotoReview.Imaging"> to the
// baseline PhotoReview.Imaging.dll (same as W4-02a). Everything in WicInterop.cs is `internal`: reached by reflection
// (Assembly.GetType), so no InternalsVisibleTo is needed. No WIC object is created: the three commands need only the
// .NET runtime (Marshal, ComWrappers), so the goldens are machine-independent.
// The IStream calls go through the REAL COM vtable of the ManagedIStream CCW ([GeneratedComClass] via
// StrategyBasedComWrappers), so the golden records the HRESULT WIC would see, not a managed exception.
// If GetDelegateForFunctionPointer refuses the CCW entry points on the wiring machine, switch W411aCall to
// `delegate* unmanaged<...>` and add <AllowUnsafeBlocks>true</AllowUnsafeBlocks> (same results expected).

// W4-11a: WicInterop constants, HRESULT -> .NET exception type, ManagedIStream through its COM vtable.
static partial class Commands
{
    private static readonly Assembly W411aImaging = Assembly.Load("PhotoReview.Imaging");

    private static Type W411aType(string name) =>
        W411aImaging.GetType("PhotoReview.Imaging.Decoding.Wic." + name, throwOnError: true)!;

    private static string W411aHex(int value) => "0x" + ((uint)value).ToString("X8");

    private static int W411aParseHr(string text) =>
        unchecked((int)Convert.ToUInt32(text.StartsWith("0x", StringComparison.OrdinalIgnoreCase) ? text[2..] : text, 16));

    // ---- wic-interop-constants ----

    /// <summary>Case {name, section: guids|enums|interfaces|statstg|sdk} -> the C# declarations of that section.</summary>
    public static JsonArray WicInteropConstants(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var section = c["section"]!.GetValue<string>();
            var o = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["section"] = section };
            switch (section)
            {
                case "guids":
                {
                    var guids = new JsonArray();
                    foreach (var f in W411aType("WicGuids").GetFields(BindingFlags.Public | BindingFlags.Static).OrderBy(f => f.MetadataToken))
                        guids.Add(new JsonObject { ["field"] = f.Name, ["value"] = ((Guid)f.GetValue(null)!).ToString("D") });
                    o["guids"] = guids;
                    break;
                }
                case "enums":
                {
                    var enums = new JsonArray();
                    foreach (var name in c["enums"]!.AsArray().Select(n => n!.GetValue<string>()))
                    {
                        var t = W411aType(name);
                        var members = new JsonArray();
                        foreach (var f in t.GetFields(BindingFlags.Public | BindingFlags.Static).OrderBy(f => f.MetadataToken))
                            members.Add(new JsonObject { ["member"] = f.Name, ["value"] = Convert.ToUInt32(f.GetRawConstantValue()) });
                        enums.Add(new JsonObject
                        {
                            ["enum"] = name,
                            ["flags"] = t.IsDefined(typeof(FlagsAttribute), false),
                            ["members"] = members,
                        });
                    }
                    o["enums"] = enums;
                    break;
                }
                case "interfaces":
                {
                    var list = new JsonArray();
                    foreach (var name in c["interfaces"]!.AsArray().Select(n => n!.GetValue<string>()))
                    {
                        var t = W411aType(name);
                        var methods = t.GetMethods(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly)
                            .OrderBy(m => m.MetadataToken)
                            .Select(m => (JsonNode?)new JsonObject
                            {
                                ["method"] = m.Name,
                                ["preserve_sig"] = m.IsDefined(typeof(PreserveSigAttribute), false),
                            })
                            .ToArray();
                        list.Add(new JsonObject
                        {
                            ["interface"] = name,
                            ["iid"] = t.GUID.ToString("D"),
                            ["bases"] = new JsonArray(t.GetInterfaces().Select(i => (JsonNode?)i.Name).OrderBy(n => n!.GetValue<string>(), StringComparer.Ordinal).ToArray()),
                            ["first_slot"] = 3 + W411aInheritedSlots(t),
                            ["methods"] = new JsonArray(methods),
                        });
                    }
                    o["interfaces"] = list;
                    break;
                }
                case "statstg":
                {
                    var t = W411aType("StatStgNative");
                    var offsets = new JsonArray();
                    foreach (var f in t.GetFields(BindingFlags.Public | BindingFlags.Instance).OrderBy(f => f.MetadataToken))
                        offsets.Add(new JsonObject { ["field"] = f.Name, ["offset"] = (long)Marshal.OffsetOf(t, f.Name) });
                    o["pointer_size"] = IntPtr.Size;
                    o["size"] = Marshal.SizeOf(t);
                    o["fields"] = offsets;
                    break;
                }
                case "sdk":
                {
                    var native = W411aType("WicNativeMethods");
                    o["WINCODEC_SDK_VERSION1"] = Convert.ToUInt32(native.GetField("WINCODEC_SDK_VERSION1")!.GetRawConstantValue());
                    var import = native.GetMethod("WICCreateImagingFactory_Proxy", BindingFlags.Public | BindingFlags.Static)!;
                    o["factory_entry_point"] = import.Name;
                    break;
                }
                default:
                    throw new InvalidOperationException("unknown section " + section);
            }
            result.Add(o);
        }
        return result;
    }

    /// <summary>Slots taken by the declared methods of the base interfaces (IUnknown excluded).</summary>
    private static int W411aInheritedSlots(Type t) =>
        t.GetInterfaces().Sum(i => i.GetMethods(BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly).Length);

    // ---- wic-hresult-exception ----

    /// <summary>Case {name, hresults:[hex]} -> per HRESULT the type Marshal.GetExceptionForHR(hr, -1) creates, its HResult and
    /// its base types (null for a success code).</summary>
    public static JsonArray WicHresultException(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var rows = new JsonArray();
            foreach (var text in c["hresults"]!.AsArray().Select(n => n!.GetValue<string>()))
            {
                var hr = W411aParseHr(text);
                var ex = Marshal.GetExceptionForHR(hr, new IntPtr(-1));
                rows.Add(new JsonObject
                {
                    ["hresult"] = W411aHex(hr),
                    ["type"] = ex?.GetType().FullName,
                    ["exception_hresult"] = ex is null ? null : W411aHex(ex.HResult),
                    ["base_types"] = ex is null ? null : new JsonArray(W411aBases(ex.GetType()).Select(n => (JsonNode?)n).ToArray()),
                });
            }
            result.Add(new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["results"] = rows });
        }
        return result;
    }

    /// <summary>Ancestors of an exception type below System.Exception (what `catch (X)` / `is X` tests see).</summary>
    private static IEnumerable<string> W411aBases(Type t)
    {
        for (var b = t.BaseType; b is not null && b != typeof(object); b = b.BaseType)
            yield return b.FullName!;
    }

    // ---- wic-managed-istream ----

    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aReadFn(nint self, nint pv, int cb, nint pcb);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aSeekFn(nint self, long move, int origin, nint pnew);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aSetSizeFn(nint self, long size);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aCopyToFn(nint self, nint pstm, long cb, nint pread, nint pwritten);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aIntFn(nint self, int value);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aVoidFn(nint self);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aRegionFn(nint self, long offset, long cb, int type);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aStatFn(nint self, nint pstat, int flag);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)] private delegate int W411aCloneFn(nint self, nint ppstm);

    private static T W411aSlot<T>(nint itf, int slot) where T : Delegate =>
        Marshal.GetDelegateForFunctionPointer<T>(Marshal.ReadIntPtr(Marshal.ReadIntPtr(itf), slot * IntPtr.Size));

    private static readonly Guid W411aIidIStream = new("0000000c-0000-0000-C000-000000000046");

    /// <summary>
    /// Case {name, backend: {kind: growable|readonly, data:[byte], position}, ops:[op]} -> per op {op, hr, ...outputs,
    /// position, length} where position/length are read from the backing MemoryStream after the op.
    /// op = {op: read, cb (may be -1), null_out} | {op: write, data:[byte], cb?, null_out} | {op: seek, move, origin, null_out}
    ///    | {op: set_size, size} | {op: commit} | {op: revert} | {op: lock} | {op: unlock} | {op: stat, flag}
    ///    | {op: clone} | {op: copy_to} | {op: dispose}.
    /// "growable" = new MemoryStream() filled with data (Rust: Cursor<Vec<u8>>), "readonly" = new MemoryStream(byte[], false)
    /// (Rust: Cursor<Arc<[u8]>>). The C# tests' `new MemoryStream(byte[])` reads exactly like "readonly".
    /// </summary>
    public static JsonArray WicManagedIStream(JsonObject inputs)
    {
        var wrappers = new StrategyBasedComWrappers();
        var managedType = W411aType("ManagedIStream");
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var backend = c["backend"]!.AsObject();
            var data = backend["data"]?.AsArray().Select(n => (byte)n!.GetValue<int>()).ToArray() ?? [];
            var stream = backend["kind"]!.GetValue<string>() switch
            {
                "growable" => W411aGrowable(data),
                "readonly" => new MemoryStream(data, writable: false),
                var k => throw new InvalidOperationException("unknown backend " + k),
            };
            stream.Position = backend["position"]?.GetValue<long>() ?? 0;
            var managed = Activator.CreateInstance(managedType, stream)!;
            var unknown = wrappers.GetOrCreateComInterfaceForObject(managed, CreateComInterfaceFlags.None);
            Marshal.ThrowExceptionForHR(Marshal.QueryInterface(unknown, in W411aIidIStream, out var itf));
            var outputs = new JsonArray();
            try
            {
                foreach (var opNode in c["ops"]!.AsArray())
                    outputs.Add(W411aRunOp(itf, managed, stream, opNode!.AsObject()));
            }
            finally
            {
                Marshal.Release(itf);
                Marshal.Release(unknown);
            }
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["backend"] = backend.DeepClone(),
                ["ops"] = c["ops"]!.DeepClone(),
                ["results"] = outputs,
            });
        }
        return result;
    }

    private static MemoryStream W411aGrowable(byte[] data)
    {
        var s = new MemoryStream();
        s.Write(data);
        return s;
    }

    private static JsonObject W411aRunOp(nint itf, object managed, MemoryStream stream, JsonObject op)
    {
        var name = op["op"]!.GetValue<string>();
        var nullOut = op["null_out"]?.GetValue<bool>() ?? false;
        var o = new JsonObject { ["op"] = name };
        var outBuf = Marshal.AllocHGlobal(16);
        try
        {
            Marshal.WriteInt64(outBuf, unchecked((long)0xCDCDCDCDCDCDCDCDUL)); // sentinel: shows whether the out value was written
            Marshal.WriteInt64(outBuf, 8, unchecked((long)0xCDCDCDCDCDCDCDCDUL));
            var pOut = nullOut ? 0 : outBuf;
            int hr;
            switch (name)
            {
                case "read":
                {
                    var cb = op["cb"]!.GetValue<int>();
                    var buffer = Marshal.AllocHGlobal(Math.Max(cb, 1));
                    try
                    {
                        for (var i = 0; i < cb; i++) Marshal.WriteByte(buffer, i, 0xEE);
                        hr = W411aSlot<W411aReadFn>(itf, 3)(itf, buffer, cb, pOut);
                        var bytes = new byte[Math.Max(cb, 0)]; // cb = -1: the CCW must refuse before touching the buffer
                        Marshal.Copy(buffer, bytes, 0, bytes.Length);
                        o["buffer"] = new JsonArray(bytes.Select(b => (JsonNode?)(int)b).ToArray());
                    }
                    finally { Marshal.FreeHGlobal(buffer); }
                    if (!nullOut) o["out_u32"] = (uint)Marshal.ReadInt32(outBuf);
                    break;
                }
                case "write":
                {
                    var bytes = op["data"]!.AsArray().Select(n => (byte)n!.GetValue<int>()).ToArray();
                    var cb = op["cb"]?.GetValue<int>() ?? bytes.Length;
                    var buffer = Marshal.AllocHGlobal(Math.Max(bytes.Length, 1));
                    try
                    {
                        Marshal.Copy(bytes, 0, buffer, bytes.Length);
                        hr = W411aSlot<W411aReadFn>(itf, 4)(itf, buffer, cb, pOut);
                    }
                    finally { Marshal.FreeHGlobal(buffer); }
                    if (!nullOut) o["out_u32"] = (uint)Marshal.ReadInt32(outBuf);
                    break;
                }
                case "seek":
                    hr = W411aSlot<W411aSeekFn>(itf, 5)(itf, op["move"]!.GetValue<long>(), op["origin"]!.GetValue<int>(), pOut);
                    if (!nullOut) o["out_u64"] = ((ulong)Marshal.ReadInt64(outBuf)).ToString();
                    break;
                case "set_size":
                    hr = W411aSlot<W411aSetSizeFn>(itf, 6)(itf, op["size"]!.GetValue<long>());
                    break;
                case "copy_to":
                    hr = W411aSlot<W411aCopyToFn>(itf, 7)(itf, 0, 1, 0, 0);
                    break;
                case "commit":
                    hr = W411aSlot<W411aIntFn>(itf, 8)(itf, 0);
                    break;
                case "revert":
                    hr = W411aSlot<W411aVoidFn>(itf, 9)(itf);
                    break;
                case "lock":
                    hr = W411aSlot<W411aRegionFn>(itf, 10)(itf, 0, 1, 0);
                    break;
                case "unlock":
                    hr = W411aSlot<W411aRegionFn>(itf, 11)(itf, 0, 1, 0);
                    break;
                case "stat":
                {
                    var stat = Marshal.AllocHGlobal(80);
                    try
                    {
                        for (var i = 0; i < 80; i++) Marshal.WriteByte(stat, i, 0xCD);
                        hr = W411aSlot<W411aStatFn>(itf, 12)(itf, stat, op["flag"]?.GetValue<int>() ?? 0);
                        if (hr >= 0)
                        {
                            var raw = new byte[80];
                            Marshal.Copy(stat, raw, 0, 80);
                            o["stat_bytes"] = Convert.ToHexString(raw); // x64 STATSTG, 80 bytes
                        }
                    }
                    finally { Marshal.FreeHGlobal(stat); }
                    break;
                }
                case "clone":
                    hr = W411aSlot<W411aCloneFn>(itf, 13)(itf, outBuf);
                    break;
                case "dispose":
                    ((IDisposable)managed).Dispose();
                    hr = 0;
                    break;
                default:
                    throw new InvalidOperationException("unknown op " + name);
            }
            o["hr"] = W411aHex(hr);
            o["position"] = stream.CanSeek ? stream.Position : null;
            o["length"] = stream.CanSeek ? stream.Length : null;
            if (name is "write" or "set_size")
                o["content"] = new JsonArray(stream.ToArray().Select(b => (JsonNode?)(int)b).ToArray());
        }
        finally
        {
            Marshal.FreeHGlobal(outBuf);
        }
        return o;
    }
}
