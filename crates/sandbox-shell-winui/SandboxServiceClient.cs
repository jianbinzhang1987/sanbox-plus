using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SandboxShell.WinUI;

internal sealed class SandboxServiceClient
{
    private const string PipeName = "SandboxPlus.Service";

    public async Task<SandboxStatusViewModel> GetStatusAsync(CancellationToken cancellationToken = default)
    {
        var response = await SendAsync(JsonValue.Create("GetStatus")!, cancellationToken);
        var status = RequirePayload(response, "Status");
        var instance = status["instance"] is JsonObject instanceJson
            ? SandboxInstanceViewModel.FromJson(instanceJson)
            : null;

        return new SandboxStatusViewModel(
            instance,
            status["health"]?.ToJsonString() ?? "Unknown",
            ReadNullableString(status["active_policy_version"]));
    }

    public async Task<IReadOnlyList<SandboxAppViewModel>> ListAppsAsync(
        SandboxIdViewModel sandboxId,
        CancellationToken cancellationToken = default)
    {
        var response = await SendAsync(new JsonObject
        {
            ["ListApps"] = new JsonObject
            {
                ["sandbox_id"] = sandboxId.Value
            }
        }, cancellationToken);

        var apps = RequirePayload(response, "Apps").AsArray();
        return apps
            .OfType<JsonObject>()
            .Select(SandboxAppViewModel.FromJson)
            .ToList();
    }

    public async Task LaunchAppAsync(
        SandboxInstanceViewModel instance,
        SandboxAppViewModel app,
        CancellationToken cancellationToken = default)
    {
        var request = new JsonObject
        {
            ["LaunchApp"] = new JsonObject
            {
                ["sandbox_id"] = instance.Id.Value,
                ["app_id"] = app.Id,
                ["executable"] = app.Executable,
                ["arguments"] = JsonSerializer.SerializeToNode(app.Arguments),
                ["working_directory"] = app.WorkingDirectory,
                ["desktop_name"] = instance.DesktopName,
                ["profile_root"] = instance.ProfileRoot,
                ["environment_overrides"] = new JsonArray(),
                ["policy_version"] = instance.PolicyVersion
            }
        };

        var response = await SendAsync(request, cancellationToken);
        _ = RequirePayload(response, "LaunchApp");
    }

    public async Task ReturnToHostAsync(
        SandboxInstanceViewModel instance,
        CancellationToken cancellationToken = default)
    {
        var response = await SendAsync(new JsonObject
        {
            ["ReturnToHost"] = new JsonObject
            {
                ["sandbox_id"] = instance.Id.Value
            }
        }, cancellationToken);

        _ = RequirePayload(response, "Ok");
    }

    private static async Task<JsonNode> SendAsync(JsonNode request, CancellationToken cancellationToken)
    {
        await using var pipe = new NamedPipeClientStream(
            ".",
            PipeName,
            PipeDirection.InOut,
            PipeOptions.Asynchronous);
        await pipe.ConnectAsync(2500, cancellationToken);

        var requestBytes = Encoding.UTF8.GetBytes(request.ToJsonString());
        await pipe.WriteAsync(requestBytes, cancellationToken);
        await pipe.FlushAsync(cancellationToken);

        var buffer = new byte[8192];
        using var response = new MemoryStream();
        while (true)
        {
            var read = await pipe.ReadAsync(buffer, cancellationToken);
            if (read == 0)
            {
                break;
            }
            response.Write(buffer, 0, read);
            if (!pipe.IsConnected)
            {
                break;
            }
        }

        var responseText = Encoding.UTF8.GetString(response.ToArray());
        return JsonNode.Parse(responseText)
            ?? throw new InvalidOperationException("Service returned empty JSON.");
    }

    private static JsonNode RequirePayload(JsonNode response, string responseKind)
    {
        if (response is JsonValue value
            && value.TryGetValue<string>(out var unitVariant)
            && unitVariant == responseKind)
        {
            return response;
        }

        if (response is not JsonObject obj)
        {
            throw new InvalidOperationException($"Unexpected service response: {response}");
        }

        if (obj["Error"] is JsonObject error)
        {
            var code = error["code"]?.GetValue<string>() ?? "ERROR";
            var message = error["message"]?.GetValue<string>() ?? "Unknown service error";
            throw new InvalidOperationException($"{code}: {message}");
        }

        return obj[responseKind]
            ?? throw new InvalidOperationException($"Expected {responseKind}, got {response}");
    }

    private static string? ReadNullableString(JsonNode? node) =>
        node is JsonValue value && value.TryGetValue<string>(out var text) ? text : null;
}

public sealed record SandboxIdViewModel(string Value);

public sealed record SandboxStatusViewModel(
    SandboxInstanceViewModel? Instance,
    string Health,
    string? ActivePolicyVersion);

public sealed record SandboxInstanceViewModel(
    SandboxIdViewModel Id,
    string DesktopName,
    string ProfileRoot,
    string PolicyVersion)
{
    public static SandboxInstanceViewModel FromJson(JsonObject json) =>
        new(
            new SandboxIdViewModel(json["id"]?.GetValue<string>() ?? string.Empty),
            json["desktop_name"]?.GetValue<string>() ?? string.Empty,
            json["profile_root"]?.GetValue<string>() ?? string.Empty,
            json["policy_version"]?.GetValue<string>() ?? string.Empty);
}

public sealed record SandboxAppViewModel(
    string Id,
    string Name,
    string Executable,
    IReadOnlyList<string> Arguments,
    string? WorkingDirectory)
{
    public static SandboxAppViewModel FromJson(JsonObject json) =>
        new(
            json["id"]?.GetValue<string>() ?? string.Empty,
            json["name"]?.GetValue<string>() ?? string.Empty,
            json["executable"]?.GetValue<string>() ?? string.Empty,
            json["arguments"]?.AsArray().OfType<JsonValue>().Select(value => value.GetValue<string>()).ToList()
                ?? [],
            JsonHelpers.ReadNullableString(json["working_directory"]));
}

internal static class JsonHelpers
{
    public static string? ReadNullableString(JsonNode? node) =>
        node is JsonValue value && value.TryGetValue<string>(out var text) ? text : null;
}
