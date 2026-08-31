use crate::model::Document;

pub fn render_html(document: &Document) -> Result<String, String> {
    let data = serde_json::to_string(document)
        .map_err(|e| e.to_string())?
        .replace("</", "<\\/");
    Ok(include_str!("../assets/viewer.html").replace("__GRAPH_DATA__", &data))
}
