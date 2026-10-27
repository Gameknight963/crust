using System;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading.Tasks;
using System.Web.Script.Serialization;

namespace Crust.VisualStudio;

public sealed class ProjectMetadata
{
    public int formatVersion { get; set; }
    public string name { get; set; } = "";
    public TargetMetadata[] targets { get; set; } = Array.Empty<TargetMetadata>();
}

public sealed class TargetMetadata
{
    public string name { get; set; } = "";
    public string kind { get; set; } = "";
    public string output { get; set; } = "";
}

public sealed class ToolResult
{
    public int ExitCode { get; }
    public string Output { get; }
    public string Error { get; }
    public ToolResult(int exitCode, string output, string error)
    { ExitCode = exitCode; Output = output; Error = error; }
}

public static class ProjectTool
{
    public static string? FindManifest(string filename)
    {
        for (var directory = new DirectoryInfo(Path.GetDirectoryName(Path.GetFullPath(filename))!);
            directory != null; directory = directory.Parent)
        {
            string manifest = Path.Combine(directory.FullName, "crust.toml");
            if (File.Exists(manifest)) return manifest;
        }
        return null;
    }

    // ProcessStartInfo.Arguments uses Windows argv quoting, including doubled
    // trailing backslashes and escaped quotes. No shell interprets these paths.
    public static string QuoteArgument(string value)
    {
        var quoted = new StringBuilder("\"");
        int slashes = 0;
        foreach (char character in value)
        {
            if (character == '\\') { slashes++; continue; }
            if (character == '"') quoted.Append('\\', slashes * 2 + 1).Append('"');
            else quoted.Append('\\', slashes).Append(character);
            slashes = 0;
        }
        return quoted.Append('\\', slashes * 2).Append('"').ToString();
    }

    public static string CompilerPath()
    {
        string configured = Environment.GetEnvironmentVariable("CRUST_COMPILER_PATH") ?? "";
        if (!string.IsNullOrWhiteSpace(configured)) return Path.GetFullPath(configured);
        string server = Environment.GetEnvironmentVariable("CRUST_LSP_PATH") ?? "";
        if (!string.IsNullOrWhiteSpace(server))
        {
            string sibling = Path.Combine(Path.GetDirectoryName(Path.GetFullPath(server))!, "crust.exe");
            if (File.Exists(sibling)) return sibling;
        }
        return Path.Combine(Path.GetDirectoryName(typeof(ProjectTool).Assembly.Location)!, "Server", "crust.exe");
    }

    public static ProcessStartInfo ConsoleStartInfo(TargetMetadata target, string manifest)
    {
        string runner = Path.Combine(Path.GetDirectoryName(typeof(ProjectTool).Assembly.Location)!, "Tools", "Crust.Console.exe");
        string directory = Path.GetDirectoryName(manifest)!;
        var start = new ProcessStartInfo(runner, QuoteArgument(target.output) + " " + QuoteArgument(directory)) {
            WorkingDirectory = directory, UseShellExecute = false
        };
        return start;
    }

    public static async Task<ToolResult> ExecuteAsync(string compiler, string manifest, string command, string? target = null)
    {
        if (command != "metadata" && command != "build" && command != "check")
            throw new ArgumentException("Unknown Crust project command", nameof(command));
        string arguments = command + " --project " + QuoteArgument(manifest);
        if (target != null) arguments += " --target " + QuoteArgument(target);
        using (var process = new Process {
            StartInfo = new ProcessStartInfo(compiler, arguments) {
                WorkingDirectory = Path.GetDirectoryName(manifest)!,
                UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8
            }
        })
        {
            if (!process.Start()) throw new InvalidOperationException("Could not start the Crust compiler");
            process.StandardInput.Close();
            Task<string> output = process.StandardOutput.ReadToEndAsync();
            Task<string> error = process.StandardError.ReadToEndAsync();
            await Task.WhenAll(output, error).ConfigureAwait(false);
            await Task.Run(() => process.WaitForExit()).ConfigureAwait(false);
            return new ToolResult(process.ExitCode,
                await output.ConfigureAwait(false), await error.ConfigureAwait(false));
        }
    }

    public static async Task<ProjectMetadata> ReadMetadataAsync(string compiler, string manifest)
    {
        ToolResult result = await ExecuteAsync(compiler, manifest, "metadata").ConfigureAwait(false);
        if (result.ExitCode != 0) throw new InvalidOperationException(result.Error.Trim());
        ProjectMetadata info;
        try
        {
            info = new JavaScriptSerializer { MaxJsonLength = 16 * 1024 * 1024 }
                .Deserialize<ProjectMetadata>(result.Output);
        }
        catch (ArgumentException error)
        { throw new InvalidOperationException("The Crust compiler returned invalid project metadata", error); }
        if (info == null || info.formatVersion != 1 || string.IsNullOrEmpty(info.name)
            || info.targets == null || info.targets.Any(target => target == null
                || string.IsNullOrEmpty(target.name) || string.IsNullOrEmpty(target.kind)
                || string.IsNullOrEmpty(target.output)))
            throw new InvalidOperationException("Unsupported Crust project metadata. Update the compiler and extension together.");
        foreach (TargetMetadata target in info.targets)
        {
            target.output = DisplayPath(target.output);
        }
        return info;
    }

    private static string DisplayPath(string path)
    {
        if (path.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase)) return @"\\" + path.Substring(8);
        return path.StartsWith(@"\\?\", StringComparison.Ordinal) ? path.Substring(4) : path;
    }
}
