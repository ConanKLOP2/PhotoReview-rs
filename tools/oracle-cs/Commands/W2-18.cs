using System.Globalization;
using System.Net;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PhotoReview.Core.Updates;

// W2-18 oracle: UpdateChecker.CheckAsync against a fake HttpMessageHandler.
//
// Dispatch line for tools/oracle-cs/Program.cs (inside the `command switch`):
//     "update-checker" => Commands.UpdateCheckerCases(inputs),
//
// The oracle appends one extra case per System.Net.Http.HttpRequestError value (id "http-request-error:<Name>") so the
// classification table is complete even for values added by a newer .NET; it reports the enum names it saw in
// "http_request_error_names" on every such case.
static partial class Commands
{
    private sealed class UcHandler(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> respond) : HttpMessageHandler
    {
        public List<HttpRequestMessage> Requests { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Requests.Add(request);
            return respond(request, cancellationToken);
        }
    }

    /// <summary>A stream that is not seekable, so StreamContent cannot report a Content-Length.</summary>
    private sealed class UcForwardOnlyStream(byte[] data) : Stream
    {
        private int _pos;
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException();
        public override long Position { get => _pos; set => throw new NotSupportedException(); }
        public override void Flush() { }
        public override int Read(byte[] buffer, int offset, int count)
        {
            var n = Math.Min(count, data.Length - _pos);
            Array.Copy(data, _pos, buffer, offset, n);
            _pos += n;
            return n;
        }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }

    /// <summary>A body that declares a Content-Length and records whether anything asked for its bytes.</summary>
    private sealed class UcDeclaredLengthContent(long declaredLength, byte[] data) : HttpContent
    {
        public bool BodyWasRequested { get; private set; }

        protected override bool TryComputeLength(out long length)
        {
            length = declaredLength;
            return true;
        }

        protected override Task SerializeToStreamAsync(Stream stream, TransportContext? context)
        {
            BodyWasRequested = true;
            return stream.WriteAsync(data, 0, data.Length);
        }

        protected override Task<Stream> CreateContentReadStreamAsync()
        {
            BodyWasRequested = true;
            return Task.FromResult<Stream>(new MemoryStream(data));
        }
    }

    private static byte[] UcBody(JsonObject? body)
    {
        if (body is null) return [];
        if (body["raw_hex"] is { } hex) return Convert.FromHexString(hex.GetValue<string>());
        var text = body["text"]?.GetValue<string>() ?? "";
        // "nested_arrays": n -> {"a":[[...[]...]],"tag_name":"v2.0.94"} with exactly n nested arrays inside the root object.
        if (body["nested_arrays"] is { } nest)
        {
            var n = nest.GetValue<int>();
            text = "{\"a\":" + new string('[', n) + new string(']', n) + ",\"tag_name\":\"v2.0.94\"}";
        }
        var bytes = Encoding.UTF8.GetBytes(text);
        if (body["padded_to"] is { } padded)
        {
            var total = padded.GetValue<int>();
            if (total < bytes.Length) throw new ArgumentException("padded_to smaller than the text");
            var result = new byte[total];
            Array.Fill(result, (byte)' ');
            bytes.CopyTo(result, 0);
            return result;
        }
        if (body["bom"]?.GetValue<bool>() == true) bytes = [0xEF, 0xBB, 0xBF, .. bytes];
        return bytes;
    }

    public static JsonArray UpdateCheckerCases(JsonObject inputs)
    {
        var maxBody = (int)typeof(UpdateChecker).GetField("MaxBodyBytes", BindingFlags.NonPublic | BindingFlags.Static)!.GetRawConstantValue()!;
        var cases = inputs["cases"]!.AsArray().Select(n => (JsonObject)n!.DeepClone()).ToList();
        var errorNames = Enum.GetNames<HttpRequestError>();
        foreach (var name in errorNames)
        {
            cases.Add(new JsonObject
            {
                ["id"] = "http-request-error:" + name,
                ["current"] = "2.0.93",
                ["transport"] = new JsonObject { ["kind"] = "throw_http_request_error", ["error"] = name },
                ["http_request_error_names"] = new JsonArray(errorNames.Select(x => (JsonNode?)x).ToArray()),
            });
        }

        var result = new JsonArray();
        foreach (var c in cases)
        {
            var t = c["transport"]!.AsObject();
            var kind = t["kind"]!.GetValue<string>();
            UcDeclaredLengthContent? declared = null;
            using var callerCts = new CancellationTokenSource();
            var started = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            var handler = new UcHandler(async (_, ct) =>
            {
                switch (kind)
                {
                    case "throw_http_request_error":
                        throw new HttpRequestException(Enum.Parse<HttpRequestError>(t["error"]!.GetValue<string>()), "simulated");
                    case "throw_http_request_exception_no_error":
                        throw new HttpRequestException("no network");
                    case "throw_task_canceled":
                        throw new TaskCanceledException("timed out");
                    case "throw_invalid_operation":
                        throw new InvalidOperationException("handler broke");
                    case "hang":
                        await Task.Delay(Timeout.Infinite, ct);
                        return new HttpResponseMessage(HttpStatusCode.OK);
                    case "cancel_by_caller":
                        started.SetResult();
                        await Task.Delay(Timeout.Infinite, ct);
                        return new HttpResponseMessage(HttpStatusCode.OK);
                }

                var status = (HttpStatusCode)(t["status"]?.GetValue<int>() ?? 200);
                var response = new HttpResponseMessage(status);
                var bytes = UcBody(t["body"]?.AsObject());
                switch (t["length_mode"]?.GetValue<string>() ?? "buffered")
                {
                    case "buffered":
                        response.Content = new ByteArrayContent(bytes);
                        break;
                    case "stream_unknown_length":
                        response.Content = new StreamContent(new UcForwardOnlyStream(bytes));
                        break;
                    case "declared":
                        declared = new UcDeclaredLengthContent(t["declared_length"]!.GetValue<long>(), bytes);
                        response.Content = declared;
                        break;
                    case "text_string_content":
                        response.Content = new StringContent(Encoding.UTF8.GetString(bytes), Encoding.UTF8, "application/json");
                        break;
                }
                // "headers": validated Add; "raw_headers": TryAddWithoutValidation (malformed values are kept by .NET but typed accessors
                // such as RetryAfter then read null). A value may be a string or an array of strings (repeated header).
                foreach (var (name, value) in t["headers"]?.AsObject() ?? [])
                    foreach (var v in value is JsonArray many ? many.Select(x => x!.GetValue<string>()) : [value!.GetValue<string>()])
                        response.Headers.Add(name, v);
                foreach (var (name, value) in t["raw_headers"]?.AsObject() ?? [])
                    foreach (var v in value is JsonArray many ? many.Select(x => x!.GetValue<string>()) : [value!.GetValue<string>()])
                        response.Headers.TryAddWithoutValidation(name, v);
                if (t["retry_after_seconds"] is { } ra)
                    response.Headers.RetryAfter = new System.Net.Http.Headers.RetryConditionHeaderValue(TimeSpan.FromSeconds(ra.GetValue<int>()));
                return response;
            });

            TimeSpan? timeout = c["timeout_ms"] is { } ms ? TimeSpan.FromMilliseconds(ms.GetValue<int>()) : null;
            var checker = new UpdateChecker(handler, timeout);
            var current = c["current"]?.GetValue<string>();
            var item = (JsonObject)c.DeepClone();
            item.Remove("http_request_error_names");
            if (c["http_request_error_names"] is { } names) item["http_request_error_names"] = names.DeepClone();
            var pending = checker.CheckAsync(current, callerCts.Token);
            if (kind == "cancel_by_caller")
            {
                Task.WhenAny(started.Task, pending).GetAwaiter().GetResult();
                callerCts.Cancel();
            }

            try
            {
                var r = pending.GetAwaiter().GetResult();
                item["result"] = new JsonObject
                {
                    ["status"] = r.Status.ToString(),
                    ["latest_version"] = r.LatestVersion,
                    ["download_url"] = r.DownloadUrl,
                    ["failure"] = r.Failure.ToString(),
                };
            }
            catch (OperationCanceledException)
            {
                item["result"] = new JsonObject { ["threw"] = "OperationCanceledException" };
            }

            item["request_count"] = handler.Requests.Count;
            if (handler.Requests.Count > 0)
            {
                var req = handler.Requests[0];
                item["request"] = new JsonObject
                {
                    ["method"] = req.Method.Method,
                    ["uri"] = req.RequestUri!.AbsoluteUri,
                    ["user_agent"] = req.Headers.UserAgent.ToString(),
                    ["accept"] = req.Headers.Accept.ToString(),
                    ["has_content"] = req.Content is not null,
                    ["query"] = req.RequestUri.Query,
                };
            }

            if (declared is not null) item["body_was_requested"] = declared.BodyWasRequested;
            item["max_body_bytes"] = maxBody;
            result.Add(item);
        }

        return result;
    }
}
