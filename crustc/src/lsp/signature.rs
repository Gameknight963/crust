use super::semantic::{PositionParams, RequestError, offset};
use super::*;

pub(super) fn request(
    params: Value,
    documents: &HashMap<String, Document>,
    roots: &[PathBuf],
) -> Result<Value, RequestError> {
    let params: PositionParams = serde_json::from_value(params)?;
    let Some(document) = documents.get(&params.text_document.uri) else {
        return Ok(Value::Null);
    };
    let Some(offset) = offset(&document.text, &params.position) else {
        return Ok(Value::Null);
    };
    let Some(context) =
        super::completion::context_at(&params.text_document.uri, offset, documents, roots)
    else {
        return Ok(Value::Null);
    };
    let Some(help) = context.analysis.signature_help_at(context.file, offset) else {
        return Ok(Value::Null);
    };
    let signatures: Vec<_> = help.signatures.into_iter().map(|signature| {
        let mut information = json!({ "label": signature.label, "parameters": signature.parameters.iter().map(|parameter| json!({ "label": [parameter.start, parameter.end] })).collect::<Vec<_>>() });
        if !signature.parameters.is_empty() && (signature.variadic || help.argument < signature.parameters.len()) {
            information["activeParameter"] = json!(help.argument.min(signature.parameters.len() - 1));
        }
        return information;
    }).collect();
    let mut result = json!({ "signatures": signatures, "activeSignature": 0 });
    if let Some(parameter) = result["signatures"][0].get("activeParameter").cloned() {
        result["activeParameter"] = parameter;
    }
    return Ok(result);
}
