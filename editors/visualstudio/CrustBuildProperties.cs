using System;
using System.Collections.Immutable;
using System.ComponentModel.Composition;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.VisualStudio.ProjectSystem;
using Microsoft.VisualStudio.ProjectSystem.Build;

namespace Crust.VisualStudio;

[Export(typeof(IProjectGlobalPropertiesProvider))]
[AppliesTo("Crust")]
internal sealed class CrustBuildProperties : StaticGlobalPropertiesProviderBase
{
    [ImportingConstructor]
    public CrustBuildProperties(IProjectCommonServices services) : base(services) { }

    public override Task<IImmutableDictionary<string, string>> GetGlobalPropertiesAsync(CancellationToken cancellationToken)
    {
        string sdk = Environment.GetEnvironmentVariable("CRUST_SDK_PATH") ??
            Path.Combine(Path.GetDirectoryName(typeof(CrustBuildProperties).Assembly.Location)!, "Sdk");
        return Task.FromResult<IImmutableDictionary<string, string>>(
            Empty.PropertiesMap.SetItem("CrustSdkPath", Escape(Path.GetFullPath(sdk)))
                .SetItem("CrustDefaultCompilerPath", Escape(ProjectTool.CompilerPath())));
    }

    internal static string Escape(string value)
    {
        return ProjectTool.EscapeMsBuild(value);
    }
}
