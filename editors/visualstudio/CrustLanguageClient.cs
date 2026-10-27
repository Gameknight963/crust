using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.ComponentModel.Composition;
using System.Diagnostics;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.VisualStudio.LanguageServer.Client;
using Microsoft.VisualStudio.Threading;
using Microsoft.VisualStudio.Utilities;

namespace Crust.VisualStudio;

[Export(typeof(ILanguageClient))]
[ContentType("crust")]
public sealed class CrustLanguageClient : ILanguageClient, IDisposable
{
    private Process? server;

    public string Name => "crust";
    public IEnumerable<string> ConfigurationSections => Array.Empty<string>();
    public object InitializationOptions => new { visualStudioClassifications = true };
    public IEnumerable<string> FilesToWatch => new[] { "**/*.cr", "**/crust.toml" };
    public bool ShowNotificationOnInitializeFailed => true;
    public event AsyncEventHandler<EventArgs>? StartAsync;
#pragma warning disable CS0067 // Required by ILanguageClient. VS owns normal shutdown.
    public event AsyncEventHandler<EventArgs>? StopAsync;
#pragma warning restore CS0067

    public async Task<Connection?> ActivateAsync(CancellationToken token)
    {
        await TaskScheduler.Default;
        token.ThrowIfCancellationRequested();
        Dispose();
        string configured = Environment.GetEnvironmentVariable("CRUST_LSP_PATH") ?? "";
        string executable = string.IsNullOrWhiteSpace(configured)
            ? Path.Combine(Path.GetDirectoryName(typeof(CrustLanguageClient).Assembly.Location)!, "Server", "crust-lsp.exe")
            : Path.GetFullPath(configured);
        if (!File.Exists(executable))
            throw new FileNotFoundException("The Crust language server was not found. Reinstall the extension or check CRUST_LSP_PATH.", executable);

        var process = new Process
        {
            StartInfo = new ProcessStartInfo(executable, "--stdio")
            {
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true
            }
        };
        process.ErrorDataReceived += (_, args) =>
        {
            if (args.Data != null) Debug.WriteLine("Crust: " + args.Data);
        };
        try
        {
            if (!process.Start()) throw new InvalidOperationException("Could not start the Crust language server");
            process.BeginErrorReadLine();
            token.ThrowIfCancellationRequested();
            Interlocked.Exchange(ref server, process);
            return new Connection(process.StandardOutput.BaseStream, process.StandardInput.BaseStream);
        }
        catch
        {
            Stop(process);
            throw;
        }
    }

    public async Task OnLoadedAsync()
    {
        var handler = StartAsync;
        if (handler != null) await handler.InvokeAsync(this, EventArgs.Empty);
    }

    public Task OnServerInitializedAsync() => Task.CompletedTask;

    public Task<InitializationFailureContext?> OnServerInitializeFailedAsync(ILanguageClientInitializationInfo initializationState)
    {
        Dispose();
        return Task.FromResult<InitializationFailureContext?>(new InitializationFailureContext
        {
            FailureMessage = "Crust language server initialization failed. Check CRUST_LSP_PATH or reinstall the extension."
        });
    }

    public void Dispose()
    {
        var process = Interlocked.Exchange(ref server, null);
        if (process != null) Stop(process);
    }

    private static void Stop(Process process)
    {
        try { if (!process.HasExited) process.Kill(); }
        catch (InvalidOperationException) { }
        catch (Win32Exception error) { Debug.WriteLine("Crust: " + error.Message); }
        finally { process.Dispose(); }
    }
}

public static class CrustContentType
{
    [Export, Name("crust"), BaseDefinition(CodeRemoteContentDefinition.CodeRemoteContentTypeName)]
    public static ContentTypeDefinition Definition = null!;

    [Export, FileExtension(".cr"), ContentType("crust")]
    public static FileExtensionToContentTypeDefinition Extension = null!;
}
