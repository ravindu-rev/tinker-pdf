//! The logical structure tree: what a tagged document says its content *is*
//! (14.7).
//!
//! A tagged PDF carries a second description of itself beside the marks on
//! the page: a tree of elements — a document, its parts, its paragraphs, its
//! figures — each tied to the content that draws it by 14.7.4's marked-content
//! identifiers. Reading order lives here rather than in the geometry, which is
//! why a two-column page whose columns interleave in the content stream still
//! reads down one column and then the other.
//!
//! This is a facade module in the shape of [`crate::optional`]: bound once
//! from the catalog, COS types kept inside it (ruling 11), nothing mutated.
//! [`crate::Document::structure`] returns `None` when the catalog has no
//! `/StructTreeRoot`, which is what most documents are — and 14.7 gives no way
//! to infer one, so nothing is guessed for them.
//!
//! # Everything here is attacker-controlled
//!
//! `/K` is a graph with no promise of acyclicity, `/RoleMap` is a rewriting
//! system a file writes for itself, and both are read before anything has
//! validated either. Ruling 1: every walk below is bounded by a named
//! constant, every truncation is a typed warning naming the object it stopped
//! at (ruling 10), and a refused subtree costs the subtree rather than the
//! document (ruling 2).
//!
//! Feature documentation: `docs/features/content-and-text.md`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use tinker_pdf_content::{TextChar, TextPage};
use tinker_pdf_cos::{
    decode_text_string, limits, number_tree, pages as cos_pages, CosDocument, Dict, Name, ObjRef,
    Object,
};

/// How deep the `/K` tree may nest before a subtree is refused (14.7.2).
///
/// [`limits::MAX_NEST_DEPTH`], the same number the COS object parser and the
/// name/number-tree walker use, because a structure tree is nesting like any
/// other and a second number would be a second thing to get wrong. Real
/// logical structure is shallow: a book's deepest path runs document → part →
/// chapter → section → list → item → paragraph → span, which is eight.
const MAX_STRUCTURE_DEPTH: u32 = limits::MAX_NEST_DEPTH;

/// How many structure elements one document may yield.
///
/// **The depth cap alone does not bound the walk.** 14.7.2 says an element has
/// one parent, but nothing in a *file* enforces that, so a document may name
/// the same subtree from two places at every level and describe 2^256 elements
/// in a few hundred objects. This is what actually stops that, and it is
/// checked before descending rather than after, so the bomb costs this many
/// visits and not one more.
///
/// The largest honest trees are long documents tagged paragraph by paragraph
/// and run to the low hundreds of thousands.
const MAX_STRUCTURE_ELEMENTS: usize = 1 << 18;

/// How many entries of one element's `/K` array are examined.
///
/// A page tagged span by span reaches the low thousands; this is well above
/// any of them, and it bounds the per-element cost the element cap alone would
/// not — refusing the 262 145th element still costs a look at every kid that
/// named it.
const MAX_KIDS: usize = 1 << 16;

/// How many `/RoleMap` hops one type name may take (14.7.3).
///
/// `/RoleMap` maps a name to another name, and nothing stops a file mapping
/// `/Foo` to `/Bar` to `/Foo`. The loop is caught by name below and this
/// bounds the acyclic-but-absurd chain as well; a real role map is one hop.
const MAX_ROLE_MAP_HOPS: u32 = 16;

/// How many warnings one structure walk retains.
///
/// A hostile tree can produce one per element, and a report nobody can read is
/// not provenance. Past this the walk still completes and its counts are still
/// right; only the enumeration stops.
const MAX_STRUCTURE_WARNINGS: usize = 1024;

/// Something the structure walk had to tolerate (ruling 10).
///
/// Every variant names the object it happened to, because "the tree was
/// truncated" with no object is a sentence a reader cannot act on — and a
/// truncated structure tree is exactly the leniency that is invisible from
/// outside, since what it costs is content quietly ceasing to be reachable in
/// reading order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructureWarning {
    /// 14.7.2: an element's `/K` reached an element already on the path from
    /// the root. The branch was cut there.
    KidCycle {
        /// The element the cycle closed on.
        element: ObjRef,
    },
    /// 14.7.2: [`MAX_STRUCTURE_DEPTH`] was reached; this element's kids were
    /// not read.
    DepthCapped {
        /// The element whose kids were refused.
        element: ObjRef,
    },
    /// [`MAX_STRUCTURE_ELEMENTS`] was reached; the rest of the tree was not
    /// read.
    ElementCapped,
    /// [`MAX_KIDS`] entries of one `/K` array were read and the rest dropped.
    KidsCapped {
        /// The element whose kid list was truncated.
        element: ObjRef,
        /// How many entries the array actually held.
        kids: usize,
    },
    /// 14.7.2: an element dictionary carries no `/S`, so it names no
    /// structure type. It is kept, with an empty type, because its kids are
    /// still content.
    UntypedElement {
        /// The element.
        element: ObjRef,
    },
    /// 14.7.2: a `/K` entry was none of the shapes 14.7.4 defines — not an
    /// integer, not a structure element, not an `/MCR` or `/OBJR`. It was
    /// dropped.
    UnreadableKid {
        /// The element whose `/K` held it, when its parent could be named.
        element: Option<ObjRef>,
    },
    /// 14.7.3: `/RoleMap` maps this type into a cycle. Resolution stopped at
    /// the name that closed it, and the element keeps that name as its
    /// standard type.
    RoleMapLoop {
        /// The raw type the loop was entered by.
        role: String,
    },
    /// 14.7.4.4: the `/ParentTree` and the `/K` walk disagree about how many
    /// marked-content sequences a page has.
    ///
    /// **The `/K` walk wins**, and this says by how much. The two are written
    /// by one producer from one set of facts, so a disagreement means one of
    /// them was regenerated and the other was not — a fact about the file
    /// worth reporting rather than a reason to prefer either.
    ParentTreeDisagreement {
        /// Zero-based page index.
        page: u32,
        /// How many distinct `/MCID`s the `/K` walk placed on this page.
        walk: usize,
        /// How many the page's `/ParentTree` entry accounts for.
        parent_tree: usize,
    },
}

/// One kid of a structure element — the three shapes 14.7.4 gives, never
/// collapsed.
///
/// In the spirit of ruling 6. A marked-content reference and an object
/// reference are not two spellings of one thing: the first names glyphs inside
/// a content stream, the second names an annotation or an XObject that has no
/// glyphs at all, and flattening them makes a `Figure` whose content is an
/// image indistinguishable from one whose content is text this engine failed
/// to find.
#[derive(Clone, Debug)]
pub enum StructKid {
    /// A nested structure element (14.7.2).
    ///
    /// Boxed, and the box is not a style choice. A `Content` kid is two words
    /// and an `Element` is a couple of hundred bytes, and a page tagged
    /// character by character — which is what an OCR producer writes — is a
    /// `/K` array of thousands of `Content` kids. Inline, every one of them
    /// would cost an element's worth of memory, so
    /// [`MAX_STRUCTURE_ELEMENTS`]-worth of kids is tens of megabytes rather
    /// than a few.
    Element(Box<StructElement>),
    /// Content in a page's stream, by its marked-content identifier
    /// (14.7.4.2) — an integer kid resolved against the element's `/Pg`, or an
    /// `/MCR` dictionary with a `/Pg` of its own.
    Content {
        /// The page it is drawn on, when one could be resolved. `None` when
        /// neither the reference nor any ancestor named a `/Pg`, which leaves
        /// the identifier where the file left it rather than guessing.
        page: Option<u32>,
        /// The `/MCID`.
        mcid: u32,
    },
    /// A whole object as a content item: an annotation, an XObject
    /// (14.7.4.3's `/OBJR`).
    Object(ObjRef),
}

/// One structure element (14.7.2).
#[derive(Clone, Debug)]
pub struct StructElement {
    /// The element's own indirect reference, when it had one. `None` for an
    /// element written inline in its parent's `/K`.
    pub reference: Option<ObjRef>,
    /// `/S` exactly as the file wrote it.
    ///
    /// Kept **beside** [`StructElement::standard_type`] rather than replaced
    /// by it. 14.7.3 lets a producer name its own types and map them to the
    /// standard set, and a consumer that wants to know a paragraph is a
    /// paragraph and a consumer that wants the file's own vocabulary back are
    /// both real; keeping one name loses one of them.
    pub raw_type: String,
    /// `/S` after `/RoleMap` (14.7.3), or `raw_type` itself when the role map
    /// does not mention it.
    ///
    /// An unmapped custom type resolving to itself is ruling 2: a `/Foo` no
    /// role map explains is a `/Foo`, not an error and not a `/Span`.
    pub standard_type: String,
    /// `/T`, the human-readable title.
    pub title: Option<String>,
    /// `/Lang`, the natural language of this element's content (14.9.2).
    pub lang: Option<String>,
    /// `/Alt`, a description for content that is not text (14.9.3).
    pub alt: Option<String>,
    /// `/ActualText`, what the content *is* rather than what it draws
    /// (14.9.4).
    pub actual_text: Option<String>,
    /// `/E`, what an abbreviation stands for (14.9.5).
    pub expansion: Option<String>,
    /// `/Pg`, resolved to a zero-based page index.
    pub page: Option<u32>,
    /// `/K`, in the order the file wrote it — which 14.8 makes reading order.
    pub kids: Vec<StructKid>,
}

/// A document's logical structure (14.7).
#[derive(Clone, Debug)]
pub struct StructureTree {
    /// The structure tree root's own `/K`.
    pub kids: Vec<StructKid>,
    /// `/MarkInfo /Marked`: the document claims to be a tagged PDF (14.7.1).
    ///
    /// A tree with `marked` false is a document carrying structure without
    /// claiming to satisfy 14.8's tagged-PDF rules. Both exist in the wild and
    /// they are different claims, so this is reported rather than folded into
    /// whether the tree is `Some`.
    pub marked: bool,
    /// `/MarkInfo /Suspects`: the producer says the tagging may be wrong.
    pub suspects: bool,
    /// `/MarkInfo /UserProperties`: the tree carries user properties.
    pub user_properties: bool,
    /// What the walk had to tolerate (ruling 10).
    pub warnings: Vec<StructureWarning>,
    /// How many pages the document has, for the one-page leniency in
    /// [`Join::content`].
    page_count: u32,
    /// The `/StructParents` key of each page that declared one (14.7.4.4).
    struct_parents: BTreeMap<u32, i64>,
    /// How many content items each `/ParentTree` key accounts for.
    parent_tree: BTreeMap<i64, usize>,
}

impl StructureTree {
    /// How many structure elements the tree holds.
    #[must_use]
    pub fn element_count(&self) -> usize {
        count(&self.kids, &|kid| matches!(kid, StructKid::Element(_)))
    }

    /// How many marked-content references the tree holds (14.7.4.2).
    #[must_use]
    pub fn content_count(&self) -> usize {
        count(&self.kids, &|kid| matches!(kid, StructKid::Content { .. }))
    }

    /// How many `/OBJR` object references the tree holds (14.7.4.3).
    #[must_use]
    pub fn object_count(&self) -> usize {
        count(&self.kids, &|kid| matches!(kid, StructKid::Object(_)))
    }

    /// Every element in the tree, depth-first — which 14.8 makes reading
    /// order.
    #[must_use]
    pub fn elements(&self) -> Vec<&StructElement> {
        let mut out = Vec::new();
        collect_elements(&self.kids, &mut out);
        out
    }

    /// One page's text in structure order, joined with this tree (14.8).
    ///
    /// `page` is the **same** [`TextPage`] [`crate::Page::text`] produces, not
    /// a second extraction. That is the whole discipline of this join: two
    /// extractors would drift, and the drift would surface as a structured
    /// view quietly disagreeing with search and selection about what the page
    /// says.
    #[must_use]
    pub fn text_for_page(&self, index: u32, page: &TextPage) -> StructuredText {
        let mut join = Join {
            index,
            unpaged_is_here: self.page_count <= 1,
            by_mcid: chars_by_mcid(page),
            page,
            claimed: BTreeSet::new(),
            nodes: Vec::new(),
        };
        join.kids(&self.kids, None, 0, false);

        let claimed = join.claimed;
        let (mut matched, mut orphans, mut unmarked) = (0usize, 0usize, 0usize);
        for line in page.lines() {
            for character in &line.chars {
                match character.mcid {
                    None => unmarked += 1,
                    Some(mcid) if claimed.contains(&mcid) => matched += 1,
                    Some(_) => orphans += 1,
                }
            }
        }

        // 14.7.4.4: the parent tree is the same association written the other
        // way round, so it is a second opinion about how many marked sequences
        // this page has. The `/K` walk has already produced the answer; this
        // only reports when the file disagrees with itself.
        let mut warnings = Vec::new();
        if let Some(key) = self.struct_parents.get(&index) {
            if let Some(declared) = self.parent_tree.get(key) {
                if *declared != claimed.len() {
                    warnings.push(StructureWarning::ParentTreeDisagreement {
                        page: index,
                        walk: claimed.len(),
                        parent_tree: *declared,
                    });
                }
            }
        }

        StructuredText {
            nodes: join.nodes,
            matched,
            orphans,
            unmarked,
            warnings,
        }
    }
}

/// A page's text in structure order (14.8).
#[derive(Clone, Debug)]
pub struct StructuredText {
    /// The runs of content, in the order the structure tree gives them.
    pub nodes: Vec<StructuredNode>,
    /// Characters a structure element claimed and this view emitted.
    pub matched: usize,
    /// Characters carrying an `/MCID` **no** structure element on this page
    /// claimed.
    ///
    /// Counted, never appended. Appending them would make the structured view
    /// a superset of the tagged content that reads like reading order and is
    /// not, and the number is the measurement that says how far a file's
    /// tagging actually goes.
    pub orphans: usize,
    /// Characters carrying no `/MCID` at all — drawn outside every marked
    /// sequence.
    ///
    /// Distinct from [`StructuredText::orphans`]: untagged text is a producer
    /// that never marked it, and a marked run nothing claims is a structure
    /// tree that lost track of it. Different causes, and collapsing them hides
    /// which one a file has.
    pub unmarked: usize,
    /// What the join had to tolerate (ruling 10).
    pub warnings: Vec<StructureWarning>,
}

impl StructuredText {
    /// The page's text in structure order, one line per run.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for node in &self.nodes {
            if node.text.is_empty() {
                continue;
            }
            out.push_str(&node.text);
            out.push('\n');
        }
        out
    }
}

/// Where a run's text came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSource {
    /// The glyphs the page drew.
    Glyphs,
    /// An `/ActualText` that replaced them (14.9.4).
    ActualText,
}

/// One run of content under one structure element.
///
/// A *run* and not an element, because an element's content and its child
/// elements interleave: `/P [ 3 /Span[4] 5 ]` reads 3, then 4, then 5, and one
/// node per element would have to report 3 and 5 together and put 4 after
/// them. That is a reordering in the middle of a sentence, and it reads as a
/// layout opinion rather than as a bug.
#[derive(Clone, Debug)]
pub struct StructuredNode {
    /// `/S` as the file wrote it.
    pub raw_type: String,
    /// `/S` after the `/RoleMap` (14.7.3).
    pub standard_type: String,
    /// How deep in the tree the element sits; the root's own kids are 0.
    pub depth: u32,
    /// The run's text.
    pub text: String,
    /// Whether [`StructuredNode::text`] is glyphs or an `/ActualText`.
    pub source: TextSource,
    /// `/Alt` (14.9.3), from the element or from its property list.
    pub alt: Option<String>,
    /// `/Lang` (14.9.2).
    pub lang: Option<String>,
    /// `/E` (14.9.5).
    pub expansion: Option<String>,
    /// The characters this run covers, for selection and highlighting.
    ///
    /// Empty when the text came from an `/ActualText`: there are no glyphs
    /// behind one, and inventing quads would put a selection rectangle over
    /// content that says something else.
    pub chars: Vec<TextChar>,
}

// ---------------------------------------------------------------------------
// Binding
// ---------------------------------------------------------------------------

/// Reads the catalog's `/StructTreeRoot` (14.7.2).
///
/// `None` when there is none, which is most documents.
pub(crate) fn bind(doc: &Arc<CosDocument>) -> Option<StructureTree> {
    let catalog = doc.catalog()?;
    let root = doc.resolve_key(&catalog, doc.intern(b"StructTreeRoot"));
    let root = root.as_dict()?;

    // 14.7.1 Table 321. Absent means false for all three: a document saying
    // nothing about its marking is claiming nothing.
    let mark_info = doc.resolve_key(&catalog, doc.intern(b"MarkInfo"));
    let flag = |key: &[u8]| {
        mark_info
            .as_dict()
            .map(|d| doc.resolve_key(d, doc.intern(key)))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };

    let all = cos_pages::collect(doc);
    let page_count = u32::try_from(all.len()).unwrap_or(u32::MAX);
    let pages: BTreeMap<ObjRef, u32> = all
        .into_iter()
        .map(|page| (page.reference, page.index))
        .collect();
    let role_map = read_role_map(doc, root);

    let mut walk = Walk {
        doc,
        pages: &pages,
        role_map: &role_map,
        path: HashSet::new(),
        budget: MAX_STRUCTURE_ELEMENTS,
        stopped: false,
        warnings: Vec::new(),
    };
    // `get`, not `resolve_key`: an indirect `/K` must arrive at the walk
    // **still a reference**, or the element it names has no object number and
    // so cannot be checked for cycles or reported by one. Resolving here cost
    // the root element exactly that, and a cycle back to it was then caught
    // one level too late.
    let k = root.get(doc.intern(b"K")).cloned().unwrap_or(Object::Null);
    let kids = walk.kids(&k, None, None, 0);
    let warnings = walk.warnings;

    let (struct_parents, parent_tree) = read_parent_tree(doc, root, &pages);

    Some(StructureTree {
        kids,
        marked: flag(b"Marked"),
        suspects: flag(b"Suspects"),
        user_properties: flag(b"UserProperties"),
        warnings,
        page_count,
        struct_parents,
        parent_tree,
    })
}

/// A text string entry, decoded by 7.9.2.2's rules.
fn text_of(doc: &CosDocument, dict: &Dict, key: &[u8]) -> Option<String> {
    doc.resolve_key(dict, doc.intern(key))
        .as_string()
        .map(|s| decode_text_string(&s.bytes))
}

/// `/RoleMap`, as raw name to mapped name (14.7.3).
fn read_role_map(doc: &CosDocument, root: &Dict) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut out = BTreeMap::new();
    let value = doc.resolve_key(root, doc.intern(b"RoleMap"));
    let Some(dict) = value.as_dict() else {
        return out;
    };
    for (key, entry) in dict.iter() {
        let (Some(from), Some(to)) = (
            doc.name_bytes(*key),
            doc.resolve(entry).as_name().and_then(|n| doc.name_bytes(n)),
        ) else {
            continue;
        };
        out.insert(from.to_vec(), to.to_vec());
    }
    out
}

/// The `/StructParents` key of each page, and how many content items each
/// `/ParentTree` entry accounts for (14.7.4.4).
fn read_parent_tree(
    doc: &CosDocument,
    root: &Dict,
    pages: &BTreeMap<ObjRef, u32>,
) -> (BTreeMap<u32, i64>, BTreeMap<i64, usize>) {
    let mut keys = BTreeMap::new();
    for (reference, index) in pages {
        let Ok(object) = doc.get(*reference) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        if let Some(key) = doc.resolve_key(dict, doc.intern(b"StructParents")).as_int() {
            keys.insert(*index, key);
        }
    }

    let mut counts = BTreeMap::new();
    let Some(reference) = root.get_ref(doc.intern(b"ParentTree")) else {
        return (keys, counts);
    };
    for (key, value) in number_tree(doc, reference) {
        // 14.7.4.4: a page's entry is an array indexed by `/MCID` whose holes
        // are null; an object's entry is a single element. Only the first
        // shape says anything about a count.
        let resolved = doc.resolve(&value);
        let Some(items) = resolved.as_array() else {
            continue;
        };
        let filled = items
            .iter()
            .take(limits::MAX_ARRAY_LEN)
            .filter(|item| !doc.resolve(item).is_null())
            .count();
        counts.insert(key, filled);
    }
    (keys, counts)
}

/// The `/K` walk's state, so the caps are one budget rather than one per
/// recursion.
struct Walk<'a> {
    doc: &'a CosDocument,
    pages: &'a BTreeMap<ObjRef, u32>,
    role_map: &'a BTreeMap<Vec<u8>, Vec<u8>>,
    /// Object numbers on the path from the root, in the discipline `trees.rs`
    /// uses: inserted on entry and removed on exit, so a subtree named twice
    /// from different places is read twice and a subtree containing itself is
    /// cut.
    path: HashSet<u32>,
    /// Elements left before the walk stops.
    budget: usize,
    /// Set when a cap ends the walk, so every loop above unwinds without
    /// visiting more of a bomb than the budget allowed.
    stopped: bool,
    warnings: Vec<StructureWarning>,
}

impl Walk<'_> {
    fn warn(&mut self, warning: StructureWarning) {
        if self.warnings.len() < MAX_STRUCTURE_WARNINGS && !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// Reads a `/K` value: one kid, or an array of them (14.7.2).
    fn kids(
        &mut self,
        value: &Object,
        parent: Option<ObjRef>,
        page: Option<u32>,
        depth: u32,
    ) -> Vec<StructKid> {
        let mut out = Vec::new();
        let resolved = self.doc.resolve(value);
        if let Some(items) = resolved.as_array() {
            if items.len() > MAX_KIDS {
                if let Some(element) = parent {
                    let kids = items.len();
                    self.warn(StructureWarning::KidsCapped { element, kids });
                }
            }
            let items: Vec<Object> = items.iter().take(MAX_KIDS).cloned().collect();
            for item in &items {
                if self.stopped {
                    break;
                }
                if let Some(kid) = self.kid(item, parent, page, depth) {
                    out.push(kid);
                }
            }
            return out;
        }
        if resolved.is_null() {
            return out;
        }
        if let Some(kid) = self.kid(value, parent, page, depth) {
            out.push(kid);
        }
        out
    }

    /// One `/K` entry, in whichever of 14.7.4's shapes it is.
    fn kid(
        &mut self,
        value: &Object,
        parent: Option<ObjRef>,
        page: Option<u32>,
        depth: u32,
    ) -> Option<StructKid> {
        let reference = value.as_objref();
        let resolved = self.doc.resolve(value);

        // 14.7.4.2: a bare integer is an `/MCID` in the content stream of the
        // page the enclosing element's `/Pg` names. Read after resolution as
        // well as before it, so an indirect integer — legal by 7.3.10, and
        // what an incremental update leaves behind — is not lost.
        if let Some(mcid) = resolved.as_int().and_then(|n| u32::try_from(n).ok()) {
            return Some(StructKid::Content { page, mcid });
        }

        let Some(dict) = resolved.as_dict() else {
            self.warn(StructureWarning::UnreadableKid { element: parent });
            return None;
        };

        let kind = self
            .doc
            .resolve_key(dict, Name::TYPE)
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))
            .map(|n| n.to_vec());

        match kind.as_deref() {
            // 14.7.4.2: an `/MCR` names a sequence, with a `/Pg` of its own.
            Some(b"MCR") => {
                let mcid = self
                    .doc
                    .resolve_key(dict, self.doc.intern(b"MCID"))
                    .as_int()
                    .and_then(|n| u32::try_from(n).ok())?;
                let page = self.page_of(dict).or(page);
                Some(StructKid::Content { page, mcid })
            }
            // 14.7.4.3: an `/OBJR` names a whole object as a content item.
            Some(b"OBJR") => {
                let object = dict.get_ref(self.doc.intern(b"Obj"))?;
                Some(StructKid::Object(object))
            }
            // Everything else is a structure element. `/Type` is *optional* on
            // one (14.7.2 Table 323), so it is not required to say
            // `/StructElem`: requiring it drops the elements of every producer
            // that leaves it out, which is most of them.
            _ => self
                .element(reference, dict, page, depth)
                .map(|element| StructKid::Element(Box::new(element))),
        }
    }

    fn element(
        &mut self,
        reference: Option<ObjRef>,
        dict: &Dict,
        inherited_page: Option<u32>,
        depth: u32,
    ) -> Option<StructElement> {
        if self.budget == 0 {
            self.stopped = true;
            self.warn(StructureWarning::ElementCapped);
            return None;
        }
        self.budget -= 1;

        // The cycle check is on the object number, which is what a `/K` can
        // name twice. An element written inline in its parent's `/K` has no
        // number and cannot be re-entered, so it needs none.
        if let Some(reference) = reference {
            if !self.path.insert(reference.num) {
                self.warn(StructureWarning::KidCycle { element: reference });
                return None;
            }
        }

        let raw = self
            .doc
            .resolve_key(dict, self.doc.intern(b"S"))
            .as_name()
            .and_then(|n| self.doc.name_bytes(n))
            .map(|n| n.to_vec());
        if raw.is_none() {
            if let Some(reference) = reference {
                self.warn(StructureWarning::UntypedElement { element: reference });
            }
        }
        let raw = raw.unwrap_or_default();
        let standard_type = self.resolve_role(&raw);

        // 14.7.2 does not say `/Pg` is inherited. It is treated as inherited
        // here because a producer that writes it on the element holding the
        // content items and nowhere else is the common shape, and the
        // alternative is a `Content` kid with no page at all — an identifier
        // naming a marked sequence in no particular stream, which can be
        // joined to nothing. An element's own `/Pg` always wins.
        let page = self.page_of(dict).or(inherited_page);

        let kids = if depth >= MAX_STRUCTURE_DEPTH {
            if let Some(reference) = reference {
                self.warn(StructureWarning::DepthCapped { element: reference });
            }
            Vec::new()
        } else {
            let k = dict
                .get(self.doc.intern(b"K"))
                .cloned()
                .unwrap_or(Object::Null);
            self.kids(&k, reference, page, depth + 1)
        };

        if let Some(reference) = reference {
            self.path.remove(&reference.num);
        }

        Some(StructElement {
            reference,
            raw_type: String::from_utf8_lossy(&raw).into_owned(),
            standard_type,
            title: text_of(self.doc, dict, b"T"),
            lang: text_of(self.doc, dict, b"Lang"),
            alt: text_of(self.doc, dict, b"Alt"),
            actual_text: text_of(self.doc, dict, b"ActualText"),
            expansion: text_of(self.doc, dict, b"E"),
            page,
            kids,
        })
    }

    /// `/Pg`, resolved to a page index.
    fn page_of(&self, dict: &Dict) -> Option<u32> {
        let reference = dict.get_ref(self.doc.intern(b"Pg"))?;
        self.pages.get(&reference).copied()
    }

    /// 14.7.3: `/RoleMap` rewrites a type name until it reaches one it does
    /// not mention.
    ///
    /// Iterative rather than recursive, bounded by [`MAX_ROLE_MAP_HOPS`], and
    /// with a visited set so `/Foo → /Bar → /Foo` reports itself rather than
    /// running the budget out silently. An unmapped name resolves to itself,
    /// which is what makes a custom type a type rather than an error.
    ///
    /// **`/P /P` is not a cycle.** An entry mapping a name to itself is what a
    /// producer writes when it emits a role map covering every type it uses
    /// and some of those types are already standard, and it is the single most
    /// common entry in the wild: the first census over the fetched corpora
    /// reported 63 role-map loops, and treating the identity entry as a
    /// termination rather than as a cycle took that to the handful of files
    /// that have a real one. A warning that fires on the ordinary case is
    /// noise, and ruling 10's warnings are only actionable while they stay
    /// rare.
    fn resolve_role(&mut self, raw: &[u8]) -> String {
        let mut current = raw.to_vec();
        let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
        seen.insert(current.clone());
        for _ in 0..MAX_ROLE_MAP_HOPS {
            let Some(next) = self.role_map.get(&current) else {
                break;
            };
            if *next == current {
                break;
            }
            if !seen.insert(next.clone()) {
                let role = String::from_utf8_lossy(raw).into_owned();
                self.warn(StructureWarning::RoleMapLoop { role });
                break;
            }
            current = next.clone();
        }
        String::from_utf8_lossy(&current).into_owned()
    }
}

fn count(kids: &[StructKid], predicate: &dyn Fn(&StructKid) -> bool) -> usize {
    let mut total = 0;
    for kid in kids {
        if predicate(kid) {
            total += 1;
        }
        if let StructKid::Element(element) = kid {
            total += count(&element.kids, predicate);
        }
    }
    total
}

fn collect_elements<'a>(kids: &'a [StructKid], out: &mut Vec<&'a StructElement>) {
    for kid in kids {
        if let StructKid::Element(element) = kid {
            out.push(element);
            collect_elements(&element.kids, out);
        }
    }
}

// ---------------------------------------------------------------------------
// The join
// ---------------------------------------------------------------------------

/// Every character of a page, grouped by the `/MCID` in force when it was
/// shown.
fn chars_by_mcid(page: &TextPage) -> BTreeMap<u32, Vec<TextChar>> {
    let mut out: BTreeMap<u32, Vec<TextChar>> = BTreeMap::new();
    for line in page.lines() {
        for character in &line.chars {
            if let Some(mcid) = character.mcid {
                out.entry(mcid).or_default().push(character.clone());
            }
        }
    }
    out
}

/// The run of content under one element, between two of its child elements.
#[derive(Default)]
struct Run {
    chars: Vec<TextChar>,
    text: String,
    /// Whether any part of it came from an `/ActualText` (14.9.4).
    replaced: bool,
    /// Whether the element has yet to emit its first node, which is where its
    /// 14.9 values go.
    first: bool,
}

struct Join<'a> {
    index: u32,
    /// Whether a content item with no resolvable `/Pg` belongs to this page.
    ///
    /// True only for a one-page document. A `/K` integer whose element chain
    /// names no `/Pg` refers to *the* content stream when there is only one,
    /// and to an unknown one otherwise; claiming it on every page of a long
    /// document would report the same paragraph on all of them.
    unpaged_is_here: bool,
    by_mcid: BTreeMap<u32, Vec<TextChar>>,
    page: &'a TextPage,
    /// Every `/MCID` on this page some element claimed, whether or not the
    /// claim produced a node.
    claimed: BTreeSet<u32>,
    nodes: Vec<StructuredNode>,
}

impl Join<'_> {
    /// Walks one level of `/K`, which 14.8 makes reading order.
    ///
    /// `depth` is the depth of these kids; `under` is the element that owns
    /// them, and sits one level above. `suppressed` is set inside an
    /// `/ActualText` subtree, where identifiers are still claimed and nothing
    /// is emitted.
    fn kids(
        &mut self,
        kids: &[StructKid],
        under: Option<&StructElement>,
        depth: u32,
        suppressed: bool,
    ) {
        let mut run = Run {
            first: true,
            ..Run::default()
        };

        for kid in kids {
            match kid {
                StructKid::Content { page, mcid } => {
                    self.content(*page, *mcid, suppressed, &mut run);
                }
                // 14.7.4.3: an object has no glyphs in this page's stream.
                StructKid::Object(_) => {}
                StructKid::Element(element) => {
                    self.flush(under, depth, &mut run);
                    self.element(element, depth, suppressed);
                }
            }
        }

        self.flush(under, depth, &mut run);
        // An element with an `/Alt` and nothing drawn — a `Figure` whose
        // content is an image — still has something to say, and saying it is
        // the whole point of 14.9.3.
        if run.first && !suppressed {
            self.alt_only(under, depth);
        }
    }

    fn content(&mut self, page: Option<u32>, mcid: u32, suppressed: bool, run: &mut Run) {
        let here = page == Some(self.index) || (page.is_none() && self.unpaged_is_here);
        if !here {
            return;
        }
        self.claimed.insert(mcid);
        if suppressed {
            return;
        }
        // 14.9.4: an `/ActualText` on the *property list* replaces exactly the
        // sequence it encloses, which is this one.
        if let Some(actual) = self
            .page
            .mcid_props
            .get(&mcid)
            .and_then(|p| p.actual_text.clone())
        {
            run.text.push_str(&actual);
            run.replaced = true;
            return;
        }
        for character in self.by_mcid.get(&mcid).into_iter().flatten() {
            run.text.push_str(&character.text);
            run.chars.push(character.clone());
        }
    }

    fn element(&mut self, element: &StructElement, depth: u32, suppressed: bool) {
        // 14.9.4: an `/ActualText` on the element replaces everything it
        // encloses. The subtree is still walked, so its identifiers count as
        // claimed rather than becoming orphans — the content was accounted
        // for, it was simply spelled differently.
        if !suppressed {
            if let Some(actual) = &element.actual_text {
                self.nodes.push(StructuredNode {
                    raw_type: element.raw_type.clone(),
                    standard_type: element.standard_type.clone(),
                    depth,
                    text: actual.clone(),
                    source: TextSource::ActualText,
                    alt: element.alt.clone(),
                    lang: element.lang.clone(),
                    expansion: element.expansion.clone(),
                    chars: Vec::new(),
                });
                self.kids(&element.kids, Some(element), depth + 1, true);
                return;
            }
        }
        self.kids(&element.kids, Some(element), depth + 1, suppressed);
    }

    fn flush(&mut self, under: Option<&StructElement>, depth: u32, run: &mut Run) {
        let Some(element) = under else {
            run.chars.clear();
            run.text.clear();
            run.replaced = false;
            return;
        };
        if run.text.is_empty() {
            run.chars.clear();
            run.replaced = false;
            return;
        }

        // The 14.9 values sit on the first node an element produces. Repeating
        // them on every run of a paragraph holding three spans would make one
        // `/Alt` read as three.
        let (alt, lang, expansion) = if run.first {
            let from_list = run
                .chars
                .first()
                .and_then(|c| c.mcid)
                .and_then(|mcid| self.page.mcid_props.get(&mcid));
            (
                element
                    .alt
                    .clone()
                    .or_else(|| from_list.and_then(|p| p.alt.clone())),
                element
                    .lang
                    .clone()
                    .or_else(|| from_list.and_then(|p| p.lang.clone())),
                element
                    .expansion
                    .clone()
                    .or_else(|| from_list.and_then(|p| p.expansion.clone())),
            )
        } else {
            (None, None, None)
        };

        self.nodes.push(StructuredNode {
            raw_type: element.raw_type.clone(),
            standard_type: element.standard_type.clone(),
            depth: depth.saturating_sub(1),
            text: std::mem::take(&mut run.text),
            source: if run.replaced {
                TextSource::ActualText
            } else {
                TextSource::Glyphs
            },
            alt,
            lang,
            expansion,
            chars: std::mem::take(&mut run.chars),
        });
        run.replaced = false;
        run.first = false;
    }

    /// An element that drew nothing on this page but describes something on
    /// it.
    fn alt_only(&mut self, under: Option<&StructElement>, depth: u32) {
        let Some(element) = under else {
            return;
        };
        if element.alt.is_none() && element.expansion.is_none() {
            return;
        }
        // Only for an element this page actually holds. Without the test, a
        // `Figure` on page 40 has its description reported on page 1.
        let here = element.page == Some(self.index)
            || element.kids.iter().any(
                |kid| matches!(kid, StructKid::Content { page, .. } if *page == Some(self.index)),
            );
        if !here {
            return;
        }
        self.nodes.push(StructuredNode {
            raw_type: element.raw_type.clone(),
            standard_type: element.standard_type.clone(),
            depth: depth.saturating_sub(1),
            text: String::new(),
            source: TextSource::Glyphs,
            alt: element.alt.clone(),
            lang: element.lang.clone(),
            expansion: element.expansion.clone(),
            chars: Vec::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document whose catalog carries `/StructTreeRoot 5 0 R` and one page
    /// at 3 0 R.
    ///
    /// `root` is the structure tree root's body, `objects` everything the
    /// tree points at, numbered from 10 by convention below. The page carries
    /// `/StructParents 0` so the parent-tree cross-check has a key to look
    /// up; a fixture about the `/K` walk alone simply leaves `/ParentTree`
    /// out and the check finds nothing to compare.
    fn document(root: &str, objects: &str) -> Arc<CosDocument> {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R \
 /MarkInfo << /Marked true >> >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /StructParents 0 >>\nendobj\n\
5 0 obj\n<< /Type /StructTreeRoot {root} >>\nendobj\n\
{objects}\
trailer\n<< /Size 400 /Root 1 0 R >>\n%%EOF\n"
        );
        let doc = crate::Document::open(bytes.into_bytes()).expect("it opens");
        doc.page(0).expect("a page").doc.clone()
    }

    fn tree(root: &str, objects: &str) -> StructureTree {
        bind(&document(root, objects)).expect("a structure tree")
    }

    /// The simplest tagged shape there is, and the baseline every hostile
    /// fixture below is a deformation of.
    #[test]
    fn a_document_element_holding_a_paragraph_reads_back() {
        let tree = tree(
            "/K 10 0 R",
            "10 0 obj\n<< /Type /StructElem /S /Document /K [11 0 R] >>\nendobj\n\
             11 0 obj\n<< /Type /StructElem /S /P /Pg 3 0 R /K [0 1] >>\nendobj\n",
        );

        assert!(tree.marked, "/MarkInfo /Marked true");
        assert!(!tree.suspects);
        assert_eq!(tree.element_count(), 2);
        assert_eq!(tree.content_count(), 2);
        assert_eq!(tree.object_count(), 0);
        assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);

        let elements = tree.elements();
        assert_eq!(elements[0].standard_type, "Document");
        assert_eq!(elements[1].standard_type, "P");
        assert_eq!(elements[1].page, Some(0), "/Pg resolved to a page index");
        assert!(matches!(
            elements[1].kids.as_slice(),
            [
                StructKid::Content {
                    page: Some(0),
                    mcid: 0
                },
                StructKid::Content {
                    page: Some(0),
                    mcid: 1
                }
            ]
        ));
    }

    /// A `/K` that reaches back up its own path. It must terminate, it must
    /// say which element closed the loop, and it must keep what it read
    /// before the loop rather than discarding the branch.
    #[test]
    fn a_kid_cycle_is_cut_and_named() {
        let tree = tree(
            "/K 10 0 R",
            "10 0 obj\n<< /S /Document /K [11 0 R] >>\nendobj\n\
             11 0 obj\n<< /S /Sect /K [12 0 R] >>\nendobj\n\
             12 0 obj\n<< /S /P /K [10 0 R] >>\nendobj\n",
        );

        assert_eq!(tree.element_count(), 3, "the three real elements are kept");
        assert!(
            tree.warnings.contains(&StructureWarning::KidCycle {
                element: ObjRef::new(10, 0),
            }),
            "the cycle names the element it closed on: {:?}",
            tree.warnings
        );
    }

    /// The same element named twice from *different* branches is not a cycle,
    /// and both branches keep it.
    ///
    /// The path set is inserted on entry and removed on exit precisely so
    /// this stays true — a global visited set would silently drop the second
    /// mention, which in a real file is a shared subtree and not damage.
    #[test]
    fn a_subtree_named_from_two_places_is_read_twice() {
        let tree = tree(
            "/K 10 0 R",
            "10 0 obj\n<< /S /Document /K [11 0 R 12 0 R] >>\nendobj\n\
             11 0 obj\n<< /S /Sect /K [13 0 R] >>\nendobj\n\
             12 0 obj\n<< /S /Sect /K [13 0 R] >>\nendobj\n\
             13 0 obj\n<< /S /P /Pg 3 0 R /K 7 >>\nendobj\n",
        );

        assert_eq!(tree.element_count(), 5, "10, 11, 12 and 13 twice");
        assert_eq!(tree.content_count(), 2);
        assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
    }

    /// Nesting past [`MAX_STRUCTURE_DEPTH`]. The walk stops, says where, and
    /// the elements above the cap are all still there.
    #[test]
    fn nesting_past_the_depth_cap_is_refused_at_the_cap() {
        let deep = MAX_STRUCTURE_DEPTH as usize + 20;
        let mut objects = String::new();
        for step in 0..deep {
            let number = 10 + step;
            objects.push_str(&format!(
                "{number} 0 obj\n<< /S /Sect /K [{} 0 R] >>\nendobj\n",
                number + 1
            ));
        }
        objects.push_str(&format!(
            "{} 0 obj\n<< /S /P /Pg 3 0 R /K 0 >>\nendobj\n",
            10 + deep
        ));

        let tree = tree("/K 10 0 R", &objects);
        assert_eq!(
            tree.element_count(),
            MAX_STRUCTURE_DEPTH as usize + 1,
            "everything down to the cap, and nothing under it"
        );
        assert!(
            tree.warnings
                .iter()
                .any(|w| matches!(w, StructureWarning::DepthCapped { .. })),
            "{:?}",
            tree.warnings
        );
    }

    /// A tree that names the same subtree twice at every level describes more
    /// elements than there are atoms in it. It must terminate on the element
    /// budget rather than on the depth cap, because the depth cap alone
    /// bounds 2^256 of them.
    #[test]
    fn a_doubling_tree_stops_on_the_element_budget() {
        let levels = 40usize;
        let mut objects = String::new();
        for step in 0..levels {
            let number = 10 + step;
            objects.push_str(&format!(
                "{number} 0 obj\n<< /S /Sect /K [{next} 0 R {next} 0 R] >>\nendobj\n",
                next = number + 1
            ));
        }
        objects.push_str(&format!(
            "{} 0 obj\n<< /S /P /Pg 3 0 R /K 0 >>\nendobj\n",
            10 + levels
        ));

        let tree = tree("/K 10 0 R", &objects);
        assert!(
            tree.element_count() <= MAX_STRUCTURE_ELEMENTS,
            "the budget bounds the tree, got {}",
            tree.element_count()
        );
        assert!(
            tree.warnings.contains(&StructureWarning::ElementCapped),
            "and it says so: {:?}",
            tree.warnings
        );
    }

    /// 14.7.3: the role map rewrites a type, and a type it does not mention
    /// rewrites to itself.
    ///
    /// Both halves in one fixture, because a build that returned the *mapped*
    /// name for everything and a build that returned the *raw* name for
    /// everything each pass a test with only one of them.
    #[test]
    fn the_role_map_maps_what_it_names_and_nothing_else() {
        let tree = tree(
            "/K 10 0 R /RoleMap << /Chapitre /Sect >>",
            "10 0 obj\n<< /S /Document /K [11 0 R 12 0 R] >>\nendobj\n\
             11 0 obj\n<< /S /Chapitre >>\nendobj\n\
             12 0 obj\n<< /S /Encadre >>\nendobj\n",
        );

        let elements = tree.elements();
        assert_eq!(elements[1].raw_type, "Chapitre", "the file's own name");
        assert_eq!(elements[1].standard_type, "Sect", "and what it maps to");
        assert_eq!(elements[2].raw_type, "Encadre");
        assert_eq!(
            elements[2].standard_type, "Encadre",
            "an unmapped custom type is itself, not an error and not a /Span"
        );
    }

    /// A role map that loops. Resolution stops, names the type it entered by,
    /// and the element keeps a type rather than losing one.
    #[test]
    fn a_role_map_loop_terminates_and_is_named() {
        let tree = tree(
            "/K 10 0 R /RoleMap << /Foo /Bar /Bar /Baz /Baz /Foo >>",
            "10 0 obj\n<< /S /Foo >>\nendobj\n",
        );

        assert!(
            tree.warnings.contains(&StructureWarning::RoleMapLoop {
                role: "Foo".to_string(),
            }),
            "{:?}",
            tree.warnings
        );
        let elements = tree.elements();
        assert_eq!(elements[0].raw_type, "Foo");
        assert!(
            !elements[0].standard_type.is_empty(),
            "a loop must not cost the element its type"
        );
    }

    /// `/P /P` is the commonest entry a real role map holds, and it is not a
    /// cycle.
    ///
    /// A producer emitting a role map that covers every type it uses writes
    /// one of these for every type that is already standard. Reporting them
    /// as loops made the first corpus census say 63 when the real number is a
    /// handful, and a warning that fires on the ordinary case is noise rather
    /// than provenance.
    #[test]
    fn an_identity_role_map_entry_is_not_a_loop() {
        let tree = tree(
            "/K 10 0 R /RoleMap << /P /P /Span /Span /Chapitre /Sect >>",
            "10 0 obj\n<< /S /P /K [11 0 R] >>\nendobj\n\
             11 0 obj\n<< /S /Chapitre >>\nendobj\n",
        );

        assert!(tree.warnings.is_empty(), "{:?}", tree.warnings);
        let elements = tree.elements();
        assert_eq!(elements[0].standard_type, "P");
        assert_eq!(
            elements[1].standard_type, "Sect",
            "and a real mapping in the same map still applies"
        );
    }

    /// Every shape a `/K` array can hold that is not an ordinary element:
    /// 14.7.4's `/MCR` and `/OBJR`, a bare integer, and four kinds of
    /// nonsense.
    ///
    /// Written as one array because the failure that matters is
    /// **desynchronization** — a walk that mis-reads the string and then
    /// mis-reads everything after it. Testing each shape alone cannot catch
    /// that.
    #[test]
    fn odd_kids_are_read_or_dropped_without_losing_their_neighbours() {
        let tree = tree(
            "/K 10 0 R",
            "10 0 obj\n<< /S /Document /Pg 3 0 R /K [\n\
                3\n\
                (a string)\n\
                null\n\
                << /Type /MCR /MCID 4 /Pg 3 0 R >>\n\
                true\n\
                << /Type /OBJR /Obj 99 0 R >>\n\
                [1 2]\n\
                << /K 5 >>\n\
                7\n\
             ] >>\nendobj\n",
        );

        let root = tree.elements()[0];
        let mcids: Vec<u32> = root
            .kids
            .iter()
            .filter_map(|kid| match kid {
                StructKid::Content { mcid, .. } => Some(*mcid),
                _ => None,
            })
            .collect();
        assert_eq!(
            mcids,
            vec![3, 4, 7],
            "the integer, the /MCR and the integer after the nonsense"
        );
        assert_eq!(tree.object_count(), 1, "the /OBJR");
        assert!(
            matches!(
                root.kids.iter().find_map(|kid| match kid {
                    StructKid::Object(reference) => Some(*reference),
                    _ => None,
                }),
                Some(reference) if reference == ObjRef::new(99, 0)
            ),
            "and it names the object it referred to"
        );
        // `<< /K 5 >>` is a dictionary with no `/S`: an element, untyped.
        assert_eq!(tree.element_count(), 2);
        assert!(
            tree.warnings
                .iter()
                .any(|w| matches!(w, StructureWarning::UnreadableKid { .. })),
            "the string, the boolean and the nested array were dropped and \
             said so: {:?}",
            tree.warnings
        );
    }

    /// An element with no `/S`. It is kept — its kids are still content — and
    /// the missing type is a warning naming it rather than a guess.
    #[test]
    fn an_element_without_a_type_is_kept_and_named() {
        let tree = tree(
            "/K 10 0 R",
            "10 0 obj\n<< /Type /StructElem /Pg 3 0 R /K 2 >>\nendobj\n",
        );

        assert_eq!(tree.element_count(), 1);
        assert_eq!(tree.content_count(), 1, "its content survived");
        assert_eq!(tree.elements()[0].raw_type, "");
        assert!(tree.warnings.contains(&StructureWarning::UntypedElement {
            element: ObjRef::new(10, 0),
        }));
    }

    /// `/Pg` is written once, on the element that owns the content items, and
    /// its descendants inherit it — which is the shape nearly every producer
    /// writes. An element's own `/Pg` still wins over the inherited one.
    #[test]
    fn a_page_is_inherited_down_the_tree_and_overridden_by_its_own() {
        let tree = tree(
            "/K 10 0 R",
            "10 0 obj\n<< /S /Document /Pg 3 0 R /K [11 0 R] >>\nendobj\n\
             11 0 obj\n<< /S /P /K [0 << /Type /MCR /MCID 9 >>] >>\nendobj\n",
        );

        let paragraph = tree.elements()[1];
        assert_eq!(paragraph.page, Some(0), "inherited from the document");
        for kid in &paragraph.kids {
            assert!(
                matches!(kid, StructKid::Content { page: Some(0), .. }),
                "an /MCR with no /Pg of its own inherits too: {kid:?}"
            );
        }
    }

    /// A document with no `/StructTreeRoot` — which is most of them — reports
    /// having none rather than an empty one.
    #[test]
    fn an_untagged_document_has_no_tree_at_all() {
        let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n"
            .to_vec();
        let doc = crate::Document::open(bytes).expect("it opens");
        assert!(doc.structure().is_none());
        assert!(doc.page(0).expect("a page").structured_text().is_none());
    }

    /// A `/StructTreeRoot` that is not a dictionary, and one whose `/K` is
    /// nonsense. Neither is a tree, and neither panics.
    #[test]
    fn a_damaged_root_degrades_rather_than_failing() {
        let unreadable_kid = tree("/K (not a kid)", "");
        assert_eq!(unreadable_kid.element_count(), 0);
        assert_eq!(unreadable_kid.content_count(), 0);

        let no_kids = tree("", "");
        assert_eq!(no_kids.element_count(), 0);
        assert!(no_kids.marked, "the /MarkInfo is still read");
    }
}
