using System.Text.Json.Nodes;
using PhotoReview.Core.Abstractions;
using PhotoReview.Core.Model;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-12a
//     "decoder-factory" => Commands.DecoderFactory(inputs),
// Needs a Reference to PhotoReview.Imaging.dll in OracleCs.csproj (the wiring step adds it; see W4-01c.cs).

// W4-12a: ImageDecoderFactory construction rules, slot mapping, fallback wrapping (observed by decoding), build-once, decorator.
static partial class Commands
{
    private sealed class W412aLog : ILog
    {
        public readonly List<string> Warnings = [];
        public bool Enabled => true;
        public void Info(string message) { }
        public void Warn(string message) => Warnings.Add(message);
        public void Error(string message, Exception? ex = null) { }
    }

    /// <summary>Decodes to an image whose PlatformImage is the marker name, or throws NotSupportedException (a fallbackable failure).</summary>
    private sealed class W412aMarker(string name, bool fails) : IImageDecoder
    {
        public string Name { get; } = name;

        public IDecodedImage Decode(DecodeRequest request) =>
            fails
                ? throw new NotSupportedException("marker " + Name + " fails")
                : new DecodedImage(Name, 1, 1, 4, actualBackend: DecoderBackend.WicDirect);

        public ImageInfo ReadInfo(string path) =>
            fails ? throw new NotSupportedException("marker " + Name + " fails") : new ImageInfo(1, 1);
    }

    private sealed class W412aDecorated(IImageDecoder inner) : IImageDecoder
    {
        public IImageDecoder Inner { get; } = inner;
        public IDecodedImage Decode(DecodeRequest request) => Inner.Decode(request);
        public ImageInfo ReadInfo(string path) => Inner.ReadInfo(path);
    }

    /// <summary>
    /// Cases {name, providers:[{backend, name, fails?}], wpf_factory?:{name, fails?}, decorator?:bool, create:[backend...]}.
    /// Reports ctor_error ({type, message} or null), is_registered for every DecoderBackend, per create
    /// {backend, decorated, wrapped_in_fallback, same_instance_as_previous (null on first), decode:{ok, served_by, actual_backend | error_type},
    /// read_info:{ok | error_type}}, provider_runs (marker name -> count), decorator_calls [{backend, inner_type}], warnings.
    /// </summary>
    public static JsonArray DecoderFactory(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var runs = new Dictionary<string, int>();
            var decoratorCalls = new JsonArray();
            var log = new W412aLog();

            Func<IImageDecoder> Provider(string name, bool fails) => () =>
            {
                runs[name] = runs.GetValueOrDefault(name) + 1;
                return new W412aMarker(name, fails);
            };

            var providers = new List<(DecoderBackend, Func<IImageDecoder>)>();
            foreach (var p in c["providers"]!.AsArray())
            {
                var o = p!.AsObject();
                providers.Add((Enum.Parse<DecoderBackend>(o["backend"]!.GetValue<string>()),
                    Provider(o["name"]!.GetValue<string>(), o["fails"]?.GetValue<bool>() ?? false)));
            }

            Func<IImageDecoder>? wpfFactory = null;
            if (c["wpf_factory"] is JsonObject wf)
            {
                wpfFactory = Provider(wf["name"]!.GetValue<string>(), wf["fails"]?.GetValue<bool>() ?? false);
            }

            Func<DecoderBackend, IImageDecoder, IImageDecoder>? decorator = null;
            if (c["decorator"]?.GetValue<bool>() == true)
            {
                decorator = (b, d) =>
                {
                    decoratorCalls.Add(new JsonObject
                    {
                        ["backend"] = b.ToString(),
                        ["inner_type"] = d is FallbackImageDecoder ? "fallback" : d is W412aMarker m ? "marker:" + m.Name : d.GetType().Name,
                    });
                    return new W412aDecorated(d);
                };
            }

            var entry = new JsonObject { ["name"] = c["name"]!.GetValue<string>() };
            ImageDecoderFactory? factory = null;
            try
            {
                factory = new ImageDecoderFactory(providers, log, null, decorator, wpfFactory);
                entry["ctor_error"] = null;
            }
            catch (Exception ex)
            {
                entry["ctor_error"] = new JsonObject { ["type"] = ex.GetType().Name, ["message"] = ex.Message };
            }

            var created = new JsonArray();
            if (factory is not null)
            {
                var registered = new JsonObject();
                foreach (var b in Enum.GetValues<DecoderBackend>())
                {
                    registered[b.ToString()] = factory.IsRegistered(b);
                }

                entry["is_registered"] = registered;

                var seen = new Dictionary<DecoderBackend, IImageDecoder>();
                foreach (var bn in c["create"]!.AsArray())
                {
                    var backend = Enum.Parse<DecoderBackend>(bn!.GetValue<string>());
                    var decoder = factory.Create(backend);
                    bool? same = seen.TryGetValue(backend, out var prev) ? ReferenceEquals(prev, decoder) : null;
                    seen[backend] = decoder;

                    var raw = decoder is W412aDecorated d ? d.Inner : decoder;
                    var row = new JsonObject
                    {
                        ["backend"] = backend.ToString(),
                        ["decorated"] = decoder is W412aDecorated,
                        ["wrapped_in_fallback"] = raw is FallbackImageDecoder,
                        ["same_instance_as_previous"] = same,
                    };
                    try
                    {
                        var img = decoder.Decode(new DecodeRequest("x.jpg", 0));
                        row["decode"] = new JsonObject
                        {
                            ["ok"] = true,
                            ["served_by"] = img.PlatformImage.ToString(),
                            ["actual_backend"] = img.ActualBackend.ToString(),
                        };
                    }
                    catch (Exception ex)
                    {
                        row["decode"] = new JsonObject { ["ok"] = false, ["error_type"] = ex.GetType().Name };
                    }

                    try
                    {
                        decoder.ReadInfo("x.jpg");
                        row["read_info"] = new JsonObject { ["ok"] = true };
                    }
                    catch (Exception ex)
                    {
                        row["read_info"] = new JsonObject { ["ok"] = false, ["error_type"] = ex.GetType().Name };
                    }

                    created.Add(row);
                }
            }

            entry["create"] = created;
            var runsJson = new JsonObject();
            foreach (var (k, v) in runs.OrderBy(kv => kv.Key, StringComparer.Ordinal))
            {
                runsJson[k] = v;
            }

            entry["provider_runs"] = runsJson;
            entry["decorator_calls"] = decoratorCalls;
            entry["warnings"] = new JsonArray(log.Warnings.Select(w => (JsonNode?)JsonValue.Create(w)).ToArray());
            result.Add(entry);
        }

        return result;
    }
}
