using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Imaging.Raw;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W5-12a
//     "raw-header-source-io" => Commands.RawHeaderSourceIo(inputs),
// Needs the <Reference Include="PhotoReview.Imaging.Raw"> that W5-02a / W5-01b add to OracleCs.csproj. Every type used here is public
// (SourceRawHeaderSource, IRawHeaderSource is not needed, ISourceReader, SourceReadPriority). Self-contained: no other oracle file.
// The block/budget arithmetic over a healthy MemoryStream is NOT repeated here: that is W5-01b's `raw-header-budget`, which the
// file-backed Rust source must pass too. This command covers what that one cannot: constructor behaviour, a stream that returns
// short chunks, ends before its declared length or throws, and Dispose.

// W5-12a: SourceRawHeaderSource over a scripted stream.
// Case = { name, ctor: "stream"|"path", length, declared_length?, length_fault?, read_fault?, fault_after?, chunk?, path?, priority?, steps }
//   length          actual bytes the stream holds; byte i = (byte)(i % 251).
//   declared_length stream.Length when different (a stream that ends early; default = length).
//   length_fault    true: reading Length throws IOException("no length") (the constructor must dispose the stream).
//   read_fault      "io" | "disposed": every Read call number >= fault_after (default 0) throws IOException("disk") /
//                   ObjectDisposedException("stream").
//   chunk           the most bytes one Read call returns (default: as many as asked): exercises the read loop.
//   path/priority   ctor "path" only: the source reader records what it was asked for; path "" / whitespace must throw ArgumentException.
//   steps           [{ op: "read", offset, count } | { op: "dispose" }]
// Result = { name, ctor_error (null | "ArgumentException" | "IOException"), ctor_message (IOException only), stream_disposed,
//            open_source (null | { path, priority, buffer_size }), length, steps: [...], stream_reads }.
// A read step reports { op, offset, count, ok, len, first, last, sum, total_bytes_read } or, on failure,
//   { op, offset, count, ok: false, error: exception type name, param, inner: null | "IOException" | "ObjectDisposedException",
//     message (omitted when inner is ObjectDisposedException: the .NET text is not portable), total_bytes_read }.
static partial class Commands
{
    private sealed class W512aStream(int length, long declaredLength, bool lengthFault, string? readFault, int faultAfter, int chunk) : Stream
    {
        private long _position;
        public bool Disposed { get; private set; }
        public int Reads { get; private set; }

        public override bool CanRead => true;
        public override bool CanSeek => true;
        public override bool CanWrite => false;

        public override long Length => lengthFault ? throw new IOException("no length") : declaredLength;

        public override long Position
        {
            get => _position;
            set
            {
                ObjectDisposedException.ThrowIf(Disposed, this);
                _position = value;
            }
        }

        public override int Read(byte[] buffer, int offset, int count)
        {
            ObjectDisposedException.ThrowIf(Disposed, this);
            if (readFault is not null && Reads++ >= faultAfter)
            {
                throw readFault == "io" ? new IOException("disk") : new ObjectDisposedException("stream");
            }

            if (readFault is null) Reads++;
            int n = (int)Math.Max(0, Math.Min(Math.Min(count, chunk), length - _position));
            for (int i = 0; i < n; i++) buffer[offset + i] = (byte)((_position + i) % 251);
            _position += n;
            return n;
        }

        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => Position = offset;
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();

        protected override void Dispose(bool disposing)
        {
            Disposed = true;
            base.Dispose(disposing);
        }
    }

    private sealed class W512aReader(Stream stream) : ISourceReader
    {
        public (string Path, SourceReadPriority Priority, int BufferSize)? Call { get; private set; }

        public Stream OpenSource(string path, SourceReadPriority priority, int bufferSize = 4096)
        {
            Call = (path, priority, bufferSize);
            return stream;
        }
    }

    public static JsonArray RawHeaderSourceIo(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            int length = c["length"]!.GetValue<int>();
            long declared = c["declared_length"]?.GetValue<long>() ?? length;
            var stream = new W512aStream(
                length, declared, c["length_fault"]?.GetValue<bool>() ?? false, c["read_fault"]?.GetValue<string>(),
                c["fault_after"]?.GetValue<int>() ?? 0, c["chunk"]?.GetValue<int>() ?? int.MaxValue);
            var reader = new W512aReader(stream);
            var item = new JsonObject { ["name"] = c["name"]!.GetValue<string>(), ["ctor_error"] = null };

            SourceRawHeaderSource? source = null;
            try
            {
                source = c["ctor"]!.GetValue<string>() == "path"
                    ? new SourceRawHeaderSource(
                        c["path"]!.GetValue<string>(), reader, Enum.Parse<SourceReadPriority>(c["priority"]?.GetValue<string>() ?? "Viewer"))
                    : new SourceRawHeaderSource(stream);
            }
            catch (ArgumentException)
            {
                item["ctor_error"] = "ArgumentException";
            }
            catch (IOException ex)
            {
                item["ctor_error"] = "IOException";
                item["ctor_message"] = ex.Message;
            }

            item["open_source"] = reader.Call is { } call
                ? new JsonObject { ["path"] = call.Path, ["priority"] = call.Priority.ToString(), ["buffer_size"] = call.BufferSize }
                : null;

            if (source is not null)
            {
                item["length"] = source.Length;
                var steps = new JsonArray();
                foreach (var s in c["steps"]!.AsArray())
                {
                    var op = s!["op"]!.GetValue<string>();
                    if (op == "dispose")
                    {
                        source.Dispose();
                        steps.Add(new JsonObject { ["op"] = "dispose", ["stream_disposed"] = stream.Disposed });
                        continue;
                    }

                    long offset = s["offset"]!.GetValue<long>();
                    int count = s["count"]!.GetValue<int>();
                    var step = new JsonObject { ["op"] = "read", ["offset"] = offset, ["count"] = count };
                    try
                    {
                        var span = source.Read(offset, count);
                        uint sum = 0;
                        foreach (var b in span) sum = unchecked(sum + b);
                        step["ok"] = true;
                        step["len"] = span.Length;
                        step["first"] = span.Length > 0 ? span[0] : null;
                        step["last"] = span.Length > 0 ? span[^1] : null;
                        step["sum"] = sum;
                    }
                    catch (Exception ex)
                    {
                        step["ok"] = false;
                        step["error"] = ex.GetType().Name;
                        step["param"] = (ex as ArgumentOutOfRangeException)?.ParamName;
                        step["inner"] = ex.InnerException?.GetType().Name;
                        if (ex.InnerException is not ObjectDisposedException) step["message"] = ex.Message;
                    }

                    step["total_bytes_read"] = source.TotalBytesRead;
                    steps.Add(step);
                }

                item["steps"] = steps;
                source.Dispose();
            }

            item["stream_disposed"] = stream.Disposed;
            item["stream_reads"] = stream.Reads;
            result.Add(item);
        }

        return result;
    }
}
