using System;
using System.Diagnostics;
using System.Text;

[assembly: System.Runtime.Versioning.TargetFramework(".NETFramework,Version=v4.7.2")]

internal static class ConsoleRunner
{
    private static int Main(string[] args)
    {
        int result;
        try
        {
            if (args.Length != 2) throw new ArgumentException("Expected an executable and working directory");
            Console.InputEncoding = new UTF8Encoding(false);
            Console.OutputEncoding = new UTF8Encoding(false);
            using (var process = Process.Start(new ProcessStartInfo(args[0]) {
                WorkingDirectory = args[1], UseShellExecute = false
            }))
            {
                if (process == null) throw new InvalidOperationException("Could not start the program");
                process.WaitForExit();
                result = process.ExitCode;
            }
            Console.WriteLine("\nProcess exited with code " + result);
        }
        catch (Exception error)
        { Console.Error.WriteLine(error.Message); result = 1; }
        if (!Console.IsInputRedirected)
        {
            Console.WriteLine("Press any key to close this window.");
            Console.ReadKey(true);
        }
        return result;
    }
}
