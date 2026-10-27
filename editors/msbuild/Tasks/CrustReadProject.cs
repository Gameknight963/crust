using System;
using System.IO;
using System.Linq;
using Crust.VisualStudio;
using Microsoft.Build.Framework;
using Microsoft.Build.Utilities;

namespace Crust.Build;

public sealed class CrustReadProject : Task
{
    [Required] public string CompilerPath { get; set; } = "";
    [Required] public string Manifest { get; set; } = "";
    [Required] public string ProjectDirectory { get; set; } = "";
    public string Target { get; set; } = "";
    [Output] public string ProjectName { get; private set; } = "";
    [Output] public ITaskItem[] Outputs { get; private set; } = Array.Empty<ITaskItem>();

    public override bool Execute()
    {
        try
        {
            var info = ProjectTool.ReadMetadataAsync(CrustCompile.FindCompiler(CompilerPath, ProjectDirectory),
                Path.GetFullPath(Path.Combine(ProjectDirectory, Manifest)))
                .GetAwaiter().GetResult();
            var selected = info.targets.Where(item => string.IsNullOrEmpty(Target) || item.name == Target).ToArray();
            if (selected.Length == 0) throw new InvalidOperationException("Unknown Crust target '" + Target + "'");
            ProjectName = info.name;
            Outputs = selected.Select(item => {
                var output = new TaskItem(ProjectTool.EscapeMsBuild(item.output));
                output.SetMetadata("TargetName", item.name);
                output.SetMetadata("TargetKind", item.kind);
                ((ITaskItem2)output).SetMetadataValueLiteral("TargetPath", item.output);
                return (ITaskItem)output;
            }).ToArray();
            return true;
        }
        catch (Exception error) when (error is InvalidOperationException || error is IOException
            || error is System.ComponentModel.Win32Exception || error is ArgumentException)
        { Log.LogErrorFromException(error, false); return false; }
    }
}
