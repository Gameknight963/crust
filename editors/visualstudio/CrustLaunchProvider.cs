using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.IO;
using System.Threading.Tasks;
using Microsoft.VisualStudio.ProjectSystem;
using Microsoft.VisualStudio.ProjectSystem.Debug;
using Microsoft.VisualStudio.ProjectSystem.VS.Debug;

namespace Crust.VisualStudio;

[ExportDebugger(CrustLaunch.SchemaName)]
[AppliesTo("Crust")]
internal sealed class CrustLaunchProvider : DebugLaunchProviderBase
{
    private readonly ConfiguredProject project;

    [ImportingConstructor]
    public CrustLaunchProvider(ConfiguredProject project) : base(project) { this.project = project; }

    public override async Task<bool> CanLaunchAsync(DebugLaunchOptions launchOptions)
    {
        try
        {
            var configuration = await ConfigurationAsync().ConfigureAwait(false);
            var info = await ProjectTool.ReadMetadataAsync(configuration.Compiler, configuration.Manifest).ConfigureAwait(false);
            ProjectTool.ExecutableTarget(info, configuration.Target);
            return true;
        }
        catch (Exception error) when (error is InvalidOperationException || error is IOException
            || error is System.ComponentModel.Win32Exception || error is ArgumentException)
        { return false; }
    }

    public override async Task<IReadOnlyList<IDebugLaunchSettings>> QueryDebugTargetsAsync(DebugLaunchOptions launchOptions)
    {
        var configuration = await ConfigurationAsync().ConfigureAwait(false);
        var info = await ProjectTool.ReadMetadataAsync(configuration.Compiler, configuration.Manifest).ConfigureAwait(false);
        var target = ProjectTool.ExecutableTarget(info, configuration.Target);
        return new IDebugLaunchSettings[] { CrustLaunch.CreateSettings(launchOptions, target,
            configuration.Manifest, configuration.Arguments, configuration.WorkingDirectory, configuration.ProjectDirectory) };
    }

    private async Task<Configuration> ConfigurationAsync()
    {
        var provider = project.Services.ProjectPropertiesProvider
            ?? throw new InvalidOperationException("Visual Studio cannot read the Crust project's launch properties.");
        var properties = provider.GetCommonProperties();
        string directory = Path.GetDirectoryName(project.UnconfiguredProject.FullPath)!;
        string manifest = await properties.GetEvaluatedPropertyValueAsync("CrustManifest");
        string compiler = await properties.GetEvaluatedPropertyValueAsync("CrustCompilerPath");
        return new Configuration {
            ProjectDirectory = directory,
            Manifest = Path.GetFullPath(Path.Combine(directory, string.IsNullOrWhiteSpace(manifest) ? "crust.toml" : manifest)),
            Compiler = ProjectTool.ResolveCompilerPath(string.IsNullOrWhiteSpace(compiler) ? ProjectTool.CompilerPath() : compiler, directory),
            Target = await properties.GetEvaluatedPropertyValueAsync("CrustTarget"),
            Arguments = await properties.GetEvaluatedPropertyValueAsync("CrustRunArguments"),
            WorkingDirectory = await properties.GetEvaluatedPropertyValueAsync("CrustRunWorkingDirectory")
        };
    }

    private sealed class Configuration
    {
        public string ProjectDirectory = "";
        public string Manifest = "";
        public string Compiler = "";
        public string Target = "";
        public string Arguments = "";
        public string WorkingDirectory = "";
    }
}

internal static class CrustLaunch
{
    internal const string SchemaName = "CrustLocal";

    internal static DebugLaunchSettings CreateSettings(DebugLaunchOptions options, TargetMetadata target,
        string manifest, string arguments, string workingDirectory, string projectDirectory)
    {
        if (target.kind != "executable") throw new InvalidOperationException("Select an executable target to run.");
        string directory = string.IsNullOrWhiteSpace(workingDirectory)
            ? Path.GetDirectoryName(manifest)! : Path.GetFullPath(Path.Combine(projectDirectory, workingDirectory));
        if (!Directory.Exists(directory)) throw new DirectoryNotFoundException("The working directory does not exist: " + directory);
        // CPS can query the planned executable before its first build.
        return new DebugLaunchSettings(options) {
            LaunchOperation = DebugLaunchOperation.CreateProcess,
            LaunchDebugEngineGuid = DebuggerEngines.NativeOnlyEngine,
            Executable = target.output,
            Arguments = arguments ?? "",
            CurrentDirectory = directory
        };
    }
}
