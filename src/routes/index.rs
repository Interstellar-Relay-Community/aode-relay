use crate::{
    config::Config,
    data::{Node, State},
    error::{Error, ErrorKind},
};
use actix_web::{web, HttpResponse};
use rand::{seq::SliceRandom, thread_rng};
use std::io::BufWriter;

const MINIFY_CONFIG: minify_html::Cfg = minify_html::Cfg {
    do_not_minify_doctype: true,
    ensure_spec_compliant_unquoted_attribute_values: true,
    keep_closing_tags: true,
    keep_html_and_head_opening_tags: false,
    keep_spaces_between_attributes: true,
    keep_comments: false,
    minify_css: true,
    minify_css_level_1: true,
    minify_css_level_2: false,
    minify_css_level_3: false,
    minify_js: true,
    remove_bangs: true,
    remove_processing_instructions: true,
};

fn open_reg(node: &Node) -> bool {
    node.instance
        .as_ref()
        .map(|i| i.reg)
        .or_else(|| node.info.as_ref().map(|i| i.reg))
        .unwrap_or(false)
}

#[tracing::instrument(name = "Index", skip(config, state))]
pub(crate) async fn route(
    state: web::Data<State>,
    config: web::Data<Config>,
) -> Result<HttpResponse, Error> {
    let all_nodes = state.node_cache().nodes().await?;

    let mut nodes = Vec::new();
    let mut local = Vec::new();

    for node in all_nodes {
        if node
            .base
            .authority_str()
            .map(|authority| {
                config
                    .local_domains()
                    .iter()
                    .any(|domain| domain.as_str() == authority)
            })
            .unwrap_or(false)
        {
            local.push(node);
        } else {
            nodes.push(node);
        }
    }

    nodes.sort_by(|lhs, rhs| match (open_reg(lhs), open_reg(rhs)) {
        (true, true) | (false, false) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
    });

    if let Some((i, _)) = nodes.iter().enumerate().find(|(_, node)| !open_reg(node)) {
        nodes[..i].shuffle(&mut thread_rng());
        nodes[i..].shuffle(&mut thread_rng());
    } else {
        nodes.shuffle(&mut thread_rng());
    }

    let mut buf = BufWriter::new(Vec::new());

    crate::templates::index_html(&mut buf, &local, &nodes, &config)?;
    let html = buf.into_inner().map_err(|e| {
        tracing::error!("Error rendering template, {}", e.error());
        ErrorKind::FlushBuffer
    })?;

    let html = minify_html::minify(&html, &MINIFY_CONFIG);

    Ok(HttpResponse::Ok().content_type("text/html").body(html))
}
