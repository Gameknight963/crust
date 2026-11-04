using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

public static class CrustLiveIde
{
    [ComImport, Guid("00000016-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMessageFilter
    {
        [PreserveSig] int HandleInComingCall(int type, IntPtr caller, int tick, IntPtr info);
        [PreserveSig] int RetryRejectedCall(IntPtr callee, int tick, int rejectType);
        [PreserveSig] int MessagePending(IntPtr callee, int tick, int pendingType);
    }
    private sealed class AutomationFilter : IMessageFilter
    {
        public int HandleInComingCall(int type, IntPtr caller, int tick, IntPtr info) { return 0; }
        public int RetryRejectedCall(IntPtr callee, int tick, int rejectType) { return rejectType == 2 && tick < 30000 ? 100 : -1; }
        public int MessagePending(IntPtr callee, int tick, int pendingType) { return 2; }
    }
    [DllImport("ole32.dll")] private static extern int CoRegisterMessageFilter(IMessageFilter filter, out IMessageFilter previous);
    [DllImport("ole32.dll")] private static extern int GetRunningObjectTable(int reserved, out IRunningObjectTable table);
    [DllImport("ole32.dll")] private static extern int CreateBindCtx(int reserved, out IBindCtx context);
    private static IMessageFilter previousFilter;
    private static IMessageFilter filter;
    public static void RegisterFilter() { filter = new AutomationFilter(); Marshal.ThrowExceptionForHR(CoRegisterMessageFilter(filter, out previousFilter)); }
    public static void RestoreFilter() { IMessageFilter ignored; CoRegisterMessageFilter(previousFilter, out ignored); filter = null; }

    public static object Find(int processId)
    {
        IRunningObjectTable table; IBindCtx context;
        Marshal.ThrowExceptionForHR(GetRunningObjectTable(0, out table));
        Marshal.ThrowExceptionForHR(CreateBindCtx(0, out context));
        IEnumMoniker enumerator; table.EnumRunning(out enumerator);
        var monikers = new IMoniker[1];
        try
        {
            while (enumerator.Next(1, monikers, IntPtr.Zero) == 0)
            {
                string name; monikers[0].GetDisplayName(context, null, out name);
                if (name.StartsWith("!VisualStudio.DTE.") && name.EndsWith(":" + processId))
                {
                    object value; table.GetObject(monikers[0], out value); return value;
                }
                Marshal.ReleaseComObject(monikers[0]);
            }
            return null;
        }
        finally { Marshal.ReleaseComObject(enumerator); Marshal.ReleaseComObject(context); Marshal.ReleaseComObject(table); }
    }

    public static string CreateProject(object raw, string root, string template)
    {
        var dte = (EnvDTE.DTE)raw;
        dte.SuppressUI = true;
        dte.Solution.Create(root, "probe");
        dte.Solution.AddFromTemplate(template, root + "/project", "Probe", false);
        var project = dte.Solution.Projects.Item(1);
        // Exercise the initial startup selection, as a newly created project does.
        dte.Solution.SaveAs(root + "/probe.sln");
        return project.FullName;
    }
    public static string ProjectItems(object raw)
    {
        var project = ((EnvDTE.DTE)raw).Solution.Projects.Item(1);
        var paths = new System.Collections.Generic.List<string>();
        CollectItems(project.ProjectItems, "", paths);
        return string.Join("\n", paths);
    }
    private static void CollectItems(EnvDTE.ProjectItems items, string prefix, System.Collections.Generic.List<string> paths)
    {
        if (items == null) return;
        foreach (EnvDTE.ProjectItem item in items)
        {
            string path = prefix + item.Name;
            paths.Add(path);
            CollectItems(item.ProjectItems, path + "/", paths);
        }
    }
    public static void Edit(object raw, string file, string text)
    {
        var document = ((EnvDTE.DTE)raw).ItemOperations.OpenFile(file).Document;
        var buffer = (EnvDTE.TextDocument)document.Object("TextDocument");
        var edit = buffer.StartPoint.CreateEditPoint();
        edit.ReplaceText(buffer.EndPoint, text, 0);
        document.Save();
    }
    public static void Build(object raw)
    {
        var build = ((EnvDTE.DTE)raw).Solution.SolutionBuild;
        build.Build(true);
        if (build.LastBuildInfo != 0) throw new Exception("Visual Studio build failed: " + Output(raw, "Build"));
    }
    public static string Output(object raw, string pane)
    {
        var document = ((EnvDTE80.DTE2)raw).ToolWindows.OutputWindow.OutputWindowPanes.Item(pane).TextDocument;
        return document.StartPoint.CreateEditPoint().GetText(document.EndPoint);
    }
    public static void Breakpoint(object raw, string file, int line) { ((EnvDTE.DTE)raw).Debugger.Breakpoints.Add("", file, line); }
    public static void ClearBreakpoints(object raw)
    {
        var points = ((EnvDTE.DTE)raw).Debugger.Breakpoints;
        while (points.Count > 0) points.Item(1).Delete();
    }
    public static bool CanStart(object raw) { return ((EnvDTE.DTE)raw).Commands.Item("Debug.Start", 0).IsAvailable; }
    public static string Mode(object raw) { return ((EnvDTE.DTE)raw).Debugger.CurrentMode.ToString(); }
    public static string Value(object raw, string expression) { return ((EnvDTE.DTE)raw).Debugger.GetExpression(expression, true, 1000).Value; }
    public static int Line(object raw) { return (int)((EnvDTE90a.StackFrame2)((EnvDTE.DTE)raw).Debugger.CurrentStackFrame).LineNumber; }
    public static void Step(object raw) { ((EnvDTE.DTE)raw).Debugger.StepOver(false); }
    public static void Go(object raw) { ((EnvDTE.DTE)raw).Debugger.Go(false); }
    public static void Start(object raw) { ((EnvDTE.DTE)raw).ExecuteCommand("Debug.Start"); }
    public static void Run(object raw) { ((EnvDTE.DTE)raw).ExecuteCommand("Debug.StartWithoutDebugging"); }
    public static void Quit(object raw) { ((EnvDTE.DTE)raw).Quit(); }
}
