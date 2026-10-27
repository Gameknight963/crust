using System;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;
using Microsoft.Build.Framework;
using Microsoft.Build.Utilities;

namespace Crust.Build;

public sealed class CrustCompile : ToolTask
{
    private static readonly Regex Diagnostic = new Regex(@"^(.+):(\d+):(\d+):\s*(.*)$", RegexOptions.CultureInvariant);
    [Required] public string CompilerPath { get; set; } = "";
    [Required] public string Manifest { get; set; } = "";
    [Required] public string ProjectDirectory { get; set; } = "";
    public string Command { get; set; } = "build";
    public string Target { get; set; } = "";

    protected override string ToolName => "crust";
    protected override Encoding StandardOutputEncoding => Encoding.UTF8;
    protected override Encoding StandardErrorEncoding => Encoding.UTF8;
    private string ManifestPath => Path.GetFullPath(Path.Combine(ProjectDirectory, Manifest));
    protected override string GetWorkingDirectory() => Path.GetDirectoryName(ManifestPath)!;

    protected override bool ValidateParameters()
    {
        if (Command != "build" && Command != "check")
        { Log.LogError("Crust MSBuild commands are build and check."); return false; }
        return base.ValidateParameters();
    }

    protected override string GenerateFullPathToTool() => FindCompiler(CompilerPath, ProjectDirectory);

    internal static string FindCompiler(string path, string directory)
    {
        return VisualStudio.ProjectTool.ResolveCompilerPath(path, directory);
    }

    protected override string GenerateCommandLineCommands()
    {
        return Command + " --project " + VisualStudio.ProjectTool.QuoteArgument(ManifestPath)
            + (string.IsNullOrEmpty(Target) ? "" : " --target " + VisualStudio.ProjectTool.QuoteArgument(Target));
    }

    protected override void LogEventsFromTextOutput(string line, MessageImportance importance)
    {
        Match match = Diagnostic.Match(line);
        if (match.Success && int.TryParse(match.Groups[2].Value, out int number)
            && int.TryParse(match.Groups[3].Value, out int column))
        {
            Log.LogError("", "CRUST", "", match.Groups[1].Value, number, column, 0, 0, match.Groups[4].Value);
            return;
        }
        base.LogEventsFromTextOutput(line, importance);
    }
}
