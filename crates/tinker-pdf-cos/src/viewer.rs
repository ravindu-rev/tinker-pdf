//! Viewer preferences (12.2): how a document asks to be shown and printed.
//!
//! Feature documentation: `docs/features/document-model.md`.
//!
//! Every entry of ISO 32000-2 Table 147, typed. Each is an `Option`, because
//! the table gives most of them a default and a document that states the
//! default and a document that states nothing are different files — the same
//! absent-not-empty contract `/Info` keeps ([`crate::outline::Metadata`]).
//!
//! A value of the wrong type, or a name outside the ones the table defines, is
//! read as absent: the table says a processor shall then use the default,
//! which is exactly what an absent entry means, and a caller that needs to see
//! the malformed value has [`crate::DocumentEditor::catalog`] and the raw
//! dictionary. Reading here records no warning, for the reason
//! `Document::fonts` gives: a read that changed what the document reports
//! about itself would make the answer depend on the order it was asked in.

use crate::name::Name;
use crate::object::{Dict, Object};
use crate::pages::PageBoundary;
use crate::resolve::Resolve;

/// 12.2 Table 147's `/NonFullScreenPageMode`: the page mode to use on leaving
/// full-screen mode, when the catalog's `/PageMode` is `/FullScreen`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NonFullScreenPageMode {
    /// `/UseNone`: neither outline nor thumbnails. The default.
    UseNone,
    /// `/UseOutlines`: the outline panel.
    UseOutlines,
    /// `/UseThumbs`: the thumbnail panel.
    UseThumbs,
    /// `/UseOC`: the optional content panel.
    UseOC,
}

/// 12.2 Table 147's `/Direction`: the predominant reading order, which
/// decides how pages are laid side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadingDirection {
    /// `/L2R`: left to right. The default.
    LeftToRight,
    /// `/R2L`: right to left, including vertical writing systems.
    RightToLeft,
}

/// 12.2 Table 147's `/PrintScaling`: the scaling a print dialog should offer
/// first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrintScaling {
    /// `/None`: no scaling.
    None,
    /// `/AppDefault`: whatever the application would do. The default.
    AppDefault,
}

/// 12.2 Table 147's `/Duplex`: the paper handling a print dialog should offer
/// first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Duplex {
    /// `/Simplex`: one side.
    Simplex,
    /// `/DuplexFlipShortEdge`: both sides, flipped on the short edge.
    DuplexFlipShortEdge,
    /// `/DuplexFlipLongEdge`: both sides, flipped on the long edge.
    DuplexFlipLongEdge,
}

/// A viewer preference PDF 2.0's `/Enforce` may name (12.2 Table 147).
///
/// The table defines one, and a name it does not define is dropped on read:
/// there is nothing a caller could do with "enforce something this build
/// cannot name".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnforcedPreference {
    /// `/PrintScaling`: the print dialog shall not offer to change it.
    PrintScaling,
}

/// A document's viewer preferences (12.2, Table 147), as the catalog's
/// `/ViewerPreferences` states them.
///
/// `None` is "the document says nothing", which a viewer answers with the
/// table's default; `Some(default)` is the document saying the default out
/// loud. [`crate::DocumentEditor::set_viewer_preferences`] writes exactly the
/// `Some` fields, so a write followed by a read is an equality.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewerPreferences {
    /// `/HideToolbar`.
    pub hide_toolbar: Option<bool>,
    /// `/HideMenubar`.
    pub hide_menubar: Option<bool>,
    /// `/HideWindowUI`: scroll bars, navigation controls and the like.
    pub hide_window_ui: Option<bool>,
    /// `/FitWindow`: resize the window to the first page.
    pub fit_window: Option<bool>,
    /// `/CenterWindow`.
    pub center_window: Option<bool>,
    /// `/DisplayDocTitle`: title the window with `/Info /Title` (or the XMP
    /// `dc:title`) rather than the file name.
    pub display_doc_title: Option<bool>,
    /// `/NonFullScreenPageMode`.
    pub non_full_screen_page_mode: Option<NonFullScreenPageMode>,
    /// `/Direction`.
    pub direction: Option<ReadingDirection>,
    /// `/ViewArea`: the boundary a screen shows (deprecated in PDF 2.0).
    pub view_area: Option<PageBoundary>,
    /// `/ViewClip`: the boundary a screen clips to (deprecated in PDF 2.0).
    pub view_clip: Option<PageBoundary>,
    /// `/PrintArea`: the boundary a printer shows (deprecated in PDF 2.0).
    pub print_area: Option<PageBoundary>,
    /// `/PrintClip`: the boundary a printer clips to (deprecated in PDF 2.0).
    pub print_clip: Option<PageBoundary>,
    /// `/PrintScaling`.
    pub print_scaling: Option<PrintScaling>,
    /// `/Duplex`.
    pub duplex: Option<Duplex>,
    /// `/PickTrayByPDFSize`: choose the paper tray by page size.
    pub pick_tray_by_pdf_size: Option<bool>,
    /// `/PrintPageRange`: the page ranges a print dialog starts with, as
    /// first-and-last pairs **numbered from 1**, as the table numbers them.
    pub print_page_range: Option<Vec<(u32, u32)>>,
    /// `/NumCopies`: the copy count a print dialog starts with.
    pub num_copies: Option<u32>,
    /// `/Enforce` (PDF 2.0): the preferences a viewer shall not let a user
    /// override. Empty when the document enforces nothing.
    pub enforce: Vec<EnforcedPreference>,
}

/// One Table 147 key, as `ViewerPreferences` holds it.
///
/// Listed once, so the reader and the writer walk the same keys and a key the
/// one knows cannot be missing from the other.
pub(crate) const KEYS: [&[u8]; 18] = [
    b"HideToolbar",
    b"HideMenubar",
    b"HideWindowUI",
    b"FitWindow",
    b"CenterWindow",
    b"DisplayDocTitle",
    b"NonFullScreenPageMode",
    b"Direction",
    b"ViewArea",
    b"ViewClip",
    b"PrintArea",
    b"PrintClip",
    b"PrintScaling",
    b"Duplex",
    b"PickTrayByPDFSize",
    b"PrintPageRange",
    b"NumCopies",
    b"Enforce",
];

impl NonFullScreenPageMode {
    fn name(self) -> &'static [u8] {
        match self {
            NonFullScreenPageMode::UseNone => b"UseNone",
            NonFullScreenPageMode::UseOutlines => b"UseOutlines",
            NonFullScreenPageMode::UseThumbs => b"UseThumbs",
            NonFullScreenPageMode::UseOC => b"UseOC",
        }
    }

    fn from_name(bytes: &[u8]) -> Option<NonFullScreenPageMode> {
        [
            NonFullScreenPageMode::UseNone,
            NonFullScreenPageMode::UseOutlines,
            NonFullScreenPageMode::UseThumbs,
            NonFullScreenPageMode::UseOC,
        ]
        .into_iter()
        .find(|mode| mode.name() == bytes)
    }
}

impl ReadingDirection {
    fn name(self) -> &'static [u8] {
        match self {
            ReadingDirection::LeftToRight => b"L2R",
            ReadingDirection::RightToLeft => b"R2L",
        }
    }

    fn from_name(bytes: &[u8]) -> Option<ReadingDirection> {
        [ReadingDirection::LeftToRight, ReadingDirection::RightToLeft]
            .into_iter()
            .find(|direction| direction.name() == bytes)
    }
}

impl PrintScaling {
    fn name(self) -> &'static [u8] {
        match self {
            PrintScaling::None => b"None",
            PrintScaling::AppDefault => b"AppDefault",
        }
    }

    fn from_name(bytes: &[u8]) -> Option<PrintScaling> {
        [PrintScaling::None, PrintScaling::AppDefault]
            .into_iter()
            .find(|scaling| scaling.name() == bytes)
    }
}

impl Duplex {
    fn name(self) -> &'static [u8] {
        match self {
            Duplex::Simplex => b"Simplex",
            Duplex::DuplexFlipShortEdge => b"DuplexFlipShortEdge",
            Duplex::DuplexFlipLongEdge => b"DuplexFlipLongEdge",
        }
    }

    fn from_name(bytes: &[u8]) -> Option<Duplex> {
        [
            Duplex::Simplex,
            Duplex::DuplexFlipShortEdge,
            Duplex::DuplexFlipLongEdge,
        ]
        .into_iter()
        .find(|duplex| duplex.name() == bytes)
    }
}

impl EnforcedPreference {
    fn name(self) -> &'static [u8] {
        match self {
            EnforcedPreference::PrintScaling => b"PrintScaling",
        }
    }

    fn from_name(bytes: &[u8]) -> Option<EnforcedPreference> {
        (bytes == b"PrintScaling").then_some(EnforcedPreference::PrintScaling)
    }
}

impl ViewerPreferences {
    /// Whether the document states no preference at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == ViewerPreferences::default()
    }

    /// Whether every stated value can be written: a page range numbered from
    /// 1, each pair in order, and a copy count of at least one.
    ///
    /// Table 147 numbers the first page 1, so a 0 is no page; a pair whose
    /// first page follows its last is no range. Neither is corrected here —
    /// which of the two numbers the caller meant is theirs to say.
    #[must_use]
    pub fn is_writable(&self) -> bool {
        let ranges = self
            .print_page_range
            .as_deref()
            .unwrap_or_default()
            .iter()
            .all(|(first, last)| *first >= 1 && first <= last);
        ranges && self.num_copies.is_none_or(|n| n >= 1)
    }

    /// The dictionary entries this value states, as `(key, value)` pairs in
    /// Table 147's order — one per `Some` field, and `/Enforce` when it is not
    /// empty.
    pub(crate) fn entries<R: Resolve + ?Sized>(&self, doc: &R) -> Vec<(Name, Object)> {
        let name = |bytes: &[u8]| Object::Name(doc.intern(bytes));
        let boundary = |b: Option<PageBoundary>| b.map(|b| name(b.key()));
        let values: [Option<Object>; 18] = [
            self.hide_toolbar.map(Object::Bool),
            self.hide_menubar.map(Object::Bool),
            self.hide_window_ui.map(Object::Bool),
            self.fit_window.map(Object::Bool),
            self.center_window.map(Object::Bool),
            self.display_doc_title.map(Object::Bool),
            self.non_full_screen_page_mode.map(|m| name(m.name())),
            self.direction.map(|d| name(d.name())),
            boundary(self.view_area),
            boundary(self.view_clip),
            boundary(self.print_area),
            boundary(self.print_clip),
            self.print_scaling.map(|s| name(s.name())),
            self.duplex.map(|d| name(d.name())),
            self.pick_tray_by_pdf_size.map(Object::Bool),
            self.print_page_range.as_ref().map(|ranges| {
                Object::Array(
                    ranges
                        .iter()
                        .flat_map(|(first, last)| {
                            [
                                Object::Int(i64::from(*first)),
                                Object::Int(i64::from(*last)),
                            ]
                        })
                        .collect(),
                )
            }),
            self.num_copies.map(|n| Object::Int(i64::from(n))),
            (!self.enforce.is_empty())
                .then(|| Object::Array(self.enforce.iter().map(|e| name(e.name())).collect())),
        ];
        KEYS.iter()
            .zip(values)
            .filter_map(|(key, value)| Some((doc.intern(key), value?)))
            .collect()
    }
}

/// The catalog's `/ViewerPreferences`, typed (12.2).
///
/// Every field is `None` — and `enforce` empty — for a document that has none,
/// which is most documents.
#[must_use]
pub fn viewer_preferences<R: Resolve + ?Sized>(doc: &R) -> ViewerPreferences {
    let Some(catalog) = doc.catalog() else {
        return ViewerPreferences::default();
    };
    let resolved = doc.resolve_key(&catalog, doc.intern(b"ViewerPreferences"));
    match resolved.as_dict() {
        Some(dict) => read(doc, dict),
        None => ViewerPreferences::default(),
    }
}

fn read<R: Resolve + ?Sized>(doc: &R, dict: &Dict) -> ViewerPreferences {
    let value = |key: &[u8]| doc.resolve_key(dict, doc.intern(key));
    let flag = |key: &[u8]| value(key).as_bool();
    let bytes_of = |object: &Object| {
        object
            .as_name()
            .and_then(|n| doc.name_bytes(n))
            .map(|b| b.to_vec())
    };
    let named = |key: &[u8]| bytes_of(&value(key));
    let boundary = |key: &[u8]| named(key).and_then(|b| PageBoundary::from_key(&b));

    // Table 147: "an even number of integers to be interpreted in pairs". A
    // trailing odd one has no partner and is not a range; a pair holding
    // anything but two page numbers is not one either.
    let print_page_range = value(b"PrintPageRange").as_array().map(|items| {
        items
            .chunks_exact(2)
            .filter_map(|pair| {
                let number = |o: &Object| o.as_int().and_then(|n| u32::try_from(n).ok());
                Some((number(pair.first()?)?, number(pair.get(1)?)?))
            })
            .collect()
    });

    let enforce = value(b"Enforce")
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| bytes_of(item).and_then(|b| EnforcedPreference::from_name(&b)))
                .collect()
        })
        .unwrap_or_default();

    ViewerPreferences {
        hide_toolbar: flag(b"HideToolbar"),
        hide_menubar: flag(b"HideMenubar"),
        hide_window_ui: flag(b"HideWindowUI"),
        fit_window: flag(b"FitWindow"),
        center_window: flag(b"CenterWindow"),
        display_doc_title: flag(b"DisplayDocTitle"),
        non_full_screen_page_mode: named(b"NonFullScreenPageMode")
            .and_then(|b| NonFullScreenPageMode::from_name(&b)),
        direction: named(b"Direction").and_then(|b| ReadingDirection::from_name(&b)),
        view_area: boundary(b"ViewArea"),
        view_clip: boundary(b"ViewClip"),
        print_area: boundary(b"PrintArea"),
        print_clip: boundary(b"PrintClip"),
        print_scaling: named(b"PrintScaling").and_then(|b| PrintScaling::from_name(&b)),
        duplex: named(b"Duplex").and_then(|b| Duplex::from_name(&b)),
        pick_tray_by_pdf_size: flag(b"PickTrayByPDFSize"),
        print_page_range,
        num_copies: value(b"NumCopies")
            .as_int()
            .and_then(|n| u32::try_from(n).ok()),
        enforce,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::CosDocument;

    fn with_preferences(entries: &str) -> CosDocument {
        let bytes = format!(
            "%PDF-2.0\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R /ViewerPreferences 4 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>\nendobj\n\
4 0 obj\n<< {entries} >>\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n"
        );
        CosDocument::open(bytes.into_bytes()).expect("it opens")
    }

    #[test]
    fn every_table_147_entry_is_read() {
        let doc = with_preferences(
            "/HideToolbar true /HideMenubar false /HideWindowUI true /FitWindow true \
             /CenterWindow false /DisplayDocTitle true /NonFullScreenPageMode /UseOC \
             /Direction /R2L /ViewArea /TrimBox /ViewClip /BleedBox /PrintArea /ArtBox \
             /PrintClip /MediaBox /PrintScaling /None /Duplex /DuplexFlipLongEdge \
             /PickTrayByPDFSize true /PrintPageRange [1 3 5 5] /NumCopies 2 \
             /Enforce [/PrintScaling]",
        );
        assert_eq!(
            viewer_preferences(&doc),
            ViewerPreferences {
                hide_toolbar: Some(true),
                hide_menubar: Some(false),
                hide_window_ui: Some(true),
                fit_window: Some(true),
                center_window: Some(false),
                display_doc_title: Some(true),
                non_full_screen_page_mode: Some(NonFullScreenPageMode::UseOC),
                direction: Some(ReadingDirection::RightToLeft),
                view_area: Some(PageBoundary::TrimBox),
                view_clip: Some(PageBoundary::BleedBox),
                print_area: Some(PageBoundary::ArtBox),
                print_clip: Some(PageBoundary::MediaBox),
                print_scaling: Some(PrintScaling::None),
                duplex: Some(Duplex::DuplexFlipLongEdge),
                pick_tray_by_pdf_size: Some(true),
                print_page_range: Some(vec![(1, 3), (5, 5)]),
                num_copies: Some(2),
                enforce: vec![EnforcedPreference::PrintScaling],
            }
        );
    }

    /// Absent is not the default: a document stating `/Direction /L2R` said
    /// something a document stating nothing did not.
    #[test]
    fn a_stated_default_is_not_an_absent_entry() {
        let stated = viewer_preferences(&with_preferences("/Direction /L2R"));
        assert_eq!(stated.direction, Some(ReadingDirection::LeftToRight));
        let silent = viewer_preferences(&with_preferences(""));
        assert_eq!(silent.direction, None);
        assert!(silent.is_empty());
    }

    /// The wrong type, a name the table does not define, a half pair and a
    /// negative count are each read as the entry being absent.
    #[test]
    fn malformed_entries_read_as_absent() {
        let prefs = viewer_preferences(&with_preferences(
            "/HideToolbar /true /Direction /Upward /ViewArea /Trimbox \
             /PrintPageRange [1 2 7] /NumCopies -3 /Enforce [/Duplex /PrintScaling]",
        ));
        assert_eq!(prefs.hide_toolbar, None);
        assert_eq!(prefs.direction, None);
        assert_eq!(prefs.view_area, None);
        assert_eq!(prefs.print_page_range, Some(vec![(1, 2)]));
        assert_eq!(prefs.num_copies, None);
        assert_eq!(prefs.enforce, vec![EnforcedPreference::PrintScaling]);
    }

    #[test]
    fn a_page_range_from_zero_or_backwards_is_not_writable() {
        let mut prefs = ViewerPreferences {
            print_page_range: Some(vec![(1, 2)]),
            ..ViewerPreferences::default()
        };
        assert!(prefs.is_writable());
        prefs.print_page_range = Some(vec![(0, 2)]);
        assert!(!prefs.is_writable());
        prefs.print_page_range = Some(vec![(3, 2)]);
        assert!(!prefs.is_writable());
        prefs.print_page_range = None;
        prefs.num_copies = Some(0);
        assert!(!prefs.is_writable());
    }
}
