//! The crate's own tests, split by the specification each is about.

mod bounds;
mod cascade;
mod parser;
mod selector;
mod tokenizer;

use crate::media::MediaContext;
use crate::{parse, Budget, Element, Limits, NoImports, Stylesheet};

/// The element a test cascades over: a flat, document-ordered tree.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attributes: Vec<(String, String)>,
    pub style: Option<String>,
    pub parent: Option<usize>,
    pub previous: Option<usize>,
    pub next: Option<usize>,
}

impl Element for Node {
    fn local_name(&self) -> &str {
        &self.name
    }
    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    fn classes(&self) -> &[String] {
        &self.classes
    }
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    fn parent(&self) -> Option<usize> {
        self.parent
    }
    fn previous_sibling(&self) -> Option<usize> {
        self.previous
    }
    fn next_sibling(&self) -> Option<usize> {
        self.next
    }
    fn inline_style(&self) -> Option<&str> {
        self.style.as_deref()
    }
}

/// Builds a tree from `(name, parent)` pairs, filling the sibling links in.
///
/// The sibling links are computed rather than written by hand, because a
/// fixture whose `+` links disagree with its parent links tests the fixture.
pub fn tree(spec: &[(&str, Option<usize>)]) -> Vec<Node> {
    let mut nodes: Vec<Node> = spec
        .iter()
        .map(|(name, parent)| Node {
            name: (*name).to_string(),
            parent: *parent,
            ..Node::default()
        })
        .collect();
    for index in 0..nodes.len() {
        let parent = nodes[index].parent;
        let previous = (0..index)
            .rev()
            .find(|earlier| nodes[*earlier].parent == parent);
        nodes[index].previous = previous;
        if let Some(previous) = previous {
            nodes[previous].next = Some(index);
        }
    }
    nodes
}

/// Parses a stylesheet with no imports at the shipped limits.
pub fn sheet(source: &str) -> Stylesheet {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    parse(
        source.as_bytes(),
        None,
        &NoImports,
        &MediaContext::screen(432.0, 648.0),
        &limits,
        &mut budget,
    )
    .expect("the fixture is under every cap")
}
