use crate::app::NamedCallView;
use crate::model::Document;
use serde::Serialize;

#[derive(Serialize)]
struct ViewerData<'a> {
    document: &'a Document,
    named_calls: &'a [NamedCallView],
}

pub fn render_html(document: &Document, named_calls: &[NamedCallView]) -> Result<String, String> {
    let data = serde_json::to_string(&ViewerData {
        document,
        named_calls,
    })
    .map_err(|e| e.to_string())?
    .replace("</", "<\\/");
    Ok(include_str!("../assets/viewer.html").replace("__GRAPH_DATA__", &data))
}
