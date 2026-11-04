using System.ComponentModel.Composition;
using StreamJsonRpc;

namespace Crust.VisualStudio;

[Export, PartCreationPolicy(CreationPolicy.Shared)]
public sealed class HoverConnection
{
    internal volatile JsonRpc? Rpc;
    internal volatile bool Ready;
}
