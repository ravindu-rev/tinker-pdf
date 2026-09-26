//! 14.2.4's **remote** resource dictionary: a `ResourceDictionary` whose
//! `Source` names a separate OPC part.
//!
//! # Why this is a pass and not a lookup
//!
//! [`super::font::Fonts::load`] and [`super::image::Images::load`] both resolve
//! their parts *before* the drawing walk starts, and for a reason that is this
//! module's too: `Package::read_part` hands back a **borrow of the package**,
//! and the walk is already holding one — the fixed page's own bytes. A
//! dictionary fetched mid-walk would need the page copied out, and a fixed page
//! part is the one part of this format whose size the file chooses. So the page
//! is scanned once for `Source` attributes, every part named is read and parsed
//! here, and the walk does a pure lookup in the table this pass filled.
//!
//! # Two guards, and they are two rules
//!
//! A remote dictionary's own root may state a `Source` of its own, so the parts
//! form a chain rather than a step. That chain can be long without repeating,
//! and it can repeat without being long:
//!
//! - longer than [`MAX_XPS_RESOURCE_DEPTH`] is
//!   [`XpsElementDefect::BrushTooDeep`];
//! - a part that names one already on the chain is
//!   [`XpsElementDefect::BrushCyclic`], and never terminates without the guard.
//!
//! This is the same pair `paint.rs`'s `{StaticResource}` lookup carries and the
//! same pair a `VisualBrush` nest carries, under the same two names. Three
//! places, one distinction, because deleting either guard in any of them leaves
//! the other passing every test written for it.
//!
//! # What a failure costs
//!
//! One [`XpsElementDefect`] on the element that named the dictionary, and
//! nothing else: the page keeps drawing and every `{StaticResource}` the
//! dictionary would have answered falls through to
//! [`XpsElementDefect::BrushUnresolved`] and the placeholder grey. That is
//! ruling 2 — a refusal is a named placeholder and never a lost page — and it
//! is why a dictionary part that will not read is not a page-level defect.

use std::collections::HashMap;

use tinker_pdf_xml::{Doctype, Event, Source};

use super::markup::{self, Budget, Node, Trouble};
use super::opc::{Package, PartName};
use super::{dialect_of, Limits, XpsElementDefect, MAX_XPS_RESOURCE_DEPTH};

/// Every remote dictionary the fixed pages of one document named.
///
/// Keyed by the **part**, not by the reference that reached it: 18.2's
/// `Source` is relative to the part it is written on, so two pages in different
/// folders may spell one dictionary two ways and it is still one dictionary.
#[derive(Default)]
pub struct Remotes {
    /// The entries a part holds, or why it holds none.
    loaded: HashMap<PartName, Result<HashMap<String, Node>, XpsElementDefect>>,
}

impl Remotes {
    /// Reads every dictionary part the `Source` attributes of one fixed page
    /// name, and every part those parts name in turn.
    ///
    /// # Errors
    /// [`Trouble::Exhausted`] when a work total is spent, which refuses the
    /// package — a dictionary part is markup and charges the same budget the
    /// page does, so a package cannot buy itself unbounded parsing by moving
    /// its markup out of its pages.
    pub fn load(
        &mut self,
        package: &mut Package<'_>,
        page: &PartName,
        limits: &Limits,
        budget: &mut Budget,
    ) -> Result<(), Trouble> {
        let wanted = match package.read_part(page) {
            Ok(bytes) => sources_named(bytes, page, limits)?,
            // The page will not read at all. The painter reports that a moment
            // later, in its own words; this pass has nothing to add.
            Err(_) => return Ok(()),
        };
        for name in wanted {
            if self.loaded.contains_key(&name) {
                continue;
            }
            let mut chain = Vec::new();
            let entries = self.follow(package, &name, &mut chain, limits, budget)?;
            self.loaded.insert(name, entries);
        }
        Ok(())
    }

    /// The entries a `Source` written on `page` names.
    ///
    /// Pure: resolution is arithmetic over two strings and the lookup is the
    /// table [`Remotes::load`] filled, so the drawing walk never needs the
    /// package.
    ///
    /// # Errors
    /// One [`XpsElementDefect`] per way a `Source` can fail to be a dictionary,
    /// each by its own name.
    pub fn get(
        &self,
        page: &PartName,
        source: &str,
    ) -> Result<&HashMap<String, Node>, XpsElementDefect> {
        let name = page
            .resolve(source)
            .ok_or(XpsElementDefect::ResourceDictionaryUnresolved)?;
        match self.loaded.get(&name) {
            Some(Ok(entries)) => Ok(entries),
            Some(Err(defect)) => Err(*defect),
            None => Err(XpsElementDefect::ResourceDictionaryUnresolved),
        }
    }

    /// One part, and the chain of `Source`s it may itself start.
    ///
    /// `chain` is the parts already being followed, innermost last. It is a
    /// list rather than a depth because the two guards answer different
    /// questions: how long, and whether it returned.
    fn follow(
        &mut self,
        package: &mut Package<'_>,
        name: &PartName,
        chain: &mut Vec<PartName>,
        limits: &Limits,
        budget: &mut Budget,
    ) -> Result<Result<HashMap<String, Node>, XpsElementDefect>, Trouble> {
        if chain.iter().any(|seen| seen == name) {
            return Ok(Err(XpsElementDefect::BrushCyclic));
        }
        if chain.len() >= MAX_XPS_RESOURCE_DEPTH {
            return Ok(Err(XpsElementDefect::BrushTooDeep));
        }
        if !package.has(name) {
            return Ok(Err(XpsElementDefect::ResourceDictionaryUnresolved));
        }
        let root = match package.read_part(name) {
            Ok(bytes) => match dictionary_root(bytes, limits, budget) {
                Ok(Some(root)) => root,
                // The part is there and is not 14.2.4's markup. Named as
                // unreadable rather than unresolved: "no such part" and "that
                // part is not a dictionary" are different facts about the file
                // and a reader can act on the difference.
                Ok(None) => return Ok(Err(XpsElementDefect::ResourceDictionaryUnreadable)),
                Err(trouble) => return Err(trouble),
            },
            Err(_) => return Ok(Err(XpsElementDefect::ResourceDictionaryUnresolved)),
        };

        // 14.2.4 lets the part's own root carry a `Source`, which is what makes
        // this a chain. It is resolved against **this part's** name and not the
        // page's, which is 18.2's rule and the reason a dictionary two folders
        // away can name a third.
        if let Some(next) = root.attr("Source") {
            let Some(next) = name.resolve(next) else {
                return Ok(Err(XpsElementDefect::ResourceDictionaryUnresolved));
            };
            chain.push(name.clone());
            let followed = self.follow(package, &next, chain, limits, budget);
            chain.pop();
            return followed;
        }
        Ok(Ok(entries_of(&root)))
    }
}

/// The `x:Key`ed children of a `ResourceDictionary`, as a table.
///
/// An entry with no key is dropped rather than given one: 14.2.2 makes `x:Key`
/// mandatory, and an entry without one can never be referenced by anything.
fn entries_of(root: &Node) -> HashMap<String, Node> {
    let mut entries = HashMap::new();
    for entry in &root.children {
        if let Some(key) = entry.key.clone() {
            entries.insert(key, entry.clone());
        }
    }
    entries
}

/// A dictionary part's root element, when it is a `ResourceDictionary`.
///
/// `Ok(None)` is a part that read as markup and whose root is something else,
/// which is a fact about the file rather than a failure of this build.
fn dictionary_root(
    bytes: &[u8],
    limits: &Limits,
    budget: &mut Budget,
) -> Result<Option<Node>, Trouble> {
    let Ok(source) = Source::new(bytes) else {
        return Ok(None);
    };
    let mut reader = source.reader_with(&limits.xml, Doctype::Refuse);
    loop {
        let Some(event) = reader.next() else {
            return Ok(None);
        };
        let Ok(event) = event else {
            return Ok(None);
        };
        if let Event::Start(element) = event {
            if element.local() != "ResourceDictionary" || dialect_of(element.namespace()).is_none()
            {
                return Ok(None);
            }
            budget.element()?;
            return markup::subtree(&mut reader, &element, budget).map(Some);
        }
    }
}

/// Every part the `ResourceDictionary` elements of one page name.
///
/// Scanned with the streaming reader rather than the materialised tree, which
/// is [`super::image::Images::load`]'s own choice and for its reason: this runs
/// before the drawing walk and has no tree to read.
fn sources_named(bytes: &[u8], page: &PartName, limits: &Limits) -> Result<Vec<PartName>, Trouble> {
    let Ok(source) = Source::new(bytes) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<PartName> = Vec::new();
    for event in source.reader_with(&limits.xml, Doctype::Refuse) {
        let element = match event {
            Ok(Event::Start(element)) => element,
            Ok(_) => continue,
            // Markup that will not read is the painter's to report, and it
            // will: this pass answers with what it found and the drawing walk
            // meets the same failure a moment later.
            Err(_) => break,
        };
        if element.local() != "ResourceDictionary" || dialect_of(element.namespace()).is_none() {
            continue;
        }
        let Some(reference) = element.attribute(None, "Source") else {
            continue;
        };
        let Some(name) = page.resolve(reference) else {
            continue;
        };
        if !out.contains(&name) {
            out.push(name);
            if out.len() > limits.max_parts {
                return Err(Trouble::Exhausted);
            }
        }
    }
    Ok(out)
}
