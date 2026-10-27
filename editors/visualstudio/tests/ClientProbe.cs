using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;

internal static class ClientProbe
{
    private static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    private static int startEvents;

    public static int Main(string[] args)
    {
        try
        {
            Run(args[0], args[1]);
            Console.WriteLine("Visual Studio client activation and bundled-server protocol checks passed");
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error);
            return 1;
        }
    }

    private static void Run(string package, string visualStudio)
    {
        string ide = Path.Combine(visualStudio, "Common7", "IDE");
        string[] directories = {
            Path.Combine(ide, "CommonExtensions", "Microsoft", "LanguageServer"),
            Path.Combine(ide, "CommonExtensions", "Microsoft", "Editor"),
            Path.Combine(ide, "PublicAssemblies", "Microsoft.VisualStudio.Threading.17.x"),
            Path.Combine(ide, "PublicAssemblies"),
            Path.Combine(ide, "PrivateAssemblies")
        };
        AppDomain.CurrentDomain.AssemblyResolve += (_, eventArgs) =>
        {
            var requested = new AssemblyName(eventArgs.Name);
            var candidates = directories.Select(directory => Path.Combine(directory, requested.Name + ".dll"))
                .Where(File.Exists).ToArray();
            string exact = candidates.FirstOrDefault(path => AssemblyName.GetAssemblyName(path).Version == requested.Version);
            string candidate = exact ?? candidates.FirstOrDefault();
            return candidate == null ? null : Assembly.LoadFrom(candidate);
        };
        Environment.SetEnvironmentVariable("CRUST_LSP_PATH", null);
        var assembly = Assembly.LoadFrom(Path.Combine(package, "Crust.VisualStudio.dll"));
        var type = assembly.GetType("Crust.VisualStudio.CrustLanguageClient", true);
        using (var client = (IDisposable)Activator.CreateInstance(type))
        {
            Assert((string)type.GetProperty("Name").GetValue(client) == "crust", "language ID");
            var options = type.GetProperty("InitializationOptions").GetValue(client);
            Assert((bool)options.GetType().GetProperty("visualStudioClassifications").GetValue(options), "native classifications");
            var start = type.GetEvent("StartAsync");
            var handler = Delegate.CreateDelegate(start.EventHandlerType, typeof(ClientProbe).GetMethod("Started", BindingFlags.NonPublic | BindingFlags.Static));
            start.AddEventHandler(client, handler);
            var loaded = (Task)type.GetMethod("OnLoadedAsync").Invoke(client, null);
            Assert(loaded.Wait(TimeSpan.FromSeconds(10)) && startEvents == 1, "client start event");
            start.RemoveEventHandler(client, handler);
            var activation = (Task)type.GetMethod("ActivateAsync").Invoke(client, new object[] { CancellationToken.None });
            Assert(activation.Wait(TimeSpan.FromSeconds(10)), "client activation timeout");
            var connection = activation.GetType().GetProperty("Result").GetValue(activation);
            var reader = (Stream)connection.GetType().GetProperty("Reader").GetValue(connection);
            var writer = (Stream)connection.GetType().GetProperty("Writer").GetValue(connection);

            Request(writer, 1, "initialize", new { capabilities = new object(), initializationOptions = options });
            var initialized = Receive(reader);
            Assert(!initialized.ContainsKey("error"), "initialization error");
            var capabilities = Object(Object(initialized["result"])["capabilities"]);
            var legend = Object(Object(capabilities["semanticTokensProvider"])["legend"]);
            Assert(((object[])legend["tokenTypes"]).Contains("struct name"), "Visual Studio token legend");
            Notify(writer, "initialized", new object());
            Notify(writer, "textDocument/didOpen", new { textDocument = new {
                uri = "untitled:client-probe.cr", languageId = "crust", version = 1,
                text = "int main() { return missing; }"
            }});
            var diagnostics = Receive(reader);
            Assert((string)diagnostics["method"] == "textDocument/publishDiagnostics", "diagnostics method");
            var errors = (object[])Object(diagnostics["params"])["diagnostics"];
            Assert(errors.Length == 1 && ((string)Object(errors[0])["message"]).Contains("missing"), "unsaved compiler diagnostic");
            Notify(writer, "textDocument/didChange", new {
                textDocument = new { uri = "untitled:client-probe.cr", version = 2 },
                contentChanges = new[] { new { text = "int main() { return 0; }" } }
            });
            diagnostics = Receive(reader);
            Assert(((object[])Object(diagnostics["params"])["diagnostics"]).Length == 0, "diagnostic recovery");
            Request(writer, 2, "shutdown", null);
            var shutdown = Receive(reader);
            Assert(!shutdown.ContainsKey("error") && Convert.ToInt32(shutdown["id"]) == 2, "shutdown response");
            Notify(writer, "exit", null);
            var closed = Task.Run(() => reader.ReadByte());
            Assert(closed.Wait(TimeSpan.FromSeconds(10)) && closed.Result == -1, "server shutdown");
        }
    }

    private static Dictionary<string, object> Object(object value) => (Dictionary<string, object>)value;

    private static Task Started(object sender, EventArgs args)
    {
        Interlocked.Increment(ref startEvents);
        return Task.CompletedTask;
    }

    private static void Assert(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    private static void Request(Stream stream, int id, string method, object parameters)
        => Send(stream, new { jsonrpc = "2.0", id, method, @params = parameters });

    private static void Notify(Stream stream, string method, object parameters)
        => Send(stream, new { jsonrpc = "2.0", method, @params = parameters });

    private static void Send(Stream stream, object message)
    {
        byte[] body = Encoding.UTF8.GetBytes(Json.Serialize(message));
        byte[] header = Encoding.ASCII.GetBytes("Content-Length: " + body.Length + "\r\n\r\n");
        stream.Write(header, 0, header.Length);
        stream.Write(body, 0, body.Length);
        stream.Flush();
    }

    private static Dictionary<string, object> Receive(Stream stream)
    {
        var task = Task.Run(() => Read(stream));
        Assert(task.Wait(TimeSpan.FromSeconds(10)), "server response timeout");
        return Object(Json.DeserializeObject(task.Result));
    }

    private static string Read(Stream stream)
    {
        var bytes = new List<byte>();
        while (bytes.Count < 8192)
        {
            int value = stream.ReadByte();
            if (value < 0) throw new EndOfStreamException();
            bytes.Add((byte)value);
            int length = bytes.Count;
            if (length >= 4 && bytes[length - 4] == 13 && bytes[length - 3] == 10 && bytes[length - 2] == 13 && bytes[length - 1] == 10)
            {
                string header = Encoding.ASCII.GetString(bytes.ToArray());
                int size = int.Parse(header.Split(new[] { "\r\n" }, StringSplitOptions.RemoveEmptyEntries)
                    .Single(line => line.StartsWith("Content-Length:", StringComparison.OrdinalIgnoreCase)).Substring(15).Trim());
                var body = new byte[size];
                int offset = 0;
                while (offset < size)
                {
                    int read = stream.Read(body, offset, size - offset);
                    if (read == 0) throw new EndOfStreamException();
                    offset += read;
                }
                return Encoding.UTF8.GetString(body);
            }
        }
        throw new InvalidDataException("LSP header exceeds the test limit");
    }
}
