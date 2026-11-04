using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Threading.Tasks;

// Test-only stdio relay. Keep the IDE's real LSP traffic with the live test's
// diagnostics, including completion requests and responses.
internal static class CrustServerProbe
{
    public static int Main()
    {
        string directory = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        string output = Environment.GetEnvironmentVariable("CRUST_LIVE_CLASSIFICATIONS");
        var start = new ProcessStartInfo(Path.Combine(directory, "crust-lsp-real.exe"), "--stdio") {
            UseShellExecute = false, CreateNoWindow = true,
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true
        };
        using (var server = Process.Start(start))
        {
            _ = Relay(Console.OpenStandardInput(), server.StandardInput.BaseStream, output + ".lsp-input");
            var stdout = Relay(server.StandardOutput.BaseStream, Console.OpenStandardOutput(), output + ".lsp-output");
            var stderr = Relay(server.StandardError.BaseStream, Console.OpenStandardError(), output + ".lsp-errors");
            server.WaitForExit();
            Task.WaitAll(stdout, stderr);
            return server.ExitCode;
        }
    }

    private static Task Relay(Stream source, Stream destination, string log)
    {
        return Task.Run(async () => {
            using (var trace = new FileStream(log, FileMode.Create, FileAccess.Write, FileShare.ReadWrite))
            {
                var bytes = new byte[8192];
                int count;
                while ((count = await source.ReadAsync(bytes, 0, bytes.Length)) != 0)
                {
                    await trace.WriteAsync(bytes, 0, count);
                    await trace.FlushAsync();
                    await destination.WriteAsync(bytes, 0, count);
                    await destination.FlushAsync();
                }
                destination.Close();
            }
        });
    }
}
