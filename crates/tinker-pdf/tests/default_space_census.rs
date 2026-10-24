//! What the corpora do with default colour spaces (8.6.5.6): roadmap K-02.4,
//! `docs/design/icc.md`, "Default colour spaces".
//!
//! A resource dictionary's `/ColorSpace` may name `/DefaultGray`,
//! `/DefaultRGB` or `/DefaultCMYK`, and each stands in for its device space
//! wherever that space is selected under the dictionary. K-02.2 builds the
//! substitution; this census says what it reaches in documents real producers
//! wrote. For each file it counts:
//!
//! - **where a default sits**, by the scope whose `/Resources` carry it: a
//!   page, a form XObject, a tiling pattern, a Type 3 font, an annotation
//!   appearance or a soft mask's group;
//! - **whether it is asked**: the content run under that scope selects the
//!   device family the default replaces, so the default changes what is
//!   drawn. A default nothing selects under is inert;
//! - **where the readings part**: a form with `/Resources` of its own and no
//!   default for a family selects that family while a scope it is invoked
//!   under has one. `docs/design/icc.md` reads 8.10.1 as stopping the default
//!   at such a form, which then draws the device space; a reader that carries
//!   a scope's defaults into the forms it invokes draws the substitute. An
//!   appearance stream and a soft mask's group are form XObjects, and count.
//!
//! ```sh
//! cargo test -p tinker-pdf --test default_space_census -- --ignored --nocapture
//! ```
//!
//! **`RAN`/`SKIPPED` is printed on the first line.** Point it at the tree with
//! `TINKER_CORPUS`; the default is `corpus/files` beside the workspace. With
//! `TINKER_CORPUS_REQUIRED` set, a missing or empty corpus fails rather than
//! skips. **No floor is held yet**: K-02.5 dispatches the corpus job and
//! commits the first figures it prints.
//!
//! # Read from bytes
//!
//! As `icc_census.rs` reads profiles: the COS layer opens the file and decodes
//! its streams, and the scopes, the colour spaces and the content operators
//! are read here, by hand, never through `PageResources` or the interpreter,
//! the code K-02.2 changes. The census cannot agree with that code by sharing
//! its mistake. It reads each file twice:
//!
//! - **The sweep** reads every object the cross-reference table lists, every
//!   object a reference in one of them or in the trailer names, and every
//!   dictionary inside them, and notes each family a dictionary names a
//!   default for, whether or not anything paints under it.
//! - **The walk** starts at each page and at each appearance stream of its
//!   annotations, and follows what the content invokes: `Do` into a form,
//!   `scn` into a tiling or shading pattern, `Tf` into a Type 3 font's glyphs,
//!   `gs` into a soft mask's group. A default it finds is one the sweep found
//!   too, and the corpus test asserts that. A default the sweep finds and no
//!   walk reaches sits in a form nothing invokes, an AcroForm's `/DR`, or a
//!   path this walk does not take, and is counted apart.
//!
//! The walk only runs on a file whose sweep found a default: with none,
//! there is nothing for it to find.
//!
//! # The rules it reads by
//!
//! - **Scope.** A stream with `/Resources` of its own runs in them (8.10.1,
//!   Table 75, 9.6.5, 12.5.5, 11.6.5.2). One without runs in the scope that
//!   invoked it, as the engine runs a form or a pattern without them; 7.8.3
//!   sends a form's names to the page's resources and 9.6.5 a Type 3 font's,
//!   which differs only under a form with resources of its own. An appearance
//!   stream is read as a form the page invokes, so one without `/Resources`
//!   reads the page's.
//! - **Selection.** What 8.6.5.6 remaps: `g`/`G`, `rg`/`RG` and `k`/`K`
//!   (8.6.8); `cs`/`CS` naming a device space, or a `/ColorSpace` resource that
//!   is one or is built over one (an `/Indexed` base, a `[/Pattern base]`'s
//!   base, a `/Separation` or `/DeviceN` alternate); an image XObject's, an
//!   inline image's and a shading's space, under `sh` or in a shading pattern.
//!   The engine's own readings of a device space remap too
//!   (`docs/design/icc.md`): `/G`, `/RGB`, `/CMYK` and a bare `/CalGray` or
//!   `/CalRGB` where no resource has the name.
//! - **Not selections.** `sc` and `scn` with no space named before them, and
//!   the initial DeviceGray a page paints in before it selects anything: no
//!   operator selects either. A `d1` glyph's colours and an uncoloured tiling
//!   cell's are ignored (9.6.5, 8.7.3.3), and a group's `/CS` and a soft
//!   mask's `/BC` paint nothing.
//! - **Counted as the standard reads them, where the engine does not.** A
//!   JPEG's or JPEG 2000's `/ColorSpace`, which the renderer never reads (the
//!   design's non-goal), and a Type 3 font's or a soft-mask group's own
//!   `/Resources`, which the engine's scopes do not read.
//! - **A Type 3 font is read whole**: every `/CharProcs` entry, not only the
//!   codes the text shows.
//! - **A default is an entry**, whatever its value; a `null` one is absent
//!   (7.3.9). Whether the clause allows it is K-02.2's question.
//!
//! # Ruling 13
//!
//! The corpora supply bytes; every number here is this file reading them.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tinker_pdf::Document;
use tinker_pdf_cos::lexer::{is_delimiter, is_regular, is_whitespace, Lexer};
use tinker_pdf_cos::{
    pages, parse_object_at, CosDocument, Dict, LadderLevel, Name, NameTable, ObjRef, Object,
    WarningSink, XrefEntry,
};

/// Printed once when the census read the corpora. CI greps it.
const RAN: &str = "default-space-census: RAN";

/// Printed once when it could not.
const SKIPPED: &str = "default-space-census: SKIPPED";

// ---- what is counted ----------------------------------------------------------

/// The three device families a default replaces (8.6.5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Family {
    Gray,
    Rgb,
    Cmyk,
}

impl Family {
    const ALL: [Family; 3] = [Family::Gray, Family::Rgb, Family::Cmyk];

    fn bit(self) -> u8 {
        1 << self as u8
    }

    fn label(self) -> &'static str {
        match self {
            Family::Gray => "Gray",
            Family::Rgb => "RGB",
            Family::Cmyk => "CMYK",
        }
    }
}

/// The kind of stream whose `/Resources` a default sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Scope {
    /// A page's `/Resources`, inherited down the page tree (7.7.3.4).
    Page,
    /// A form XObject's own, invoked by `Do` (8.10.1).
    Form,
    /// A tiling pattern's own (8.7.3.1, Table 75).
    TilingPattern,
    /// A Type 3 font's (9.6.5).
    Type3Font,
    /// An annotation appearance stream's own (12.5.5).
    Appearance,
    /// A soft mask's group XObject's own, `/SMask`'s `/G` (11.6.5.2).
    SoftMaskGroup,
}

impl Scope {
    const ALL: [Scope; 6] = [
        Scope::Page,
        Scope::Form,
        Scope::TilingPattern,
        Scope::Type3Font,
        Scope::Appearance,
        Scope::SoftMaskGroup,
    ];

    /// The scopes that are form XObjects, where the readings can part.
    const FORMS: [Scope; 3] = [Scope::Form, Scope::Appearance, Scope::SoftMaskGroup];

    fn label(self) -> &'static str {
        match self {
            Scope::Page => "page",
            Scope::Form => "form",
            Scope::TilingPattern => "tiling pattern",
            Scope::Type3Font => "Type 3 font",
            Scope::Appearance => "annotation appearance",
            Scope::SoftMaskGroup => "soft-mask group",
        }
    }
}

/// What one file carries.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FileCensus {
    /// The families some dictionary in the file names a default for, found
    /// by the sweep, reached or not.
    swept: BTreeSet<Family>,
    /// The scopes the walk reaches whose `/ColorSpace` names a default.
    carried: BTreeSet<(Scope, Family)>,
    /// Of those, the ones whose content selects the family under the default.
    selected: BTreeSet<(Scope, Family)>,
    /// A form with `/Resources` of its own and no default for the family,
    /// selecting it under a scope that has one.
    parted: BTreeSet<(Scope, Family)>,
    /// Whether the walk stopped at its depth or run budget.
    cut_short: bool,
}

/// What one file carries, or `None` when it does not open.
///
/// Most encrypted documents in the corpora open on the empty user password,
/// and a resource dictionary in an object stream is unreadable until the file
/// key exists.
fn census_of(bytes: Vec<u8>) -> Option<FileCensus> {
    let document = Document::open(bytes).ok()?;
    if document.is_encrypted() {
        let _ = document.authenticate("");
    }
    let cos = document.cos();
    let keys = Keys::new(cos);
    let swept = sweep(cos, &keys);
    let mut walk = Walk {
        cos,
        keys,
        census: FileCensus {
            swept,
            ..FileCensus::default()
        },
        visited: HashSet::new(),
        runs: 0,
    };
    if !walk.census.swept.is_empty() {
        walk.pages();
    }
    Some(walk.census)
}

// ---- the names it asks for ------------------------------------------------------

/// The names the census reads, interned in the document's own table.
struct Keys {
    defaults: [Name; 3],
    devices: [Name; 3],
    /// Names the engine draws as a device space where no resource has them:
    /// 8.9.7's abbreviations, and a bare `/CalGray` or `/CalRGB`.
    read_as_device: [(Name, Family); 5],
    color_space: Name,
    /// `/ColorSpace` abbreviated, in an inline image (8.9.7).
    cs: Name,
    xobject: Name,
    /// The resource category, and the family.
    pattern: Name,
    font: Name,
    ext_g_state: Name,
    shading: Name,
    subtype: Name,
    form: Name,
    image: Name,
    /// `/ImageMask` and its inline abbreviation `/IM`.
    masks: [Name; 2],
    pattern_type: Name,
    paint_type: Name,
    type3: Name,
    char_procs: Name,
    smask: Name,
    /// A soft mask's `/G`.
    group: Name,
    annots: Name,
    ap: Name,
    /// `/N`, `/R`, `/D` (12.5.5).
    appearances: [Name; 3],
    /// `/Indexed` and its inline abbreviation `/I`.
    indexed: [Name; 2],
    /// `/Separation` and `/DeviceN`, whose third element is the alternate.
    alternated: [Name; 2],
}

impl Keys {
    fn new(cos: &CosDocument) -> Keys {
        let name = |bytes: &[u8]| cos.intern(bytes);
        Keys {
            defaults: [
                name(b"DefaultGray"),
                name(b"DefaultRGB"),
                name(b"DefaultCMYK"),
            ],
            devices: [name(b"DeviceGray"), name(b"DeviceRGB"), name(b"DeviceCMYK")],
            read_as_device: [
                (name(b"G"), Family::Gray),
                (name(b"RGB"), Family::Rgb),
                (name(b"CMYK"), Family::Cmyk),
                (name(b"CalGray"), Family::Gray),
                (name(b"CalRGB"), Family::Rgb),
            ],
            color_space: name(b"ColorSpace"),
            cs: name(b"CS"),
            xobject: name(b"XObject"),
            pattern: name(b"Pattern"),
            font: name(b"Font"),
            ext_g_state: name(b"ExtGState"),
            shading: name(b"Shading"),
            subtype: name(b"Subtype"),
            form: name(b"Form"),
            image: name(b"Image"),
            masks: [name(b"ImageMask"), name(b"IM")],
            pattern_type: name(b"PatternType"),
            paint_type: name(b"PaintType"),
            type3: name(b"Type3"),
            char_procs: name(b"CharProcs"),
            smask: name(b"SMask"),
            group: name(b"G"),
            annots: name(b"Annots"),
            ap: name(b"AP"),
            appearances: [name(b"N"), name(b"R"), name(b"D")],
            indexed: [name(b"Indexed"), name(b"I")],
            alternated: [name(b"Separation"), name(b"DeviceN")],
        }
    }

    /// The family a device space's own name names (8.6.4).
    fn device(&self, name: Name) -> Option<Family> {
        Family::ALL
            .into_iter()
            .zip(self.devices)
            .find_map(|(family, device)| (device == name).then_some(family))
    }

    /// The family the engine draws a name as, where no resource has it.
    fn read_as_device(&self, name: Name) -> Option<Family> {
        self.read_as_device
            .iter()
            .find_map(|&(read, family)| (read == name).then_some(family))
    }
}

/// The families `dict` names a default for, as bits: an entry whose value is
/// not `null` (7.3.9).
fn defaults_in(cos: &CosDocument, keys: &Keys, dict: &Dict) -> u8 {
    Family::ALL
        .into_iter()
        .zip(keys.defaults)
        .filter(|&(_, key)| {
            dict.get(key)
                .is_some_and(|value| !cos.resolve(value).is_null())
        })
        .fold(0, |bits, (family, _)| bits | family.bit())
}

fn families(bits: u8) -> impl Iterator<Item = Family> {
    Family::ALL
        .into_iter()
        .filter(move |family| bits & family.bit() != 0)
}

// ---- the sweep ------------------------------------------------------------------

/// Every family a dictionary anywhere in the file names a default for: in each
/// object the cross-reference table lists, each object a reference in one of
/// those or in the trailer names, and each dictionary inside them.
///
/// Following the references is what makes the sweep reach everything the walk
/// can: a damaged file's reference to an object its table marks free or does
/// not list is answered from the repair scan (`CosDocument::get`), and the
/// walk follows such references like any other.
fn sweep(cos: &CosDocument, keys: &Keys) -> BTreeSet<Family> {
    let mut found = 0;
    let mut queue: Vec<u32> = cos
        .xref()
        .iter()
        .filter(|(_, entry)| !matches!(entry, XrefEntry::Free { .. }))
        .map(|(number, _)| number)
        .collect();
    let trailer = Object::Dict(cos.trailer().clone());
    found |= defaults_within(cos, keys, &trailer, &mut queue);
    let mut seen = HashSet::new();
    while let Some(number) = queue.pop() {
        if number == 0 || !seen.insert(number) {
            continue;
        }
        // The number is the identity; `get` reads past a stale generation.
        if let Ok(object) = cos.get(ObjRef::new(number, 0)) {
            found |= defaults_within(cos, keys, &object, &mut queue);
        }
    }
    families(found).collect()
}

/// The defaults named by any dictionary in `object`, its direct parts
/// included. A reference is an object of its own: its number goes on `refs`.
fn defaults_within(cos: &CosDocument, keys: &Keys, object: &Object, refs: &mut Vec<u32>) -> u8 {
    let dict = match object {
        Object::Dict(dict) => dict,
        Object::Stream(stream) => &stream.dict,
        Object::Array(items) => {
            return items.iter().fold(0, |bits, item| {
                bits | defaults_within(cos, keys, item, refs)
            });
        }
        Object::Ref(reference) => {
            refs.push(reference.num);
            return 0;
        }
        _ => return 0,
    };
    dict.iter()
        .fold(defaults_in(cos, keys, dict), |bits, (_, value)| {
            bits | defaults_within(cos, keys, value, refs)
        })
}

// ---- the walk -------------------------------------------------------------------

/// How deep forms, patterns, glyphs and soft masks may nest.
const MAX_DEPTH: u32 = 32;

/// How many streams one file's walk reads.
const MAX_RUNS: usize = 100_000;

/// How deep one colour space may name another.
const MAX_SPACE_DEPTH: u32 = 8;

/// A resource dictionary a stream's names resolve in.
#[derive(Clone)]
struct ScopeOf {
    kind: Scope,
    /// The object whose `/Resources` these are.
    owner: ObjRef,
    resources: Option<Dict>,
    /// The families its `/ColorSpace` names a default for, as bits.
    defaults: u8,
}

/// Where in the walk a stream runs.
#[derive(Clone, Copy)]
struct Run<'s> {
    scope: &'s ScopeOf,
    /// The families a default is named for here or in any scope this one was
    /// invoked under: what a reader carrying defaults inward would apply.
    carried: u8,
    /// A `d1` glyph or an uncoloured tiling cell, whose colours are ignored
    /// (9.6.5, 8.7.3.3).
    shape_only: bool,
    /// A Type 3 glyph description, where `d1` means shape only.
    glyph: bool,
    depth: u32,
}

/// A stream run once as one kind of scope, in one scope under one carried
/// set.
type Visit = (ObjRef, Scope, ObjRef, u8, bool);

struct Walk<'a> {
    cos: &'a CosDocument,
    keys: Keys,
    census: FileCensus,
    visited: HashSet<Visit>,
    runs: usize,
}

impl Walk<'_> {
    /// Every page's content, then every appearance stream its annotations
    /// carry, read as forms the page invokes.
    fn pages(&mut self) {
        for mut page in pages::collect(self.cos) {
            let content = pages::content_bytes(self.cos, &page);
            let scope = self.scope(Scope::Page, page.reference, page.resources.take());
            let run = Run {
                scope: &scope,
                carried: scope.defaults,
                shape_only: false,
                glyph: false,
                depth: 0,
            };
            self.runs += 1;
            self.content(&content, run);
            self.annotations(page.reference, run);
        }
    }

    /// A scope over `resources`, its defaults counted as carried.
    fn scope(&mut self, kind: Scope, owner: ObjRef, resources: Option<Dict>) -> ScopeOf {
        let defaults = resources.as_ref().map_or(0, |resources| {
            let spaces = self.cos.resolve_key(resources, self.keys.color_space);
            spaces
                .as_dict()
                .map_or(0, |spaces| defaults_in(self.cos, &self.keys, spaces))
        });
        for family in families(defaults) {
            self.census.carried.insert((kind, family));
        }
        ScopeOf {
            kind,
            owner,
            resources,
            defaults,
        }
    }

    fn annotations(&mut self, page: ObjRef, run: Run<'_>) {
        let Ok(object) = self.cos.get(page) else {
            return;
        };
        let Some(dict) = object.as_dict() else {
            return;
        };
        let annots = self.cos.resolve_key(dict, self.keys.annots);
        let Some(annots) = annots.as_array() else {
            return;
        };
        for annot in annots {
            let annot = self.cos.resolve(annot);
            let Some(annot) = annot.as_dict() else {
                continue;
            };
            let appearances = self.cos.resolve_key(annot, self.keys.ap);
            let Some(appearances) = appearances.as_dict() else {
                continue;
            };
            for key in self.keys.appearances {
                let Some(value) = appearances.get(key) else {
                    continue;
                };
                // A stream, or a dictionary of them by appearance state.
                let resolved = self.cos.resolve(value);
                let streams: Vec<ObjRef> = match (&*resolved, value.as_objref()) {
                    (Object::Stream(_), Some(stream)) => vec![stream],
                    (Object::Dict(states), _) => {
                        states.iter().filter_map(|(_, s)| s.as_objref()).collect()
                    }
                    _ => Vec::new(),
                };
                for stream in streams {
                    self.enter(stream, Scope::Appearance, run);
                }
            }
        }
    }

    /// Runs a stream `run` invokes: in its own `/Resources` when it has them,
    /// in the invoker's when it has none.
    fn enter(&mut self, stream: ObjRef, kind: Scope, run: Run<'_>) {
        if run.depth >= MAX_DEPTH || self.runs >= MAX_RUNS {
            self.census.cut_short = true;
            return;
        }
        // Invoked from this scope under this carried set before: nothing new.
        let invoked = Self::visit(stream, kind, run);
        if !self.visited.insert(invoked) {
            return;
        }
        let Ok(object) = self.cos.get(stream) else {
            return;
        };
        let Some(body) = object.as_stream() else {
            return;
        };
        let own = self
            .cos
            .resolve_key(&body.dict, Name::RESOURCES)
            .as_dict()
            .cloned();
        let uncoloured = kind == Scope::TilingPattern
            && self
                .cos
                .resolve_key(&body.dict, self.keys.paint_type)
                .as_int()
                == Some(2);
        let scope = match own {
            Some(resources) => self.scope(kind, stream, Some(resources)),
            None => run.scope.clone(),
        };
        let inner = Run {
            scope: &scope,
            carried: run.carried | scope.defaults,
            shape_only: run.shape_only || uncoloured,
            glyph: false,
            depth: run.depth + 1,
        };
        // Run in this scope under this carried set before, invoked from
        // another: a form with resources of its own, shared between pages. A
        // stream run in the invoker's scope is the visit just recorded.
        let ran = Self::visit(stream, kind, inner);
        if ran != invoked && !self.visited.insert(ran) {
            return;
        }
        let Ok(content) = self.cos.stream_decoded(stream) else {
            return;
        };
        self.runs += 1;
        self.content(&content, inner);
    }

    /// What makes a run of `object` the same run again: the kind of scope it
    /// is reached as, the scope it runs or is invoked in, what that carries,
    /// and whether its colours count. A form reused as an appearance stream
    /// is reached twice, once as each.
    fn visit(object: ObjRef, kind: Scope, run: Run<'_>) -> Visit {
        (object, kind, run.scope.owner, run.carried, run.shape_only)
    }

    /// The value of `name` in the scope's `category`, and its reference when
    /// it was given by one.
    fn resource(
        &self,
        scope: &ScopeOf,
        category: Name,
        name: Name,
    ) -> Option<(Arc<Object>, Option<ObjRef>)> {
        let entries = self.cos.resolve_key(scope.resources.as_ref()?, category);
        let value = entries.as_dict()?.get(name)?;
        Some((self.cos.resolve(value), value.as_objref()))
    }

    /// The families a space selected by name selects. A device space's own
    /// name never names a resource (8.6.8); any other name is looked up first,
    /// and read as the engine reads it only where no resource has it.
    fn name_families(&self, name: Name, scope: &ScopeOf, depth: u32) -> u8 {
        if let Some(family) = self.keys.device(name) {
            return family.bit();
        }
        if name == self.keys.pattern {
            return 0;
        }
        if let Some((space, _)) = self.resource(scope, self.keys.color_space, name) {
            return self.space_families(&space, scope, depth + 1);
        }
        self.keys.read_as_device(name).map_or(0, Family::bit)
    }

    /// The device families a colour space selects: itself, or the space it is
    /// built over (8.6.5.6).
    fn space_families(&self, space: &Object, scope: &ScopeOf, depth: u32) -> u8 {
        if depth > MAX_SPACE_DEPTH {
            return 0;
        }
        let space = self.cos.resolve(space);
        let items = match &*space {
            Object::Name(name) => return self.name_families(*name, scope, depth),
            Object::Array(items) => items,
            _ => return 0,
        };
        let Some(&Object::Name(family)) = items.first() else {
            return 0;
        };
        let under = if self.keys.indexed.contains(&family) || family == self.keys.pattern {
            items.get(1)
        } else if self.keys.alternated.contains(&family) {
            items.get(2)
        } else if items.len() == 1 {
            return self.name_families(family, scope, depth);
        } else {
            // ICCBased, CalGray, CalRGB, Lab: no device space.
            None
        };
        under.map_or(0, |under| self.space_families(under, scope, depth + 1))
    }

    /// The selection of `found` in `run`'s scope: under its default, or, in a
    /// form with resources of its own, under one only a scope outside has.
    fn select(&mut self, found: u8, run: Run<'_>) {
        if run.shape_only {
            return;
        }
        for family in families(found) {
            if run.scope.defaults & family.bit() != 0 {
                self.census.selected.insert((run.scope.kind, family));
            } else if run.carried & family.bit() != 0 && Scope::FORMS.contains(&run.scope.kind) {
                self.census.parted.insert((run.scope.kind, family));
            }
        }
    }

    fn content(&mut self, bytes: &[u8], run: Run<'_>) {
        let cos = self.cos;
        let mut run = run;
        operations(bytes, cos.names_table(), |operator, operands| {
            self.operation(operator, operands, &mut run);
        });
    }

    fn operation(&mut self, operator: &[u8], operands: &[Object], run: &mut Run<'_>) {
        let last = operands.last().and_then(Object::as_name);
        match operator {
            b"g" | b"G" => self.select(Family::Gray.bit(), *run),
            b"rg" | b"RG" => self.select(Family::Rgb.bit(), *run),
            b"k" | b"K" => self.select(Family::Cmyk.bit(), *run),
            b"cs" | b"CS" => {
                if let Some(name) = last {
                    let found = self.name_families(name, run.scope, 0);
                    self.select(found, *run);
                }
            }
            b"scn" | b"SCN" => {
                if let Some(name) = last {
                    self.pattern(name, *run);
                }
            }
            b"sh" => {
                if let Some((shading, _)) =
                    last.and_then(|name| self.resource(run.scope, self.keys.shading, name))
                {
                    self.shading(&shading, *run);
                }
            }
            b"Do" => {
                if let Some(name) = last {
                    self.xobject(name, *run);
                }
            }
            b"BI" => {
                if let Some(Object::Dict(image)) = operands.first() {
                    self.inline_image(image, *run);
                }
            }
            b"Tf" => {
                if let Some(name) = operands.iter().find_map(Object::as_name) {
                    self.type3(name, *run);
                }
            }
            b"gs" => {
                if let Some(name) = last {
                    self.soft_mask(name, *run);
                }
            }
            b"d1" if run.glyph => run.shape_only = true,
            _ => {}
        }
    }

    fn xobject(&mut self, name: Name, run: Run<'_>) {
        let Some((object, reference)) = self.resource(run.scope, self.keys.xobject, name) else {
            return;
        };
        let Some(body) = object.as_stream() else {
            return;
        };
        let subtype = body.dict.get_name(self.keys.subtype);
        if subtype == Some(self.keys.form) {
            if let Some(form) = reference {
                self.enter(form, Scope::Form, run);
            }
        } else if subtype == Some(self.keys.image) && !self.is_mask(&body.dict) {
            if let Some(space) = body.dict.get(self.keys.color_space) {
                let found = self.space_families(space, run.scope, 0);
                self.select(found, run);
            }
        }
    }

    /// Whether an image is a stencil mask, which paints the fill colour and
    /// names no space of its own (8.9.6.2).
    fn is_mask(&self, image: &Dict) -> bool {
        self.keys
            .masks
            .iter()
            .any(|&key| self.cos.resolve_key(image, key).as_bool() == Some(true))
    }

    fn inline_image(&mut self, image: &Dict, run: Run<'_>) {
        if self.is_mask(image) {
            return;
        }
        let space = image
            .get(self.keys.color_space)
            .or_else(|| image.get(self.keys.cs));
        if let Some(space) = space {
            let found = self.space_families(space, run.scope, 0);
            self.select(found, run);
        }
    }

    fn shading(&mut self, shading: &Object, run: Run<'_>) {
        let dict = match shading {
            Object::Stream(stream) => &stream.dict,
            Object::Dict(dict) => dict,
            _ => return,
        };
        if let Some(space) = dict.get(self.keys.color_space) {
            let found = self.space_families(space, run.scope, 0);
            self.select(found, run);
        }
    }

    fn pattern(&mut self, name: Name, run: Run<'_>) {
        let Some((pattern, reference)) = self.resource(run.scope, self.keys.pattern, name) else {
            return;
        };
        let dict = match &*pattern {
            Object::Stream(stream) => &stream.dict,
            Object::Dict(dict) => dict,
            _ => return,
        };
        match self.cos.resolve_key(dict, self.keys.pattern_type).as_int() {
            Some(1) => {
                if let Some(tiling) = reference {
                    self.enter(tiling, Scope::TilingPattern, run);
                }
            }
            Some(2) => {
                let shading = self.cos.resolve_key(dict, self.keys.shading);
                self.shading(&shading, run);
            }
            _ => {}
        }
    }

    /// A Type 3 font's glyphs, every one, in the font's `/Resources` or the
    /// invoker's.
    fn type3(&mut self, name: Name, run: Run<'_>) {
        let Some((font, reference)) = self.resource(run.scope, self.keys.font, name) else {
            return;
        };
        let Some(font) = font.as_dict() else {
            return;
        };
        if font.get_name(self.keys.subtype) != Some(self.keys.type3) {
            return;
        }
        if run.depth >= MAX_DEPTH {
            self.census.cut_short = true;
            return;
        }
        if let Some(reference) = reference {
            if !self
                .visited
                .insert(Self::visit(reference, Scope::Type3Font, run))
            {
                return;
            }
        }
        let own = self
            .cos
            .resolve_key(font, Name::RESOURCES)
            .as_dict()
            .cloned();
        let scope = match own {
            Some(resources) => self.scope(
                Scope::Type3Font,
                reference.unwrap_or(run.scope.owner),
                Some(resources),
            ),
            None => run.scope.clone(),
        };
        let inner = Run {
            scope: &scope,
            carried: run.carried | scope.defaults,
            shape_only: run.shape_only,
            glyph: true,
            depth: run.depth + 1,
        };
        let glyphs = self.cos.resolve_key(font, self.keys.char_procs);
        let Some(glyphs) = glyphs.as_dict() else {
            return;
        };
        for (_, glyph) in glyphs.iter() {
            let Some(glyph) = glyph.as_objref() else {
                continue;
            };
            if self.runs >= MAX_RUNS {
                self.census.cut_short = true;
                return;
            }
            if !self
                .visited
                .insert(Self::visit(glyph, Scope::Type3Font, inner))
            {
                continue;
            }
            let Ok(content) = self.cos.stream_decoded(glyph) else {
                continue;
            };
            self.runs += 1;
            self.content(&content, inner);
        }
    }

    /// A soft mask's group, from the `ExtGState` `gs` names (11.6.5.2).
    fn soft_mask(&mut self, name: Name, run: Run<'_>) {
        let Some((state, _)) = self.resource(run.scope, self.keys.ext_g_state, name) else {
            return;
        };
        let Some(state) = state.as_dict() else {
            return;
        };
        let mask = self.cos.resolve_key(state, self.keys.smask);
        // `/None` is a name, and no mask.
        let Some(group) = mask
            .as_dict()
            .and_then(|mask| mask.get_ref(self.keys.group))
        else {
            return;
        };
        self.enter(group, Scope::SoftMaskGroup, run);
    }
}

// ---- reading a content stream -----------------------------------------------------

/// The operands kept before an operator: `scn`, the operator here that takes
/// the most, takes a component per colorant and a name, and Annex C allows a
/// `DeviceN` space 32 colorants.
const MAX_OPERANDS: usize = 64;

/// Reads a content stream as operators and their operands (7.8.2), calling
/// `each` for every operator. An inline image (8.9.7) arrives as `BI` with its
/// dictionary as the one operand, and its data is stepped over, never read as
/// operators.
fn operations(content: &[u8], names: &NameTable, mut each: impl FnMut(&[u8], &[Object])) {
    let mut sink = WarningSink::new();
    let mut operands: Vec<Object> = Vec::new();
    let mut at = 0;
    while let Some(start) = next_token(content, at) {
        let byte = content[start];
        if matches!(
            byte,
            b'/' | b'(' | b'<' | b'[' | b'+' | b'-' | b'.' | b'0'..=b'9'
        ) {
            let parsed = parse_object_at(content, start as u64, names, &mut sink);
            at = past(parsed.end_offset, start, content.len());
            // No operator read here takes more; a stream of operands with no
            // operator after them is held to the last few.
            if operands.len() == MAX_OPERANDS {
                operands.remove(0);
            }
            operands.push(parsed.object);
        } else if is_regular(byte) {
            let end = word_end(content, start);
            at = end;
            match &content[start..end] {
                b"true" | b"false" | b"null" => operands.push(Object::Null),
                b"BI" => {
                    let (image, next) = inline_image(content, end, names, &mut sink);
                    at = next;
                    each(b"BI", &[Object::Dict(image)]);
                    operands.clear();
                }
                operator => {
                    each(operator, &operands);
                    operands.clear();
                }
            }
        } else {
            // `)`, `>`, `]`, `{` or `}`, where no object starts.
            at = start + 1;
        }
        // The census reads no warning, and a long stream would hold them all.
        let _ = sink.take();
    }
}

/// Where the next token after `at` starts, past white space and comments.
fn next_token(content: &[u8], at: usize) -> Option<usize> {
    let mut lexer = Lexer::at(content, at as u64);
    lexer.skip_space();
    let start = usize::try_from(lexer.offset()).ok()?;
    (start < content.len()).then_some(start)
}

fn word_end(content: &[u8], start: usize) -> usize {
    content[start..]
        .iter()
        .position(|&byte| !is_regular(byte))
        .map_or(content.len(), |length| start + length)
}

/// Where reading goes on after an object the parser ended at `end`: always
/// past `start`, never past the stream.
fn past(end: u64, start: usize, length: usize) -> usize {
    usize::try_from(end)
        .unwrap_or(length)
        .clamp(start + 1, length)
}

/// After `BI`: the image's dictionary up to `ID`, and where reading goes on,
/// past the `EI` that ends its data (8.9.7).
fn inline_image(
    content: &[u8],
    from: usize,
    names: &NameTable,
    sink: &mut WarningSink,
) -> (Dict, usize) {
    let mut image = Dict::new();
    let mut at = from;
    let data = loop {
        let Some(start) = next_token(content, at) else {
            return (image, content.len());
        };
        if content[start] == b'/' {
            let key = parse_object_at(content, start as u64, names, sink);
            at = past(key.end_offset, start, content.len());
            let Some(value_at) = next_token(content, at) else {
                return (image, content.len());
            };
            let value = parse_object_at(content, value_at as u64, names, sink);
            at = past(value.end_offset, value_at, content.len());
            if let Object::Name(key) = key.object {
                image.insert(key, value.object);
            }
        } else {
            let end = if is_regular(content[start]) {
                word_end(content, start)
            } else {
                start + 1
            };
            at = end;
            if &content[start..end] == b"ID" {
                // One white-space byte separates `ID` from the data.
                break end + 1;
            }
        }
    };
    // The data ends at an `EI` with white space before it, and white space, a
    // delimiter or the stream's end after it.
    let mut at = data.min(content.len());
    while at + 2 <= content.len() {
        if &content[at..at + 2] == b"EI"
            && is_whitespace(content[at - 1])
            && content
                .get(at + 2)
                .is_none_or(|&byte| is_whitespace(byte) || is_delimiter(byte))
        {
            return (image, at + 2);
        }
        at += 1;
    }
    (image, content.len())
}

// ---- the tally ----------------------------------------------------------------

/// A row of a table: one scope or any, one family or any.
type Row = (Option<Scope>, Option<Family>);

/// Files counted over a corpus. Each figure is a count of files.
#[derive(Debug, Default)]
struct Tally {
    files: usize,
    opened: usize,
    swept: BTreeMap<Option<Family>, usize>,
    carried: BTreeMap<Row, usize>,
    selected: BTreeMap<Row, usize>,
    parted: BTreeMap<Row, usize>,
    /// Files whose sweep finds a family's default in a dictionary no walked
    /// scope carries.
    unreached: BTreeMap<Option<Family>, usize>,
    cut_short: usize,
}

impl Tally {
    fn add(&mut self, census: Option<&FileCensus>) {
        self.files += 1;
        let Some(census) = census else {
            return;
        };
        self.opened += 1;
        count_families(&mut self.swept, census.swept.iter().copied());
        count_rows(&mut self.carried, &census.carried);
        count_rows(&mut self.selected, &census.selected);
        count_rows(&mut self.parted, &census.parted);
        let reached: BTreeSet<Family> = census.carried.iter().map(|&(_, f)| f).collect();
        count_families(
            &mut self.unreached,
            census.swept.difference(&reached).copied(),
        );
        if census.cut_short {
            self.cut_short += 1;
        }
    }

    fn print(&self) {
        println!("{} files read, {} opened", self.files, self.opened);
        println!("\nfiles where some dictionary names a default, reached or not:");
        print_families(&self.swept);
        println!("\nfiles carrying a default in a scope the walk reaches:");
        print_rows(&self.carried, &Scope::ALL, "any scope");
        println!("\nfiles whose content selects, under that default, the family it replaces:");
        print_rows(&self.selected, &Scope::ALL, "any scope");
        println!("\nfiles naming a default only where no walked scope carries it:");
        print_families(&self.unreached);
        println!(
            "\nfiles where the readings part: a form with /Resources of its own and no \
             default selects the family under a scope that has one:"
        );
        print_rows(&self.parted, &Scope::FORMS, "any form");
        println!(
            "\nwalks cut short by the depth or run budget: {}",
            self.cut_short
        );
    }
}

/// Counts one file once under each family it names and once under "any".
fn count_families(
    into: &mut BTreeMap<Option<Family>, usize>,
    families: impl Iterator<Item = Family>,
) {
    let mut keys = BTreeSet::new();
    for family in families {
        keys.insert(Some(family));
        keys.insert(None);
    }
    for key in keys {
        *into.entry(key).or_default() += 1;
    }
}

/// Counts one file once under each row its pairs reach, the "any" rows
/// included.
fn count_rows(into: &mut BTreeMap<Row, usize>, pairs: &BTreeSet<(Scope, Family)>) {
    let mut keys = BTreeSet::new();
    for &(scope, family) in pairs {
        keys.extend([
            (Some(scope), Some(family)),
            (Some(scope), None),
            (None, Some(family)),
            (None, None),
        ]);
    }
    for key in keys {
        *into.entry(key).or_default() += 1;
    }
}

fn count<K: Ord>(map: &BTreeMap<K, usize>, key: K) -> usize {
    map.get(&key).copied().unwrap_or(0)
}

const COLUMNS: [Option<Family>; 4] = [
    Some(Family::Gray),
    Some(Family::Rgb),
    Some(Family::Cmyk),
    None,
];

fn print_families(map: &BTreeMap<Option<Family>, usize>) {
    let cells: Vec<String> = COLUMNS
        .iter()
        .map(|&family| {
            format!(
                "{} {}",
                family.map_or("any", Family::label),
                count(map, family)
            )
        })
        .collect();
    println!("  {}", cells.join("  "));
}

fn print_rows(map: &BTreeMap<Row, usize>, scopes: &[Scope], all: &str) {
    println!(
        "  {:<24}{:>8}{:>8}{:>8}{:>8}",
        "", "Gray", "RGB", "CMYK", "any"
    );
    for scope in scopes.iter().copied().map(Some).chain([None]) {
        let cells: String = COLUMNS
            .iter()
            .map(|&family| format!("{:>8}", count(map, (scope, family))))
            .collect();
        println!("  {:<24}{cells}", scope.map_or(all, |s| s.label()));
    }
}

// ---- synthetic files --------------------------------------------------------------

/// A file from its numbered objects, with a cross-reference table that finds
/// each (7.5.4).
fn pdf(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let size = objects.iter().map(|(n, _)| *n).max().unwrap_or(0) + 1;
    tabled(objects, size, &[], &[])
}

/// As [`pdf`], with a table that lists the objects below `size` alone, marks
/// those in `free` free, and sends those in `misplaced` to offset zero, where
/// no object starts.
fn tabled(objects: &[(u32, Vec<u8>)], size: u32, free: &[u32], misplaced: &[u32]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = BTreeMap::new();
    for (number, body) in objects {
        offsets.insert(*number, out.len());
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for number in 1..size {
        let entry = match offsets.get(&number) {
            Some(_) if free.contains(&number) => "0000000000 65535 f \n".to_string(),
            Some(_) if misplaced.contains(&number) => "0000000000 00000 n \n".to_string(),
            Some(at) => format!("{at:010} 00000 n \n"),
            None => "0000000000 65535 f \n".to_string(),
        };
        out.extend_from_slice(entry.as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut body = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// One page, object 3, with its resources, its content (object 4), any
/// entries beside them, and the objects from 5 on.
fn page(resources: &str, content: &str, entries: &str, objects: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut all = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources {resources} \
                 /Contents 4 0 R {entries} >>"
            )
            .into_bytes(),
        ),
        (4, stream("", content.as_bytes())),
    ];
    all.extend(objects);
    pdf(&all)
}

/// A form XObject, with `/Resources` of its own or none.
fn form(resources: Option<&str>, content: &str) -> Vec<u8> {
    let resources = resources
        .map(|r| format!("/Resources {r}"))
        .unwrap_or_default();
    stream(
        &format!("/Type /XObject /Subtype /Form /BBox [0 0 10 10] {resources}"),
        content.as_bytes(),
    )
}

/// A square annotation whose normal appearance is object 6.
fn annotation() -> Vec<u8> {
    b"<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /AP << /N 6 0 R >> >>".to_vec()
}

/// An `ExtGState` whose luminosity soft mask's group is object 6.
fn soft_mask() -> Vec<u8> {
    b"<< /Type /ExtGState /SMask << /Type /Mask /S /Luminosity /G 6 0 R >> >>".to_vec()
}

/// A soft mask's group XObject.
fn group(resources: &str, content: &str) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
             /Group << /S /Transparency /CS /DeviceGray >> /Resources {resources}"
        ),
        content.as_bytes(),
    )
}

/// A Type 3 font whose one glyph, `a`, is object 6.
fn type3(resources: Option<&str>) -> Vec<u8> {
    let resources = resources
        .map(|r| format!("/Resources {r}"))
        .unwrap_or_default();
    format!(
        "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
         /CharProcs << /a 6 0 R >> /Encoding << /Type /Encoding /Differences [97 /a] >> \
         /FirstChar 97 /LastChar 97 /Widths [1] {resources} >>"
    )
    .into_bytes()
}

/// A tiling pattern, coloured (`1`) or uncoloured (`2`).
fn tiling(paint_type: u8, resources: Option<&str>, content: &str) -> Vec<u8> {
    let resources = resources
        .map(|r| format!("/Resources {r}"))
        .unwrap_or_default();
    stream(
        &format!(
            "/Type /Pattern /PatternType 1 /PaintType {paint_type} /TilingType 1 \
             /BBox [0 0 2 2] /XStep 2 /YStep 2 {resources}"
        ),
        content.as_bytes(),
    )
}

/// The three defaults, as a file might name them. The census reads none of
/// them as a space: a default is an entry.
const GRAY: &str = "[/CalGray << /WhitePoint [0.9505 1 1.089] >>]";
const RGB: &str = "[/CalRGB << /WhitePoint [0.9505 1 1.089] >>]";
/// An `ICCBased` space over object 9.
const CMYK: &str = "[/ICCBased 9 0 R]";

/// Object 9: a four-channel profile stream, never read.
fn profile() -> (u32, Vec<u8>) {
    (9, stream("/N 4", b""))
}

fn pairs(pairs: &[(Scope, Family)]) -> BTreeSet<(Scope, Family)> {
    pairs.iter().copied().collect()
}

/// One synthetic file and what the census must find in it.
struct Case {
    name: &'static str,
    bytes: Vec<u8>,
    swept: &'static [Family],
    carried: &'static [(Scope, Family)],
    selected: &'static [(Scope, Family)],
    parted: &'static [(Scope, Family)],
}

impl Case {
    fn expected(&self) -> FileCensus {
        FileCensus {
            swept: self.swept.iter().copied().collect(),
            carried: pairs(self.carried),
            selected: pairs(self.selected),
            parted: pairs(self.parted),
            cut_short: false,
        }
    }
}

/// A file per scope carrying a default its content selects under, the forms a
/// page's default does not reach, and their guards.
fn scope_cases() -> Vec<Case> {
    use Family::{Cmyk, Gray, Rgb};
    use Scope::{Appearance, Form, Page, SoftMaskGroup, TilingPattern, Type3Font};
    let page_rgb = format!("<< /ColorSpace << /DefaultRGB {RGB} >> /XObject << /Fm0 5 0 R >> >>");
    let page_gray = format!("<< /ColorSpace << /DefaultGray {GRAY} >> >>");
    let form_gray = format!("<< /ColorSpace << /DefaultGray {GRAY} >> >>");
    vec![
        Case {
            name: "a page's own default",
            bytes: page(
                &format!("<< /ColorSpace << /DefaultRGB {RGB} >> >>"),
                "1 0 0 rg 0 0 5 5 re f",
                "",
                vec![],
            ),
            swept: &[Rgb],
            carried: &[(Page, Rgb)],
            selected: &[(Page, Rgb)],
            parted: &[],
        },
        Case {
            name: "a page's default inherited from the page tree",
            bytes: pdf(&[
                (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
                (
                    2,
                    format!(
                        "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                         /Resources << /ColorSpace << /DefaultRGB {RGB} >> >> >>"
                    )
                    .into_bytes(),
                ),
                (
                    3,
                    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>"
                        .to_vec(),
                ),
                (4, stream("", b"1 0 0 rg 0 0 5 5 re f")),
            ]),
            swept: &[Rgb],
            carried: &[(Page, Rgb)],
            selected: &[(Page, Rgb)],
            parted: &[],
        },
        Case {
            name: "a page's default named through references",
            bytes: page(
                "<< /ColorSpace 5 0 R >>",
                "1 0 0 rg 0 0 5 5 re f",
                "",
                vec![
                    (5, b"<< /DefaultRGB 6 0 R >>".to_vec()),
                    (6, RGB.as_bytes().to_vec()),
                ],
            ),
            swept: &[Rgb],
            carried: &[(Page, Rgb)],
            selected: &[(Page, Rgb)],
            parted: &[],
        },
        Case {
            name: "a form's own default",
            bytes: page(
                "<< /XObject << /Fm0 5 0 R >> >>",
                "/Fm0 Do",
                "",
                vec![(5, form(Some(&form_gray), "0.5 g 0 0 5 5 re f"))],
            ),
            swept: &[Gray],
            carried: &[(Form, Gray)],
            selected: &[(Form, Gray)],
            parted: &[],
        },
        Case {
            name: "a tiling pattern's own default",
            bytes: page(
                "<< /Pattern << /P0 5 0 R >> >>",
                "/Pattern cs /P0 scn 0 0 10 10 re f",
                "",
                vec![
                    (
                        5,
                        tiling(
                            1,
                            Some(&format!("<< /ColorSpace << /DefaultCMYK {CMYK} >> >>")),
                            "0 0 0 1 k 0 0 1 1 re f",
                        ),
                    ),
                    profile(),
                ],
            ),
            swept: &[Cmyk],
            carried: &[(TilingPattern, Cmyk)],
            selected: &[(TilingPattern, Cmyk)],
            parted: &[],
        },
        Case {
            name: "a Type 3 font's own default",
            bytes: page(
                "<< /Font << /T0 5 0 R >> >>",
                "BT /T0 1 Tf (a) Tj ET",
                "",
                vec![
                    (
                        5,
                        type3(Some(&format!("<< /ColorSpace << /DefaultRGB {RGB} >> >>"))),
                    ),
                    (6, stream("", b"1 0 d0 0 0 1 rg 0 0 1 1 re f")),
                ],
            ),
            swept: &[Rgb],
            carried: &[(Type3Font, Rgb)],
            selected: &[(Type3Font, Rgb)],
            parted: &[],
        },
        Case {
            name: "an annotation appearance's own default",
            bytes: page(
                "<< >>",
                "",
                "/Annots [5 0 R]",
                vec![
                    (5, annotation()),
                    (6, form(Some(&form_gray), "0.2 g 0 0 10 10 re f")),
                ],
            ),
            swept: &[Gray],
            carried: &[(Appearance, Gray)],
            selected: &[(Appearance, Gray)],
            parted: &[],
        },
        Case {
            name: "a soft-mask group's own default",
            bytes: page(
                "<< /ExtGState << /GS0 5 0 R >> >>",
                "/GS0 gs 0 0 10 10 re f",
                "",
                vec![
                    (5, soft_mask()),
                    (6, group(&form_gray, "1 g 0 0 10 10 re f")),
                ],
            ),
            swept: &[Gray],
            carried: &[(SoftMaskGroup, Gray)],
            selected: &[(SoftMaskGroup, Gray)],
            parted: &[],
        },
        Case {
            name: "a form the page's default does not reach",
            bytes: page(
                &page_rgb,
                "/Fm0 Do",
                "",
                vec![(5, form(Some("<< >>"), "0 0 1 rg 0 0 5 5 re f"))],
            ),
            swept: &[Rgb],
            carried: &[(Page, Rgb)],
            selected: &[],
            parted: &[(Form, Rgb)],
        },
        Case {
            name: "a form two forms down the page's default does not reach",
            bytes: page(
                &page_rgb,
                "/Fm0 Do",
                "",
                vec![
                    (5, form(Some("<< /XObject << /Fm1 6 0 R >> >>"), "/Fm1 Do")),
                    (6, form(Some("<< >>"), "0 0 1 rg 0 0 5 5 re f")),
                ],
            ),
            swept: &[Rgb],
            carried: &[(Page, Rgb)],
            selected: &[],
            parted: &[(Form, Rgb)],
        },
        Case {
            name: "an appearance the page's default does not reach",
            bytes: page(
                &page_gray,
                "",
                "/Annots [5 0 R]",
                vec![
                    (5, annotation()),
                    (6, form(Some("<< >>"), "0.2 g 0 0 10 10 re f")),
                ],
            ),
            swept: &[Gray],
            carried: &[(Page, Gray)],
            selected: &[],
            parted: &[(Appearance, Gray)],
        },
        Case {
            name: "a soft-mask group the page's default does not reach",
            bytes: page(
                &format!("<< /ColorSpace << /DefaultGray {GRAY} >> /ExtGState << /GS0 5 0 R >> >>"),
                "/GS0 gs 0 0 10 10 re f",
                "",
                vec![(5, soft_mask()), (6, group("<< >>", "1 g 0 0 10 10 re f"))],
            ),
            swept: &[Gray],
            carried: &[(Page, Gray)],
            selected: &[],
            parted: &[(SoftMaskGroup, Gray)],
        },
        Case {
            name: "a form with no resources of its own runs in the page's",
            bytes: page(
                &page_rgb,
                "/Fm0 Do",
                "",
                vec![(5, form(None, "0 0 1 rg 0 0 5 5 re f"))],
            ),
            swept: &[Rgb],
            carried: &[(Page, Rgb)],
            selected: &[(Page, Rgb)],
            parted: &[],
        },
        Case {
            name: "a form with a default of its own",
            bytes: page(
                &page_rgb,
                "/Fm0 Do",
                "",
                vec![(
                    5,
                    form(
                        Some(&format!("<< /ColorSpace << /DefaultRGB {RGB} >> >>")),
                        "0 0 1 rg 0 0 5 5 re f",
                    ),
                )],
            ),
            swept: &[Rgb],
            carried: &[(Page, Rgb), (Form, Rgb)],
            selected: &[(Form, Rgb)],
            parted: &[],
        },
        Case {
            name: "a default for a family the page does not select",
            bytes: page(
                &format!("<< /ColorSpace << /DefaultCMYK {CMYK} >> >>"),
                "1 0 0 rg 0 0 5 5 re f",
                "",
                vec![profile()],
            ),
            swept: &[Cmyk],
            carried: &[(Page, Cmyk)],
            selected: &[],
            parted: &[],
        },
        Case {
            name: "a form carrying a default that nothing invokes",
            bytes: page(
                "<< /XObject << /Fm0 5 0 R >> >>",
                "0 0 5 5 re f",
                "",
                vec![(5, form(Some(&form_gray), "0.5 g 0 0 5 5 re f"))],
            ),
            swept: &[Gray],
            carried: &[],
            selected: &[],
            parted: &[],
        },
        Case {
            name: "a null default is no default",
            bytes: page(
                "<< /ColorSpace << /DefaultRGB null >> >>",
                "1 0 0 rg 0 0 5 5 re f",
                "",
                vec![],
            ),
            swept: &[],
            carried: &[],
            selected: &[],
            parted: &[],
        },
        Case {
            name: "no default anywhere",
            bytes: page(
                "<< /XObject << /Fm0 5 0 R >> >>",
                "1 0 0 rg /Fm0 Do",
                "",
                vec![(5, form(Some("<< >>"), "0 0 1 rg 0 0 5 5 re f"))],
            ),
            swept: &[],
            carried: &[],
            selected: &[],
            parted: &[],
        },
    ]
}

/// **The census finds a default in every scope, and the forms a page's
/// default does not reach.** Each synthetic file is read to exactly what it
/// carries, and the tally over them counts each scope's files exactly.
#[test]
fn the_census_finds_a_default_in_every_scope() {
    let cases = scope_cases();
    let mut tally = Tally::default();
    for case in &cases {
        let census = census_of(case.bytes.clone());
        assert_eq!(census, Some(case.expected()), "{}", case.name);
        tally.add(census.as_ref());
    }

    assert_eq!((tally.files, tally.opened), (18, 18));
    // Files carrying a default in each scope, then in any: the page's ten are
    // the three page cases, the four the readings part under, the form with
    // no resources, the form with its own, and the other family.
    let carrying = [10, 2, 1, 1, 1, 1, 15];
    let selecting = [4, 2, 1, 1, 1, 1, 10];
    for (at, scope) in Scope::ALL
        .iter()
        .copied()
        .map(Some)
        .chain([None])
        .enumerate()
    {
        assert_eq!(
            count(&tally.carried, (scope, None)),
            carrying[at],
            "files carrying a default in {scope:?}"
        );
        assert_eq!(
            count(&tally.selected, (scope, None)),
            selecting[at],
            "files selecting under a default in {scope:?}"
        );
    }
    for (family, carrying) in [(Family::Gray, 5), (Family::Rgb, 8), (Family::Cmyk, 2)] {
        assert_eq!(
            count(&tally.carried, (None, Some(family))),
            carrying,
            "{family:?}"
        );
    }
    // The forms a page's default does not reach: two forms, an appearance and
    // a soft-mask group, and nothing parts outside a form.
    for (scope, parting) in [
        (Some(Scope::Form), 2),
        (Some(Scope::Appearance), 1),
        (Some(Scope::SoftMaskGroup), 1),
        (Some(Scope::Page), 0),
        (Some(Scope::TilingPattern), 0),
        (Some(Scope::Type3Font), 0),
        (None, 4),
    ] {
        assert_eq!(
            count(&tally.parted, (scope, None)),
            parting,
            "parted in {scope:?}"
        );
    }
    // The sweep: every file naming a default, and the one no walk reaches.
    for (family, swept) in [
        (Some(Family::Gray), 6),
        (Some(Family::Rgb), 8),
        (Some(Family::Cmyk), 2),
        (None, 16),
    ] {
        assert_eq!(count(&tally.swept, family), swept, "swept {family:?}");
    }
    assert_eq!(count(&tally.unreached, Some(Family::Gray)), 1);
    assert_eq!(count(&tally.unreached, None), 1);
    assert_eq!(tally.cut_short, 0);
}

/// **The sweep reaches every object the walk reads**, a damaged file's
/// included. Each file's table names object 7 at an offset where no object
/// starts, so the file opens at the patch level and keeps its repair scan;
/// the page's `/ColorSpace` is an object the table marks free in one and does
/// not list in the other, and `CosDocument::get` reads it from the scan. A
/// sweep of the table's entries alone finds no default in either, and the
/// walk it gates never runs.
#[test]
fn the_sweep_reaches_every_object_the_walk_reads() {
    let objects = |spaces: u32| {
        vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] \
                     /Resources << /ColorSpace {spaces} 0 R >> /Contents 4 0 R >>"
                )
                .into_bytes(),
            ),
            (4, stream("", b"1 0 0 rg 0 0 5 5 re f")),
            (7, b"<< >>".to_vec()),
            (spaces, format!("<< /DefaultRGB {RGB} >>").into_bytes()),
        ]
    };
    let rows = [
        (
            "an object the table marks free",
            tabled(&objects(5), 8, &[5], &[7]),
        ),
        (
            "an object the table does not list",
            tabled(&objects(8), 8, &[], &[7]),
        ),
    ];
    let expected = FileCensus {
        swept: [Family::Rgb].into_iter().collect(),
        carried: pairs(&[(Scope::Page, Family::Rgb)]),
        selected: pairs(&[(Scope::Page, Family::Rgb)]),
        ..FileCensus::default()
    };
    for (name, bytes) in rows {
        let document = Document::open(bytes.clone()).expect("it opens");
        assert!(
            matches!(document.cos().ladder_level(), LadderLevel::Patch),
            "{name}: the file opens at the patch level, its scan kept"
        );
        assert_eq!(census_of(bytes), Some(expected.clone()), "{name}");
    }
}

/// **A stream reached two ways counts in both scopes**: a form the page
/// invokes with `Do` and its annotation names as its normal appearance.
#[test]
fn a_stream_reached_two_ways_counts_in_both_scopes() {
    let bytes = page(
        "<< /XObject << /Fm0 5 0 R >> >>",
        "/Fm0 Do",
        "/Annots [6 0 R]",
        vec![
            (
                5,
                form(
                    Some(&format!("<< /ColorSpace << /DefaultGray {GRAY} >> >>")),
                    "0.5 g 0 0 5 5 re f",
                ),
            ),
            (
                6,
                b"<< /Type /Annot /Subtype /Square /Rect [0 0 10 10] /AP << /N 5 0 R >> >>"
                    .to_vec(),
            ),
        ],
    );
    let both = pairs(&[
        (Scope::Form, Family::Gray),
        (Scope::Appearance, Family::Gray),
    ]);
    let census = census_of(bytes).expect("it opens");
    assert_eq!(census.carried, both);
    assert_eq!(census.selected, both);
}

/// **Every appearance stream an annotation carries is a scope**: each state of
/// a normal appearance given as a dictionary of them, as a checkbox's is, and
/// the down appearance beside it (12.5.5).
#[test]
fn every_appearance_an_annotation_carries_is_a_scope() {
    let own = |default: &str, space: &str| {
        form(
            Some(&format!("<< /ColorSpace << /{default} {space} >> >>")),
            "0 0 10 10 re f",
        )
    };
    let bytes = page(
        "<< >>",
        "",
        "/Annots [5 0 R]",
        vec![
            (
                5,
                b"<< /Type /Annot /Subtype /Widget /Rect [0 0 10 10] \
                  /AP << /N << /On 6 0 R /Off 7 0 R >> /D 8 0 R >> >>"
                    .to_vec(),
            ),
            (6, own("DefaultGray", GRAY)),
            (7, own("DefaultRGB", RGB)),
            (8, own("DefaultCMYK", CMYK)),
            profile(),
        ],
    );
    let census = census_of(bytes).expect("it opens");
    assert_eq!(
        census.carried,
        pairs(&[
            (Scope::Appearance, Family::Gray),
            (Scope::Appearance, Family::Rgb),
            (Scope::Appearance, Family::Cmyk),
        ])
    );
}

/// A page naming all three defaults, the spaces given, and the content.
fn under_all_three(
    spaces: &str,
    extra: &str,
    content: &str,
    objects: Vec<(u32, Vec<u8>)>,
) -> Vec<u8> {
    let resources = format!(
        "<< /ColorSpace << /DefaultGray {GRAY} /DefaultRGB {RGB} /DefaultCMYK {CMYK} {spaces} >> \
         {extra} >>"
    );
    let mut objects = objects;
    objects.push(profile());
    page(&resources, content, "", objects)
}

/// A one-sample image XObject, object 5.
fn image(entries: &str) -> (u32, Vec<u8>) {
    (
        5,
        stream(
            &format!(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 {entries}"
            ),
            b"abcd",
        ),
    )
}

/// A type 2 function from one input to `n` outputs, written inline.
fn function(n: usize) -> String {
    let zeros = vec!["0"; n].join(" ");
    let ones = vec!["1"; n].join(" ");
    format!("<< /FunctionType 2 /Domain [0 1] /C0 [{zeros}] /C1 [{ones}] /N 1 >>")
}

/// **Every selection 8.6.5.6 remaps is read as one, and nothing else is.**
/// Under a page naming all three defaults, each row's content selects the
/// families listed and no other.
#[test]
fn the_census_reads_every_selection_a_default_replaces() {
    use Family::{Cmyk, Gray, Rgb};
    let shading = |space: &str, n: usize| {
        format!(
            "<< /ShadingType 2 /ColorSpace {space} /Coords [0 0 1 0] /Function {} >>",
            function(n)
        )
    };
    let rows: Vec<(&str, Vec<u8>, &[Family])> = vec![
        ("g", under_all_three("", "", "0.5 g", vec![]), &[Gray]),
        ("G", under_all_three("", "", "0.5 G", vec![]), &[Gray]),
        ("rg", under_all_three("", "", "1 0 0 rg", vec![]), &[Rgb]),
        ("RG", under_all_three("", "", "1 0 0 RG", vec![]), &[Rgb]),
        ("k", under_all_three("", "", "0 0 0 1 k", vec![]), &[Cmyk]),
        ("K", under_all_three("", "", "0 0 0 1 K", vec![]), &[Cmyk]),
        (
            "cs naming DeviceGray",
            under_all_three("", "", "/DeviceGray cs", vec![]),
            &[Gray],
        ),
        (
            "CS naming DeviceRGB",
            under_all_three("", "", "/DeviceRGB CS", vec![]),
            &[Rgb],
        ),
        (
            "cs naming DeviceCMYK",
            under_all_three("", "", "/DeviceCMYK cs", vec![]),
            &[Cmyk],
        ),
        (
            "a resource that is a device space",
            under_all_three("/CS0 /DeviceRGB", "", "/CS0 cs", vec![]),
            &[Rgb],
        ),
        (
            "an Indexed base",
            under_all_three(
                "/CS0 [/Indexed /DeviceCMYK 0 <00000000>]",
                "",
                "/CS0 cs",
                vec![],
            ),
            &[Cmyk],
        ),
        (
            "an uncoloured pattern's base",
            under_all_three("/CS0 [/Pattern /DeviceGray]", "", "/CS0 cs", vec![]),
            &[Gray],
        ),
        (
            "a Separation alternate",
            under_all_three(
                &format!("/CS0 [/Separation /Spot /DeviceCMYK {}]", function(4)),
                "",
                "/CS0 cs",
                vec![],
            ),
            &[Cmyk],
        ),
        (
            "a DeviceN alternate",
            under_all_three(
                &format!("/CS0 [/DeviceN [/Spot] /DeviceRGB {}]", function(3)),
                "",
                "/CS0 CS",
                vec![],
            ),
            &[Rgb],
        ),
        (
            "an abbreviation no resource has",
            under_all_three("", "", "/RGB cs", vec![]),
            &[Rgb],
        ),
        (
            "a bare CalRGB",
            under_all_three("", "", "/CalRGB cs", vec![]),
            &[Rgb],
        ),
        (
            "a device name never names a resource",
            under_all_three("/DeviceRGB [/ICCBased 9 0 R]", "", "/DeviceRGB cs", vec![]),
            &[Rgb],
        ),
        (
            "a resource shadows an abbreviation",
            under_all_three("/RGB [/ICCBased 9 0 R]", "", "/RGB cs", vec![]),
            &[],
        ),
        (
            "an ICCBased space is no device space",
            under_all_three("/CS0 [/ICCBased 9 0 R]", "", "/CS0 cs", vec![]),
            &[],
        ),
        (
            "a CalRGB space is no device space",
            under_all_three(&format!("/CS0 {RGB}"), "", "/CS0 cs", vec![]),
            &[],
        ),
        (
            "an image XObject",
            under_all_three(
                "",
                "/XObject << /Im0 5 0 R >>",
                "/Im0 Do",
                vec![image("/ColorSpace /DeviceRGB")],
            ),
            &[Rgb],
        ),
        (
            "an image over an Indexed base",
            under_all_three(
                "",
                "/XObject << /Im0 5 0 R >>",
                "/Im0 Do",
                vec![image("/ColorSpace [/Indexed /DeviceGray 1 <00FF>]")],
            ),
            &[Gray],
        ),
        (
            "an image mask names no space it paints in",
            under_all_three(
                "",
                "/XObject << /Im0 5 0 R >>",
                "/Im0 Do",
                vec![image("/ImageMask true /ColorSpace /DeviceRGB")],
            ),
            &[],
        ),
        (
            "an inline image",
            under_all_three("", "", "BI /W 1 /H 1 /CS /G /BPC 8 ID a EI", vec![]),
            &[Gray],
        ),
        (
            "an inline image's abbreviated Indexed",
            under_all_three(
                "",
                "",
                "BI /W 1 /H 1 /CS [/I /RGB 0 <FF0000>] /BPC 8 ID a EI",
                vec![],
            ),
            &[Rgb],
        ),
        (
            "an inline image naming a resource",
            under_all_three(
                "/CS0 [/Indexed /DeviceCMYK 0 <00000000>]",
                "",
                "BI /W 1 /H 1 /CS /CS0 /BPC 8 ID a EI",
                vec![],
            ),
            &[Cmyk],
        ),
        (
            "an inline image mask",
            under_all_three("", "", "BI /W 1 /H 1 /IM true /CS /RGB ID a EI", vec![]),
            &[],
        ),
        (
            "inline image data is not content",
            under_all_three("", "", "BI /W 3 /H 1 /CS /G /BPC 8 ID 1 k EI", vec![]),
            &[Gray],
        ),
        (
            "a shading",
            under_all_three(
                "",
                "/Shading << /Sh0 5 0 R >>",
                "/Sh0 sh",
                vec![(5, shading("/DeviceRGB", 3).into_bytes())],
            ),
            &[Rgb],
        ),
        (
            "a shading pattern",
            under_all_three(
                "",
                "/Pattern << /P0 5 0 R >>",
                "/Pattern cs /P0 scn 0 0 10 10 re f",
                vec![(
                    5,
                    format!(
                        "<< /PatternType 2 /Shading {} >>",
                        shading("/DeviceCMYK", 4)
                    )
                    .into_bytes(),
                )],
            ),
            &[Cmyk],
        ),
        (
            "Pattern alone selects nothing",
            under_all_three("", "", "/Pattern cs", vec![]),
            &[],
        ),
        (
            "an uncoloured cell's own colour is ignored",
            under_all_three(
                "/CS0 [/Pattern /DeviceGray]",
                "/Pattern << /P0 5 0 R >>",
                "/CS0 cs 0.5 /P0 scn 0 0 10 10 re f",
                vec![(5, tiling(2, None, "0 0 1 rg 0 0 1 1 re f"))],
            ),
            &[Gray],
        ),
        (
            "a d0 glyph's colour is read",
            under_all_three(
                "",
                "/Font << /T0 5 0 R >>",
                "BT /T0 1 Tf (a) Tj ET",
                vec![
                    (5, type3(None)),
                    (6, stream("", b"1 0 d0 0 0 1 rg 0 0 1 1 re f")),
                ],
            ),
            &[Rgb],
        ),
        (
            "a d1 glyph's colour is ignored",
            under_all_three(
                "",
                "/Font << /T0 5 0 R >>",
                "BT /T0 1 Tf (a) Tj ET",
                vec![
                    (5, type3(None)),
                    (6, stream("", b"1 0 0 0 1 1 d1 0 0 1 rg 0 0 1 1 re f")),
                ],
            ),
            &[],
        ),
        (
            "an operator inside a string is no selection",
            under_all_three("", "", "BT /F0 1 Tf (1 0 0 rg) Tj ET", vec![]),
            &[],
        ),
        (
            "an operator inside a comment is no selection",
            under_all_three("", "", "% 1 0 0 rg\n0 0 5 5 re f", vec![]),
            &[],
        ),
        (
            "sc with no space named selects nothing",
            under_all_three("", "", "0.5 sc 0 0 5 5 re f", vec![]),
            &[],
        ),
    ];
    for (name, bytes, families) in rows {
        let census = census_of(bytes).expect("it opens");
        let expected: BTreeSet<(Scope, Family)> = families
            .iter()
            .map(|&family| (Scope::Page, family))
            .collect();
        assert_eq!(census.selected, expected, "{name}");
        assert!(
            census.parted.is_empty(),
            "{name}: nothing parts on one page"
        );
    }
}

// ---- the corpora ----------------------------------------------------------------

/// Whether a missing corpus is a failure rather than a skip.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}

fn corpus_root() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os("TINKER_CORPUS") {
        let path = PathBuf::from(named);
        return path.is_dir().then_some(path);
    }
    let guess = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()?;
    guess.is_dir().then_some(guess)
}

fn pdfs_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// How many of the files where the readings part are named in the output.
const NAMED: usize = 20;

/// **The corpora's default colour spaces, by scope.** Ignored by default: it
/// reads every fetched file, and runs in CI's corpus job.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn census_of_the_corpus_default_spaces() {
    let Some(root) = corpus_root() else {
        println!("{SKIPPED} (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at TINKER_CORPUS: \
             this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    if files.is_empty() {
        println!("{SKIPPED} ({} holds no PDFs)", root.display());
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and the corpus is empty"
        );
        return;
    }
    println!("{RAN} over {} files", files.len());

    let mut tally = Tally::default();
    let mut invented = Vec::new();
    let mut parting = Vec::new();
    for path in &files {
        let census = std::fs::read(path).ok().and_then(census_of);
        if let Some(census) = &census {
            let reached: BTreeSet<Family> = census.carried.iter().map(|&(_, f)| f).collect();
            if !reached.is_subset(&census.swept) {
                invented.push(relative(&root, path));
            }
            if !census.parted.is_empty() {
                parting.push(relative(&root, path));
            }
        }
        tally.add(census.as_ref());
    }
    tally.print();
    println!(
        "\nthe files where the readings part ({} in all, the first {NAMED} named):",
        parting.len()
    );
    for name in parting.iter().take(NAMED) {
        println!("  {name}");
    }
    assert!(
        invented.is_empty(),
        "the walk found a default the sweep of every object did not, in {} files: {:?}",
        invented.len(),
        &invented[..invented.len().min(NAMED)]
    );
}
