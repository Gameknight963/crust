using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Web.Script.Serialization;
using System.Diagnostics;
using Microsoft.VisualStudio.Language.Intellisense;
using Microsoft.VisualStudio.Language.Intellisense.AsyncCompletion;
using Microsoft.VisualStudio.Language.Intellisense.AsyncCompletion.Data;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Classification;
using Microsoft.VisualStudio.Text.Editor;
using Microsoft.VisualStudio.Utilities;

// Added only to the live test's copy of the VSIX. Observe the editor's actual
// combined TextMate/LSP classifications through the public editor API.
[Export(typeof(ITextViewCreationListener)), ContentType("crust"), TextViewRole(PredefinedTextViewRoles.Document)]
public sealed class CrustEditorProbe : ITextViewCreationListener
{
    [Import] public IClassifierAggregatorService Classifiers = null;
    [Import] public ITextDocumentFactoryService Documents = null;
    [Import] public IAsyncCompletionBroker Completions = null;
    [Import] public IAsyncQuickInfoBroker QuickInfo = null;
    [Import] public IClassificationFormatMapService FormatMaps = null;
    [Import] public IClassificationTypeRegistryService Registry = null;
    private static readonly object Gate = new object();

    public void TextViewCreated(ITextView view)
    {
        string output = Environment.GetEnvironmentVariable("CRUST_LIVE_CLASSIFICATIONS");
        ITextDocument document;
        if (string.IsNullOrEmpty(output) || !Documents.TryGetTextDocument(view.TextBuffer, out document)) return;
        if (!document.FilePath.EndsWith("main.cr", StringComparison.OrdinalIgnoreCase)) output += "." + Path.GetFileName(document.FilePath);
        var classifier = Classifiers.GetClassifier(view.TextBuffer);
        bool reading = false;
        int completionVersion = -1;
        int featureVersion = -1;
        var elapsed = Stopwatch.StartNew();
        Action record = () =>
        {
            lock (Gate)
            {
                if (reading || view.IsClosed) return;
                reading = true;
                try
                {
                    var snapshot = view.TextSnapshot;
                    var spans = classifier.GetClassificationSpans(new SnapshotSpan(snapshot, 0, snapshot.Length));
                    var payload = new {
                        file = document.FilePath, text = snapshot.GetText(),
                        elapsedMilliseconds = elapsed.ElapsedMilliseconds,
                        spans = spans.Select(span => new {
                            start = span.Span.Start.Position, length = span.Span.Length,
                            text = span.Span.GetText(), types = Types(span.ClassificationType).Distinct().ToArray()
                        }).ToArray()
                    };
                    File.WriteAllText(output, new JavaScriptSerializer().Serialize(payload));
                    if (snapshot.GetText().StartsWith("// crust editor feature probe\n") && featureVersion != snapshot.Version.VersionNumber)
                    {
                        featureVersion = snapshot.Version.VersionNumber;
                        _ = RecordFeaturesAsync(view, snapshot, output);
                    }
                    if (snapshot.GetText().StartsWith("#pragma crust be\n") && completionVersion != snapshot.Version.VersionNumber)
                    {
                        completionVersion = snapshot.Version.VersionNumber;
                        _ = RecordCompletionsAsync(view, snapshot, output);
                    }
                }
                catch (Exception error) { File.WriteAllText(output + ".error", error.ToString()); }
                finally { reading = false; }
            }
        };
        EventHandler<ClassificationChangedEventArgs> classified = (_, args) => record();
        EventHandler<TextContentChangedEventArgs> changed = (_, args) => record();
        classifier.ClassificationChanged += classified;
        view.TextBuffer.Changed += changed;
        view.Closed += (_, args) => {
            classifier.ClassificationChanged -= classified;
            view.TextBuffer.Changed -= changed;
            var disposable = classifier as IDisposable;
            if (disposable != null) disposable.Dispose();
        };
        record();
    }

    private async Task RecordFeaturesAsync(ITextView view, ITextSnapshot snapshot, string output)
    {
        try
        {
            await Task.Delay(200);
            int cursor = snapshot.GetText().IndexOf("support(42)", StringComparison.Ordinal);
            var timer = Stopwatch.StartNew();
            var items = await QuickInfo.GetQuickInfoItemsAsync(view,
                snapshot.CreateTrackingPoint(cursor + 2, PointTrackingMode.Positive), CancellationToken.None);
            string hover = items == null ? "" : Flatten(items.Items, 0);
            long hoverMilliseconds = timer.ElapsedMilliseconds;
            var classes = items == null ? new string[0] : HoverClasses(items.Items, 0).Distinct().ToArray();
            var colors = new[] { "keyword", "method name", "parameter name" }.Select(name => new {
                classification = name,
                editor = FormatMaps.GetClassificationFormatMap("text").GetTextProperties(Registry.GetClassificationType(name)).ForegroundBrush.ToString(),
                tooltip = FormatMaps.GetClassificationFormatMap("tooltip").GetTextProperties(Registry.GetClassificationType(name)).ForegroundBrush.ToString()
            }).ToArray();
            File.WriteAllText(output + ".features", new JavaScriptSerializer().Serialize(new { hover, hoverMilliseconds, classes, colors }));
        }
        catch (Exception error) { File.WriteAllText(output + ".features-error", error.ToString()); }
    }

    private static IEnumerable<string> HoverClasses(object value, int depth)
    {
        if (value == null || value is string || depth > 8) yield break;
        var classification = value.GetType().GetProperty("ClassificationTypeName");
        if (classification != null) yield return (string)classification.GetValue(value);
        var list = value as System.Collections.IEnumerable;
        if (list != null) { foreach (var item in list) foreach (string name in HoverClasses(item, depth + 1)) yield return name; }
        else foreach (string propertyName in new[] { "Runs", "Elements", "Content" }) {
            var property = value.GetType().GetProperty(propertyName);
            if (property != null) foreach (string name in HoverClasses(property.GetValue(value), depth + 1)) yield return name;
        }
    }

    private static string Flatten(object value, int depth)
    {
        if (value == null || depth > 8) return "";
        if (value is string) return (string)value;
        var list = value as System.Collections.IEnumerable;
        if (list != null) return string.Join(" ", list.Cast<object>().Select(item => Flatten(item, depth + 1)));
        return string.Join(" ", new[] { "Text", "Runs", "Elements", "Content" }
            .Select(name => value.GetType().GetProperty(name))
            .Where(property => property != null && property.GetIndexParameters().Length == 0)
            .Select(property => Flatten(property.GetValue(value), depth + 1)));
    }

    private async Task RecordCompletionsAsync(ITextView view, ITextSnapshot snapshot, string output)
    {
        try
        {
            // Let document synchronization observe the edit before querying the
            // client. The IDE is hidden, so use its normal completion sources
            // without opening a popup that requires keyboard focus.
            await Task.Delay(200);
            var trigger = new CompletionTrigger(CompletionTriggerReason.Invoke, snapshot);
            var point = snapshot.GetLineFromLineNumber(0).End;
            view.Caret.MoveTo(point);
            var session = Completions.TriggerCompletion(view, trigger, point, CancellationToken.None);
            bool immediate = session != null;
            // VS 18's universal LSP source fetches items before creating its
            // session. TriggerCompletion can return null while that runs.
            for (int attempt = 0; session == null && attempt < 100; attempt++)
            {
                await Task.Delay(100);
                session = Completions.GetSession(view);
            }
            if (session == null) throw new InvalidOperationException("Visual Studio did not create a completion session.");
            if (immediate) session.OpenOrUpdate(trigger, point, CancellationToken.None);
            var context = await Task.Run(() => session.GetComputedItems(CancellationToken.None));
            session.CommitIfUnique(CancellationToken.None);
#pragma warning disable CS0618 // Items is shared by VS 17.14 and 18. ItemList is newer.
            var payload = new {
                supported = Completions.IsCompletionSupported(view.TextBuffer.ContentType, view.Roles),
                contentType = view.TextBuffer.ContentType.TypeName,
                roles = view.Roles.ToArray(), position = point.Position,
                committedText = view.TextSnapshot.GetText(),
                items = context.Items.Select(item => new { label = item.DisplayText, insertion = item.InsertText }).ToArray()
            };
#pragma warning restore CS0618
            File.WriteAllText(output + ".completion", new JavaScriptSerializer().Serialize(payload));
            session.Dismiss();
        }
        catch (Exception error) { File.WriteAllText(output + ".completion-error", error.ToString()); }
    }

    private static IEnumerable<string> Types(IClassificationType type)
    {
        yield return type.Classification;
        foreach (var parent in type.BaseTypes)
            foreach (var name in Types(parent)) yield return name;
    }
}
