using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.IO;
using System.Linq;
using System.Web.Script.Serialization;
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
    private static readonly object Gate = new object();

    public void TextViewCreated(ITextView view)
    {
        string output = Environment.GetEnvironmentVariable("CRUST_LIVE_CLASSIFICATIONS");
        ITextDocument document;
        if (string.IsNullOrEmpty(output) || !Documents.TryGetTextDocument(view.TextBuffer, out document)) return;
        var classifier = Classifiers.GetClassifier(view.TextBuffer);
        bool reading = false;
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
                        spans = spans.Select(span => new {
                            start = span.Span.Start.Position, length = span.Span.Length,
                            text = span.Span.GetText(), types = Types(span.ClassificationType).Distinct().ToArray()
                        }).ToArray()
                    };
                    File.WriteAllText(output, new JavaScriptSerializer().Serialize(payload));
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

    private static IEnumerable<string> Types(IClassificationType type)
    {
        yield return type.Classification;
        foreach (var parent in type.BaseTypes)
            foreach (var name in Types(parent)) yield return name;
    }
}
