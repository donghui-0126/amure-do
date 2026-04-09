/// Dashboard serving — main UI and graph visualization.

use axum::response::Html;

const MAIN_DASHBOARD: &str = include_str!("../../dashboard/index.html");
const GRAPH_DASHBOARD: &str = include_str!("../../dashboard/graph.html");

pub async fn serve_dashboard() -> Html<&'static str> {
    Html(MAIN_DASHBOARD)
}

pub async fn serve_graph() -> Html<&'static str> {
    Html(GRAPH_DASHBOARD)
}
