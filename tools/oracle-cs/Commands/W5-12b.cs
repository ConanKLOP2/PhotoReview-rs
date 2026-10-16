using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Localization;
using PhotoReview.Core.Model;
using PhotoReview.Imaging;
using PhotoReview.Imaging.Caching;
using PhotoReview.Imaging.Decoding;
using PhotoReview.Imaging.Metadata;
using PhotoReview.Imaging.Raw;

// Dispatch lines for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-12b
//     "raw-decoder-core" => Commands.RawDecoderCore(inputs),
//     "raw-decoder-fallback" => Commands.RawDecoderFallback(inputs),
// Needs a Reference to PhotoReview.Imaging.Raw.dll in OracleCs.csproj (the wiring step adds it; same note as W5-01b/W5-11a).
// Everything used here is public API of PhotoReview.Imaging(.Raw) and PhotoReview.Core: no reflection.
//
// W5-12b: RawDecoder driven by scripted fakes (container reader, inner decoder, thumbnail fallback, full decoder, source
// reader) over synthetic files. One engine, two commands: raw-decoder-core (W5-12b1: identity, cache, ReadInfo, Decode
// composition, preview range read) and raw-decoder-fallback (W5-12b2: next-best preview, ORF thumbnail, no-preview path,
// full decode, failure classification). Case schema: contracts/W5-12b.md, "Golden package".
//
// Determinism (D19/D27): physical files live in a fresh temp folder per case and get fixed write times
// (2026-01-01T00:00:00Z, +10 min per "touch", +20 min after an "append"); every message has the case's request path
// replaced by "{path}"; no clock, no cwd, no machine data is reported. The same JSON must come out on every run.
static partial class Commands
{
    public static JsonArray RawDecoderCore(JsonObject inputs) => W512bRunAll(inputs);

    public static JsonArray RawDecoderFallback(JsonObject inputs) => W512bRunAll(inputs);

    private static readonly DateTime W512bBaseTime = new(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc);
    private const string W512bVirtualPath = @"C:\raw-virtual-nonexistent\a.dng";

    private static JsonArray W512bRunAll(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
            result.Add(W512bRun(node!.AsObject()));
        return result;
    }

    // ---- per-case state ----------------------------------------------------------------------------------------------

    private sealed class W512bState
    {
        public string Path = "";
        public bool Physical;
        public byte[] Bytes = [];
        public int Opens;
        public long StreamBytesRead;
        public int ContainerReads;
        public int FallbackCalls;
        public int FullCalls;
        public int InnerCalls;
        public bool HooksEnabled = true;
        public int Touches;
        public readonly List<JsonObject> InnerRequests = [];
    }

    // ---- fakes -------------------------------------------------------------------------------------------------------

    private sealed class W512bComException(string message, int hresult) : COMException(message, hresult);

    /// <summary>{type, message, hresult?, inner_hresult?} -> the exception the fakes throw.</summary>
    private static Exception W512bMakeException(JsonObject spec)
    {
        var message = spec["message"]?.GetValue<string>() ?? "scripted";
        Exception? inner = spec["inner_hresult"] is { } ih ? new W512bComException("inner", ih.GetValue<int>()) : null;
        return spec["type"]!.GetValue<string>() switch
        {
            "InvalidData" => new InvalidDataException(message, inner),
            "Format" => new FormatException(message, inner),
            "FileFormat" => new FileFormatException(message, inner),
            "NotSupported" => new NotSupportedException(message, inner),
            "MissingCodec" => new MissingImageCodecException(message),
            "Com" => new W512bComException(message, spec["hresult"]!.GetValue<int>()),
            "Io" => new IOException(message, inner),
            "Canceled" => new OperationCanceledException(message),
            "OutOfMemory" => new OutOfMemoryException(message),
            "InvalidCast" => new InvalidCastException(message),
            var other => throw new ArgumentException("unknown exception type " + other),
        };
    }

    private sealed class W512bFakeImage(int width, int height) : IDecodedImage
    {
        public int PixelWidth { get; } = width;
        public int PixelHeight { get; } = height;
        public bool Downscaled { get; init; }
        public int Orientation => 1;
        public long EstimatedBytes => (long)PixelWidth * PixelHeight * 4;
        public object PlatformImage { get; } = new();
        public int OriginalWidth { get; init; } = width;
        public int OriginalHeight { get; init; } = height;
        public ExifSummary? Exif { get; init; }
        public DecoderBackend ActualBackend { get; init; } = DecoderBackend.Wpf;
        public bool IsDegradedFallback { get; init; }
    }

    /// <summary>{w, h, downscaled?, original?:[w,h], exif_make?, backend?, degraded?} -> a fake decoded image.</summary>
    private static IDecodedImage W512bMakeImage(JsonObject ok) => new W512bFakeImage(ok["w"]!.GetValue<int>(), ok["h"]!.GetValue<int>())
    {
        Downscaled = ok["downscaled"]?.GetValue<bool>() ?? false,
        OriginalWidth = ok["original"] is JsonArray o ? o[0]!.GetValue<int>() : ok["w"]!.GetValue<int>(),
        OriginalHeight = ok["original"] is JsonArray o2 ? o2[1]!.GetValue<int>() : ok["h"]!.GetValue<int>(),
        Exif = ok["exif_make"] is { } make ? new ExifSummary { CameraMake = make.GetValue<string>() } : null,
        ActualBackend = ok["backend"] is { } b ? Enum.Parse<DecoderBackend>(b.GetValue<string>()) : DecoderBackend.Wpf,
        IsDegradedFallback = ok["degraded"]?.GetValue<bool>() ?? false,
    };

    /// <summary>{ok:{..}} | {throw:{..}} -> run it.</summary>
    private static IDecodedImage W512bOutcome(JsonObject outcome) =>
        outcome["throw"] is JsonObject t ? throw W512bMakeException(t) : W512bMakeImage(outcome["ok"]!.AsObject());

    /// <summary>
    /// Inner decoder: answers by the request's byte length ("by_length" map, else "default"); "first_call" (optional) runs once,
    /// first: {touch: true} sets the file's write time +10 min (the stale-offsets test), then its own ok/throw.
    /// </summary>
    private sealed class W512bInner(JsonObject spec, W512bState state) : IImageDecoder
    {
        public ImageInfo ReadInfo(string path) => throw new NotSupportedException("inner ReadInfo is never called by RawDecoder");

        public IDecodedImage Decode(DecodeRequest request)
        {
            var length = request.Bytes?.Length ?? -1;
            state.InnerCalls++;
            state.InnerRequests.Add(new JsonObject
            {
                ["length"] = length,
                ["first_byte"] = length > 0 ? request.Bytes!.Value.Span[0] : null,
                ["last_byte"] = length > 0 ? request.Bytes!.Value.Span[length - 1] : null,
                ["box"] = new JsonArray(request.Box.Width, request.Box.Height),
                ["apply_orientation"] = request.ApplyOrientation,
                ["source_orientation"] = request.SourceOrientation,
                ["priority"] = request.Priority.ToString(),
            });
            if (state.InnerCalls == 1 && spec["first_call"] is JsonObject first)
            {
                if (first["touch"]?.GetValue<bool>() == true) W512bTouch(state);
                return W512bOutcome(first);
            }

            if (spec["by_length"] is JsonObject map && map[length.ToString(System.Globalization.CultureInfo.InvariantCulture)] is JsonObject hit)
                return W512bOutcome(hit);
            return W512bOutcome(spec["default"]!.AsObject());
        }
    }

    private sealed class W512bFull(JsonObject spec, W512bState state) : IImageDecoder
    {
        public ImageInfo ReadInfo(string path) => throw new NotSupportedException();

        public IDecodedImage Decode(DecodeRequest request)
        {
            state.FullCalls++;
            return W512bOutcome(spec);
        }
    }

    /// <summary>{bytes: N} (N zero bytes) | {throw: {..}}.</summary>
    private sealed class W512bFallback(JsonObject spec, W512bState state) : IRawPreviewFallback
    {
        public ReadOnlyMemory<byte> ReadJpegThumbnail(string path, RawFormat format)
        {
            state.FallbackCalls++;
            if (spec["throw"] is JsonObject t) throw W512bMakeException(t);
            return new byte[spec["bytes"]!.GetValue<int>()];
        }
    }

    private sealed class W512bFixedReader(JsonObject spec, RawContainerInfo? info, W512bState state) : IRawContainerReader
    {
        public bool CanRead(ReadOnlySpan<byte> first64Bytes, string extension) =>
            extension.Equals(spec["ext"]?.GetValue<string>() ?? ".dng", StringComparison.OrdinalIgnoreCase);

        public RawContainerInfo Read(IRawHeaderSource source, CancellationToken cancellationToken)
        {
            state.ContainerReads++;
            if (spec["on_read"]?.GetValue<string>() == "move-file") File.Move(state.Path, state.Path + ".moved");
            if (spec["kind"]!.GetValue<string>() == "throws")
                throw new InvalidDataException(spec["message"]!.GetValue<string>());
            return info!;
        }
    }

    /// <summary>Counts opens and stream bytes; runs the open hooks; serves the physical file or the virtual bytes.</summary>
    private sealed class W512bSourceReader(JsonObject file, JsonArray? openHooks, W512bState state) : ISourceReader
    {
        public Stream OpenSource(string path, SourceReadPriority priority, int bufferSize = 1024 * 1024)
        {
            state.Opens++;
            if (state.HooksEnabled && openHooks is not null)
            {
                foreach (var h in openHooks)
                {
                    var hook = h!.AsObject();
                    if (hook["n"]!.GetValue<int>() != state.Opens) continue;
                    switch (hook["action"]!.GetValue<string>())
                    {
                        case "touch": W512bTouch(state); break;
                        case "append":
                            File.WriteAllBytes(state.Path, [.. state.Bytes, .. new byte[hook["bytes"]!.GetValue<int>()]]);
                            File.SetLastWriteTimeUtc(state.Path, W512bBaseTime.AddMinutes(20));
                            break;
                        default: throw new ArgumentException("unknown hook action");
                    }
                }
            }

            Stream inner;
            if (file["faulty_read"] is { } fault)
                inner = new W512bFaultyStream(file["size"]!.GetValue<long>(), fault.GetValue<string>());
            else if (file["zero_length"] is { } zl)
                inner = new W512bZeroStream(zl.GetValue<long>());
            else if (state.Physical)
                inner = PhysicalSourceReader.Instance.OpenSource(path, priority, bufferSize);
            else
                inner = new MemoryStream(state.Bytes, writable: false);
            return new W512bCountingStream(inner, file["max_chunk"]?.GetValue<int>() ?? int.MaxValue, state);
        }
    }

    private sealed class W512bCountingStream(Stream inner, int maxChunk, W512bState state) : Stream
    {
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => inner.Length;
        public override long Position { get => inner.Position; set => inner.Position = value; }
        public override void Flush() { }

        public override int Read(byte[] buffer, int offset, int count)
        {
            var read = inner.Read(buffer, offset, Math.Min(count, maxChunk));
            state.StreamBytesRead += read;
            return read;
        }

        public override long Seek(long offset, SeekOrigin origin) => inner.Seek(offset, origin);
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();

        protected override void Dispose(bool disposing)
        {
            if (disposing) inner.Dispose();
            base.Dispose(disposing);
        }
    }

    private sealed class W512bZeroStream(long length) : Stream
    {
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => length;
        public override long Position { get; set; }
        public override void Flush() { }

        public override int Read(byte[] buffer, int offset, int count)
        {
            var n = (int)Math.Min(count, Math.Max(0, length - Position));
            Array.Clear(buffer, offset, n);
            Position += n;
            return n;
        }

        public override long Seek(long offset, SeekOrigin origin) =>
            Position = origin switch { SeekOrigin.Begin => offset, SeekOrigin.Current => Position + offset, _ => length + offset };

        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    /// <summary>A stream whose every Read throws IOException(message) (RawHeaderSourceIoTests.FaultyStream).</summary>
    private sealed class W512bFaultyStream(long length, string message) : Stream
    {
        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;
        public override long Length => length;
        public override long Position { get; set; }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count) => throw new IOException(message);
        public override long Seek(long offset, SeekOrigin origin) => Position = offset;
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    private static void W512bTouch(W512bState state)
    {
        state.Touches++;
        File.SetLastWriteTimeUtc(state.Path, W512bBaseTime.AddMinutes(10 * state.Touches));
    }

    // ---- builders ----------------------------------------------------------------------------------------------------

    private static byte[] W512bBuildFile(JsonObject file)
    {
        var size = file["size"]?.GetValue<int>() ?? 0;
        var fill = file["fill"] is { } f ? Convert.FromHexString(f.GetValue<string>())[0] : (byte)0;
        var bytes = new byte[size];
        if (fill != 0) Array.Fill(bytes, fill);
        foreach (var s in file["segments"]?.AsArray() ?? [])
        {
            var seg = s!.AsObject();
            var data = Convert.FromHexString(seg["hex"]!.GetValue<string>());
            var at = seg["at"]!.GetValue<int>();
            if (at + data.Length > bytes.Length) Array.Resize(ref bytes, at + data.Length);
            data.CopyTo(bytes, at);
        }
        return bytes;
    }

    private static RawContainerInfo W512bInfo(JsonObject info) => new(
        Enum.Parse<RawFormat>(info["format"]!.GetValue<string>()),
        info["sensor"]![0]!.GetValue<int>(), info["sensor"]![1]!.GetValue<int>(),
        info["orientation"]?.GetValue<int>() ?? 1,
        (info["previews"]?.AsArray() ?? []).Select(p =>
        {
            var o = p!.AsObject();
            return new EmbeddedPreview(o["index"]!.GetValue<int>(), o["offset"]!.GetValue<long>(), o["length"]!.GetValue<long>(),
                EmbeddedPreviewKind.Jpeg, o["width"]!.GetValue<int>(), o["height"]!.GetValue<int>(),
                Enum.Parse<PreviewColorSpace>(o["color_space"]?.GetValue<string>() ?? "Srgb"));
        }).ToArray(),
        (info["exif_blocks"]?.AsArray() ?? []).Select(b =>
        {
            var o = b!.AsObject();
            return new ExifBlock(o["offset"]!.GetValue<long>(), o["length"]!.GetValue<long>(), o["tiff"]!.GetValue<bool>(),
                o["ifd_is_exif"]?.GetValue<bool>() ?? false);
        }).ToArray());

    // ---- reporting ---------------------------------------------------------------------------------------------------

    private static string W512bTypeName(Exception ex) => ex switch
    {
        EndOfStreamException => "EndOfStreamException",
        FileNotFoundException => "FileNotFoundException",
        DirectoryNotFoundException => "DirectoryNotFoundException",
        IOException => "IOException",
        InvalidDataException => "InvalidDataException",
        FileFormatException or FormatException => "FormatException",
        MissingImageCodecException => "MissingImageCodecException",
        NotSupportedException => "NotSupportedException",
        COMException => "COMException",
        OperationCanceledException => "OperationCanceledException",
        OutOfMemoryException => "OutOfMemoryException",
        ArgumentException => "ArgumentException",
        OverflowException => "OverflowException",
        InvalidCastException => "InvalidCastException",
        _ => ex.GetType().Name,
    };

    private static JsonObject W512bError(Exception ex, string path)
    {
        string? key = null;
        if (UserFacingError.IsLocalized(ex))
        {
            var text = UserFacingError.Describe(ex);
            key = text == Tr.ImageErrorRawCorrupt ? "image.error.rawCorrupt"
                : text == Tr.ImageErrorRawUnsupported ? "image.error.rawUnsupported"
                : text == Tr.ImageErrorRawNoPreview ? "image.error.rawNoPreview"
                : text == Tr.ErrIoFileChangedWhileReading(path) ? "err.io.fileChangedWhileReading"
                : "other:" + text.Replace(path, "{path}", StringComparison.Ordinal);
        }
        return new JsonObject
        {
            ["type"] = W512bTypeName(ex),
            ["message"] = ex.Message.Replace(path, "{path}", StringComparison.Ordinal),
            ["hresult"] = ex is COMException ? ex.HResult : null,
            ["localized_key"] = key,
        };
    }

    private static JsonObject W512bImage(IDecodedImage d) => new()
    {
        ["pixel"] = new JsonArray(d.PixelWidth, d.PixelHeight),
        ["original"] = new JsonArray(d.OriginalWidth, d.OriginalHeight),
        ["downscaled"] = d.Downscaled,
        ["orientation"] = d.Orientation,
        ["degraded"] = d.IsDegradedFallback,
        ["estimated_bytes"] = d.EstimatedBytes,
        ["source_bytes_read"] = d is ISourceReadMetrics m ? m.SourceBytesRead : null,
        ["embedded_preview"] = d is IRawPreviewInfo p ? new JsonArray(p.EmbeddedPreviewWidth, p.EmbeddedPreviewHeight) : null,
        ["backend"] = d.ActualBackend.ToString(),
        ["exif_make"] = d.Exif?.CameraMake,
    };

    // ---- engine ------------------------------------------------------------------------------------------------------

    private static JsonObject W512bRun(JsonObject c)
    {
        var state = new W512bState();
        var file = c["file"]!.AsObject();
        state.Physical = file["physical"]?.GetValue<bool>() ?? false;
        state.Bytes = W512bBuildFile(file);
        string? root = null;
        try
        {
            if (state.Physical)
            {
                root = Path.Combine(Path.GetTempPath(), "pr-oracle-w512b-" + Guid.NewGuid().ToString("N"));
                Directory.CreateDirectory(root);
                state.Path = Path.Combine(root, file["name"]?.GetValue<string>() ?? "a.dng");
                File.WriteAllBytes(state.Path, state.Bytes);
                File.SetLastWriteTimeUtc(state.Path, W512bBaseTime);
            }
            else
            {
                state.Path = file["virtual_path"]?.GetValue<string>() ?? W512bVirtualPath;
            }

            var container = c["container"]!.AsObject();
            var registry = container["kind"]!.GetValue<string>() switch
            {
                "fixed" => new RawContainerReaderRegistry([new W512bFixedReader(container, W512bInfo(container["info"]!.AsObject()), state)]),
                "throws" => new RawContainerReaderRegistry([new W512bFixedReader(container, null, state)]),
                "none" => new RawContainerReaderRegistry([]),
                "default" => new RawContainerReaderRegistry(),
                var k => throw new ArgumentException("unknown container kind " + k),
            };
            var sourceReader = new W512bSourceReader(file, c["open_hooks"] as JsonArray, state);
            var cache = c["cache"] is JsonObject cc ? new SourceBytesCache(cc["capacity"]!.GetValue<long>()) : null;
            var decoder = new RawDecoder(
                new W512bInner(c["inner"]!.AsObject(), state),
                sourceReader,
                registry,
                cache,
                c["fallback"] is JsonObject fb ? new W512bFallback(fb, state) : null,
                c["full"] is JsonObject full ? new W512bFull(full, state) : null);

            var steps = new JsonArray();
            foreach (var o in c["ops"]!.AsArray())
            {
                var op = o!.AsObject();
                var kind = op["op"]!.GetValue<string>();
                if (kind == "hooks")
                {
                    state.HooksEnabled = op["enabled"]!.GetValue<bool>();
                    continue;
                }

                if (kind == "rewrite")
                {
                    // Same path, new content ({size, fill?, segments}), write time base + minutes.
                    state.Bytes = W512bBuildFile(op["file"]!.AsObject());
                    File.WriteAllBytes(state.Path, state.Bytes);
                    File.SetLastWriteTimeUtc(state.Path, W512bBaseTime.AddMinutes(op["minutes"]!.GetValue<int>()));
                    continue;
                }

                state.InnerRequests.Clear();
                int fallbackBefore = state.FallbackCalls, fullBefore = state.FullCalls, opensBefore = state.Opens;
                long streamBefore = state.StreamBytesRead;
                var step = new JsonObject { ["op"] = kind };
                try
                {
                    if (kind == "read_info")
                    {
                        var info = decoder.ReadInfo(state.Path);
                        step["info"] = new JsonObject
                        {
                            ["pixel"] = new JsonArray(info.PixelWidth, info.PixelHeight),
                            ["orientation"] = info.Orientation,
                            ["size"] = new JsonArray(info.Width, info.Height),
                        };
                    }
                    else
                    {
                        var box = op["box"] is JsonArray b ? new DecodeBox(b[0]!.GetValue<int>(), b[1]!.GetValue<int>()) : DecodeBox.Unbounded;
                        var request = new DecodeRequest(state.Path, box,
                            applyOrientation: op["apply_orientation"]?.GetValue<bool>() ?? true,
                            priority: Enum.Parse<SourceReadPriority>(op["priority"]?.GetValue<string>() ?? "Viewer"));
                        step["ok"] = W512bImage(decoder.Decode(request));
                    }
                }
                catch (Exception ex)
                {
                    step["error"] = W512bError(ex, state.Path);
                }

                step["inner_requests"] = new JsonArray(state.InnerRequests.Select(r => (JsonNode?)r.DeepClone()).ToArray());
                step["fallback_calls"] = state.FallbackCalls - fallbackBefore;
                step["full_calls"] = state.FullCalls - fullBefore;
                step["opens"] = state.Opens - opensBefore;
                step["stream_bytes_read"] = state.StreamBytesRead - streamBefore;
                step["container_reads_total"] = state.ContainerReads;
                if (cache is not null)
                {
                    step["cache_count"] = cache.Count;
                    step["cache_size"] = cache.CurrentSize;
                }
                steps.Add(step);
            }

            var output = (JsonObject)c.DeepClone();
            output["steps"] = steps;
            return output;
        }
        finally
        {
            if (root is not null)
            {
                try { Directory.Delete(root, recursive: true); } catch (IOException) { }
            }
        }
    }
}
