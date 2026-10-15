using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json.Nodes;
using PhotoReview.Imaging.Decoding;

// Dispatch line for tools/oracle-cs/Program.cs (inside the `JsonArray cases = command switch { ... }`):
//     // W4-01c
//     "decoder-exceptions" => Commands.DecoderExceptions(inputs),
// Needs a Reference to PhotoReview.Imaging.dll in OracleCs.csproj (the wiring step adds it; today only Core is referenced).

// W4-01c: the three decoder exception types (default messages, ctor forms, base-type facts).
static partial class Commands
{
    private static readonly Type[] W401cProbeTypes =
    [
        typeof(IOException), typeof(NotSupportedException), typeof(InvalidDataException), typeof(COMException),
        typeof(InvalidCastException), typeof(OverflowException), typeof(FormatException),
    ];

    /// <summary>
    /// Cases {name, type: "busy"|"memory-admission"|"missing-codec", ctor: "default"|"message"|"inner", message?: string}.
    /// Reports Message, whether an inner exception is attached (and its message), IsSealed, and which probe base types the
    /// exception is assignable to (the INV-12 whitelist guard).
    /// </summary>
    public static JsonArray DecoderExceptions(JsonObject inputs)
    {
        var result = new JsonArray();
        foreach (var node in inputs["cases"]!.AsArray())
        {
            var c = node!.AsObject();
            var type = c["type"]!.GetValue<string>();
            var ctor = c["ctor"]!.GetValue<string>();
            var message = c["message"]?.GetValue<string>();
            var inner = new NotSupportedException("inner-cause");
            Exception ex = (type, ctor) switch
            {
                ("busy", "default") => new DecoderBusyException(),
                ("busy", "message") => new DecoderBusyException(message!),
                ("busy", "inner") => new DecoderBusyException(message!, inner),
                ("memory-admission", "default") => new DecoderMemoryAdmissionException(),
                ("memory-admission", "message") => new DecoderMemoryAdmissionException(message!),
                ("memory-admission", "inner") => new DecoderMemoryAdmissionException(message!, inner),
                ("missing-codec", "default") => new MissingImageCodecException(),
                ("missing-codec", "message") => new MissingImageCodecException(message!),
                ("missing-codec", "inner") => new MissingImageCodecException(message!, inner),
                _ => throw new ArgumentException($"unknown case {type}/{ctor}"),
            };
            var assignable = new JsonObject();
            foreach (var probe in W401cProbeTypes) assignable[probe.Name] = probe.IsAssignableFrom(ex.GetType());
            result.Add(new JsonObject
            {
                ["name"] = c["name"]!.GetValue<string>(),
                ["type"] = type,
                ["ctor"] = ctor,
                ["message_in"] = message,
                ["message"] = ex.Message,
                ["has_inner"] = ex.InnerException is not null,
                ["inner_message"] = ex.InnerException?.Message,
                ["is_sealed"] = ex.GetType().IsSealed,
                ["assignable_to"] = assignable,
            });
        }
        return result;
    }
}
