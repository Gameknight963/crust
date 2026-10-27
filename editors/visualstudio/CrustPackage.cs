using System;
using System.ComponentModel.Design;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;
using Microsoft.VisualStudio;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;
using Microsoft.VisualStudio.Threading;

namespace Crust.VisualStudio;

[Guid(PackageGuid)]
[PackageRegistration(UseManagedResourcesOnly = true, AllowsBackgroundLoading = true)]
[ProvideMenuResource("Menus.ctmenu", 1)]
public sealed class CrustPackage : AsyncPackage
{
    public const string PackageGuid = "f9801e0c-892b-4886-8045-29fe53df7965";
    private static readonly Guid Commands = new Guid("df604453-c0fa-4bcb-8d82-813306bf15b2");
    private static readonly Guid OutputGuid = new Guid("02b73377-3143-4dcc-a813-6a78e2865d5c");
    private static readonly Regex Diagnostic = new Regex(@"^(.+):(\d+):(\d+):\s*(.*)$");
    private IVsOutputWindowPane? output;
    private ErrorListProvider? errors;
    private bool busy;

    protected override async Task InitializeAsync(CancellationToken cancellationToken, IProgress<ServiceProgressData> progress)
    {
        var commands = await GetServiceAsync(typeof(IMenuCommandService)) as OleMenuCommandService;
        object? windowService = await GetServiceAsync(typeof(SVsOutputWindow));
        await JoinableTaskFactory.SwitchToMainThreadAsync(cancellationToken);
        var window = windowService as IVsOutputWindow;
        errors = new ErrorListProvider(this) { ProviderName = "Crust", ProviderGuid = OutputGuid };
        if (window != null)
        {
            Guid pane = OutputGuid;
            ErrorHandler.ThrowOnFailure(window.CreatePane(ref pane, "Crust", 1, 1));
            ErrorHandler.ThrowOnFailure(window.GetPane(ref pane, out output));
        }
        if (commands == null) return;
        AddCommand(commands, 0x0100, "build");
        AddCommand(commands, 0x0101, "check");
        AddCommand(commands, 0x0102, "run");
    }

    private void AddCommand(OleMenuCommandService commands, int id, string action)
    {
        ThreadHelper.ThrowIfNotOnUIThread();
        var command = new OleMenuCommand((sender, args) => {
            _ = JoinableTaskFactory.RunAsync(() => ExecuteAsync(action));
        }, new CommandID(Commands, id));
        command.BeforeQueryStatus += (_, __) => command.Enabled = !busy;
        commands.AddCommand(command);
    }

    private async Task ExecuteAsync(string action)
    {
        await JoinableTaskFactory.SwitchToMainThreadAsync();
        if (busy) return;
        busy = true;
        try
        {
            var dte = await GetServiceAsync(typeof(EnvDTE.DTE)) as EnvDTE.DTE;
            await JoinableTaskFactory.SwitchToMainThreadAsync();
            if (dte == null) throw new InvalidOperationException("Visual Studio automation is unavailable.");
            string filename = dte.ActiveDocument?.FullName ?? "";
            if (string.IsNullOrEmpty(filename))
                throw new InvalidOperationException("Open a Crust source file or crust.toml to select a project.");
            dte.Documents.SaveAll();
            errors!.Tasks.Clear();
            output?.Clear();
            output?.Activate();
            dte.ExecuteCommand("View.Output");
            Write("Loading Crust project...\n");
            await TaskScheduler.Default;
            string manifest = ProjectTool.FindManifest(filename)
                ?? throw new InvalidOperationException("No crust.toml was found above the active file.");
            string compiler = ProjectTool.CompilerPath();
            ProjectMetadata info = await ProjectTool.ReadMetadataAsync(compiler, manifest);
            await JoinableTaskFactory.SwitchToMainThreadAsync();
            TargetMetadata? selected = null;
            if (action == "run")
            {
                TargetMetadata[] executables = info.targets.Where(target => target.kind == "executable").ToArray();
                if (executables.Length == 0) throw new InvalidOperationException("This project has no executable target.");
                selected = executables.Length == 1 ? executables[0] : SelectTarget(executables);
                if (selected == null) return;
            }
            Write($"{info.name}: {(action == "run" ? "build " + selected!.name : action)}\n");
            await TaskScheduler.Default;
            ToolResult result = await ProjectTool.ExecuteAsync(compiler, manifest,
                action == "run" ? "build" : action, selected?.name);
            await JoinableTaskFactory.SwitchToMainThreadAsync();
            Write(result.Output);
            Write(result.Error);
            AddErrors(result.Error);
            Write($"Crust exited with code {result.ExitCode}\n");
            if (result.ExitCode != 0) { if (errors.Tasks.Count > 0) errors.Show(); return; }
            if (selected != null)
            {
                string runner = Path.Combine(Path.GetDirectoryName(typeof(CrustPackage).Assembly.Location)!, "Tools", "Crust.Console.exe");
                using (Process.Start(new ProcessStartInfo(runner,
                    ProjectTool.QuoteArgument(selected.output) + " " + ProjectTool.QuoteArgument(Path.GetDirectoryName(manifest)!)) {
                        WorkingDirectory = Path.GetDirectoryName(manifest)!, UseShellExecute = false
                    })) { }
            }
        }
        catch (Exception error)
        {
            await JoinableTaskFactory.SwitchToMainThreadAsync();
            Write(error.Message + "\n");
            VsShellUtilities.ShowMessageBox(this, error.Message, "Crust",
                OLEMSGICON.OLEMSGICON_WARNING, OLEMSGBUTTON.OLEMSGBUTTON_OK, OLEMSGDEFBUTTON.OLEMSGDEFBUTTON_FIRST);
        }
        finally
        {
            await JoinableTaskFactory.SwitchToMainThreadAsync();
            busy = false;
        }
    }

    private void Write(string text)
    {
        ThreadHelper.ThrowIfNotOnUIThread();
        output?.OutputStringThreadSafe(text.Replace("\r\n", "\n").Replace("\n", "\r\n"));
    }

    private void AddErrors(string text)
    {
        ThreadHelper.ThrowIfNotOnUIThread();
        foreach (string line in text.Split('\n'))
        {
            Match match = Diagnostic.Match(line.TrimEnd('\r'));
            if (!match.Success || !int.TryParse(match.Groups[2].Value, out int row)
                || !int.TryParse(match.Groups[3].Value, out int column)) continue;
            var task = new ErrorTask {
                Text = match.Groups[4].Value, Document = match.Groups[1].Value,
                Line = Math.Max(0, row - 1), Column = Math.Max(0, column - 1),
                Category = TaskCategory.BuildCompile, ErrorCategory = TaskErrorCategory.Error
            };
            task.Navigate += (sender, args) => {
                ThreadHelper.ThrowIfNotOnUIThread();
                VsShellUtilities.OpenDocument(this, task.Document, VSConstants.LOGVIEWID_Code,
                    out _, out _, out _, out var view);
                view?.SetCaretPos(task.Line, task.Column);
                view?.CenterLines(task.Line, 1);
            };
            errors!.Tasks.Add(task);
        }
    }

    private TargetMetadata? SelectTarget(TargetMetadata[] targets)
    {
        ThreadHelper.ThrowIfNotOnUIThread();
        using (var dialog = new Form { Text = "Run Crust target", Width = 360, Height = 150,
            FormBorderStyle = FormBorderStyle.FixedDialog, StartPosition = FormStartPosition.CenterParent,
            MaximizeBox = false, MinimizeBox = false, AutoScaleMode = AutoScaleMode.Dpi })
        using (var choices = new ComboBox { Left = 16, Top = 16, Width = 310, DropDownStyle = ComboBoxStyle.DropDownList })
        using (var run = new Button { Text = "Run", DialogResult = DialogResult.OK, Left = 170, Top = 56 })
        using (var cancel = new Button { Text = "Cancel", DialogResult = DialogResult.Cancel, Left = 252, Top = 56 })
        {
            choices.Items.AddRange(targets.Select(target => target.name).ToArray());
            choices.SelectedIndex = 0;
            dialog.Controls.AddRange(new Control[] { choices, run, cancel });
            dialog.AcceptButton = run;
            dialog.CancelButton = cancel;
            var shell = GetService(typeof(SVsUIShell)) as IVsUIShell;
            IntPtr owner = IntPtr.Zero;
            shell?.GetDialogOwnerHwnd(out owner);
            return dialog.ShowDialog(new WindowOwner(owner)) == DialogResult.OK ? targets[choices.SelectedIndex] : null;
        }
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing) errors?.Dispose();
        base.Dispose(disposing);
    }

    private sealed class WindowOwner : IWin32Window
    {
        public IntPtr Handle { get; }
        public WindowOwner(IntPtr handle) { Handle = handle; }
    }
}
