//! PDF/UA conformance (ISO 14289), read — `docs/design/pdfua.md`.
//!
//! The question is the one [`crate::pdfa`] answers for ISO 19005, asked of a
//! different standard: **which** part of ISO 14289 does this file claim, and
//! which of that part's clauses does it break? The answer has the same shape —
//! a list of findings, each naming its clause and the object it is about, and
//! a [`Coverage`] saying which rule groups ran — and one thing more, because
//! most of ISO 14289 is not a rule a reader can run.
//!
//! # Abstention is a value, not a sentence in a doc
//!
//! Much of the standard is a judgement about meaning: whether a `/P` is a
//! paragraph, whether the reading order is the author's, whether an `/Alt`
//! describes the picture. No reader decides those, and a verdict that reported
//! an empty finding list over them would be claiming a conformance nobody
//! checked. So [`Verdict::abstained`] names, in the same struct as the
//! findings, every clause this build knows it did not decide — in two classes,
//! because they are scheduled differently:
//!
//! - [`AbstentionClass::Staged`]: a reader *could* decide it and this build
//!   does not yet, because the machinery it needs is missing. Each entry in
//!   [`STAGED`] says which machinery. A staged clause is a milestone.
//! - [`AbstentionClass::Undecidable`]: no reader alone decides it. Each entry
//!   in [`UNDECIDABLE`] says what judgement it is. An undecidable clause is a
//!   limit, and is never counted as agreement.
//!
//! # The kernel is [`crate::pdfa`]'s
//!
//! The design asks that the PDF/A kernel be shared rather than copied, and it
//! is: [`crate::pdfa::Machinery`] counts every reach this validator makes, a
//! finding is a [`crate::pdfa::ConformanceFinding`] whose kind is the one
//! closed [`crate::pdfa::FindingKind`], and where ISO 14289 asks what ISO
//! 19005 already asks the PDF/A rule runs and only the clause number differs.
//! The kernel stays in `pdfa.rs` with its items opened to the crate rather
//! than moved to a third module; the design leaves the shape of the move to
//! whichever standard lands first, and opening the items is the smallest move
//! that shares them.
//!
//! # Where the clause numbers come from
//!
//! ISO 14289-1 and -2 are sold, and this environment has neither. The clause
//! numbers below are the ones three published sources agree on, which is the
//! design's stated rule: the veraPDF corpus's clause directories (counted in
//! the design), its fixtures' own outlines, and veraPDF's published PDF/UA
//! validation profiles (`PDFUA-1.xml`, "Validation rules against ISO
//! 14289-1:2014", and `PDFUA-2.xml`, read as data at
//! `veraPDF-validation-profiles` `070d39f`; nothing of theirs runs, ruling
//! 13). A rule whose part has no number in those sources has no row for that
//! part and does not run under it.

use tinker_pdf_xml::{Event, Name, Source};

use crate::pdfa::{Clause, ConformanceFinding, FindingKind, Machinery, RuleGroup};
use crate::Document;

mod fonts;
mod structure;
mod syntax;

/// Which part of ISO 14289 a file claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UaPart {
    /// ISO 14289-1:2014, on ISO 32000-1.
    One,
    /// ISO 14289-2:2024, on ISO 32000-2.
    Two,
}

impl UaPart {
    /// The `pdfuaid:part` value.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            UaPart::One => 1,
            UaPart::Two => 2,
        }
    }

    fn from_number(value: i64) -> Option<UaPart> {
        match value {
            1 => Some(UaPart::One),
            2 => Some(UaPart::Two),
            _ => None,
        }
    }
}

impl core::fmt::Display for UaPart {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PDF/UA-{}", self.number())
    }
}

/// One rule's clause number in each part of ISO 14289 that has it.
///
/// `None` is not a gap in the table: it is a part whose published sources
/// state no such rule, and a rule with no number under the part being checked
/// does not run there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UaClauses {
    one: Option<&'static str>,
    two: Option<&'static str>,
}

impl UaClauses {
    /// The clause `part` numbers this rule, if it has the rule at all.
    pub(crate) fn of(self, part: UaPart) -> Option<&'static str> {
        match part {
            UaPart::One => self.one,
            UaPart::Two => self.two,
        }
    }
}

/// Every rule's clause number, per part.
pub(crate) mod clauses {
    use super::UaClauses;

    /// Version identification: the `pdfuaid` claim (5 in both parts).
    pub(crate) const IDENTIFICATION: UaClauses = UaClauses {
        one: Some("5"),
        two: Some("5"),
    };

    /// `/MarkInfo /Marked true`. Both published profiles number it 6.2.
    pub(crate) const MARKED: UaClauses = UaClauses {
        one: Some("6.2"),
        two: Some("6.2"),
    };

    /// The structure hierarchy: a `/StructTreeRoot`, and one that can be
    /// walked (UA-1 7.1, UA-2 8.2.1).
    pub(crate) const STRUCTURE_HIERARCHY: UaClauses = UaClauses {
        one: Some("7.1"),
        two: Some("8.2.1"),
    };

    /// `/Suspects` not true. UA-1 7.1, citing ISO 32000-1 Table 321; the
    /// UA-2 profile states no such rule, and PDF 2.0 deprecates the key.
    pub(crate) const SUSPECTS: UaClauses = UaClauses {
        one: Some("7.1"),
        two: None,
    };

    /// A `Figure`'s alternative description (UA-1 7.3, UA-2 8.2.5.28.2).
    pub(crate) const FIGURE_ALTERNATIVE: UaClauses = UaClauses {
        one: Some("7.3"),
        two: Some("8.2.5.28.2"),
    };

    /// Heading levels in order (UA-1 7.4.2). The UA-2 profile carries no
    /// such rule — it forbids the unnumbered `H` instead (8.2.5.12) — so
    /// nothing runs there.
    pub(crate) const HEADING_LEVELS: UaClauses = UaClauses {
        one: Some("7.4.2"),
        two: None,
    };

    /// Natural language stated somewhere (UA-1 7.2). Part 2 asks for more —
    /// the catalog's own `/Lang`, [`CATALOG_LANGUAGE`] — which every file this
    /// rule would report also breaks, so it has no part 2 row: one defect,
    /// one finding.
    pub(crate) const NATURAL_LANGUAGE: UaClauses = UaClauses {
        one: Some("7.2"),
        two: None,
    };

    /// The catalog's `/Lang`, present and not empty: UA-2 8.4.4-1. Part 1
    /// states no such rule — its 7.2 asks for a language "determinable" for
    /// each text, which an element's `/Lang` can supply.
    pub(crate) const CATALOG_LANGUAGE: UaClauses = UaClauses {
        one: None,
        two: Some("8.4.4"),
    };

    /// Every `/Lang` a language identifier (UA-1 7.2-29, UA-2 8.4.4-2).
    pub(crate) const LANGUAGE_IDENTIFIER: UaClauses = UaClauses {
        one: Some("7.2"),
        two: Some("8.4.4"),
    };

    /// Structure types against ISO 32000-1 14.8.4: a non-standard type
    /// mapped to a standard one (UA-1 7.1-5), and a standard one not
    /// remapped (7.1-7). Part 2's rules are stated over PDF 2.0's namespaces
    /// (8.2.4), which this build does not judge yet; the row is part 1's.
    pub(crate) const STRUCTURE_TYPES: UaClauses = UaClauses {
        one: Some("7.1"),
        two: None,
    };

    /// A structure element's `/P` (UA-1 7.1-12, UA-2 8.2.1-2).
    pub(crate) const STRUCTURE_PARENT: UaClauses = UaClauses {
        one: Some("7.1"),
        two: Some("8.2.1"),
    };

    /// The catalog's metadata stream and its `dc:title` (UA-1 7.1-8 and -9,
    /// UA-2 8.11.1-1 and -2).
    pub(crate) const METADATA_STREAM: UaClauses = UaClauses {
        one: Some("7.1"),
        two: Some("8.11.1"),
    };

    /// `/ViewerPreferences /DisplayDocTitle true` (UA-1 7.1-10, UA-2
    /// 8.11.2-1).
    pub(crate) const DISPLAY_DOC_TITLE: UaClauses = UaClauses {
        one: Some("7.1"),
        two: Some("8.11.2"),
    };

    /// Optional content configurations (UA-1 7.10, UA-2 8.7).
    pub(crate) const OPTIONAL_CONTENT: UaClauses = UaClauses {
        one: Some("7.10"),
        two: Some("8.7"),
    };

    /// An embedded file's `/F` and `/UF`, both non-empty (UA-1 7.11-1).
    pub(crate) const EMBEDDED_FILE_NAMES: UaClauses = UaClauses {
        one: Some("7.11"),
        two: None,
    };

    /// An embedded file's `/Desc`, for every specification in the
    /// `/EmbeddedFiles` name tree (UA-2 8.14.1-1).
    pub(crate) const EMBEDDED_FILE_DESCRIPTION: UaClauses = UaClauses {
        one: None,
        two: Some("8.14"),
    };

    /// No reference XObject (UA-1 7.20-1). The UA-2 profile states no such
    /// rule.
    pub(crate) const REFERENCE_XOBJECTS: UaClauses = UaClauses {
        one: Some("7.20"),
        two: None,
    };

    /// An encrypted file permits extraction for accessibility (UA-1
    /// 7.16-1). The UA-2 profile states no such rule.
    pub(crate) const SECURITY: UaClauses = UaClauses {
        one: Some("7.16"),
        two: None,
    };

    /// No XFA form at all (UA-2 8.10.1-3). Part 1 forbids only *dynamic*
    /// XFA (7.15-1), which needs the XFA packet read and is staged.
    pub(crate) const XFA: UaClauses = UaClauses {
        one: None,
        two: Some("8.10.1"),
    };

    /// Font embedding (UA-1 7.21.4.1, UA-2 8.4.5.5.1).
    pub(crate) const FONT_EMBEDDING: UaClauses = UaClauses {
        one: Some("7.21.4.1"),
        two: Some("8.4.5.5.1"),
    };

    /// A composite font's collection against its CMap's (UA-1 7.21.3.1,
    /// UA-2 8.4.5.3.1).
    pub(crate) const CID_SYSTEM_INFO: UaClauses = UaClauses {
        one: Some("7.21.3.1"),
        two: Some("8.4.5.3.1"),
    };

    /// A Type 2 CIDFont's `/CIDToGIDMap` (UA-1 7.21.3.2, UA-2 8.4.5.3.2).
    pub(crate) const CID_TO_GID: UaClauses = UaClauses {
        one: Some("7.21.3.2"),
        two: Some("8.4.5.3.2"),
    };

    /// CMaps: embedded unless predefined, `/WMode` agreeing, no reference
    /// outside Table 118 (UA-1 7.21.3.3, UA-2 8.4.5.4).
    pub(crate) const CMAPS: UaClauses = UaClauses {
        one: Some("7.21.3.3"),
        two: Some("8.4.5.4"),
    };

    /// TrueType encodings: a non-symbolic font's `/Encoding` is WinAnsi or
    /// MacRoman, a symbolic one has none (UA-1 7.21.6-2 and -3, UA-2 8.4.5.7-2
    /// and -3).
    pub(crate) const TRUETYPE_ENCODINGS: UaClauses = UaClauses {
        one: Some("7.21.6"),
        two: Some("8.4.5.7"),
    };

    /// Unicode: every drawn code mapped, and never to U+0000, U+FEFF or
    /// U+FFFE (UA-1 7.21.7-1 and -2, UA-2 8.4.5.8-1 and -2).
    pub(crate) const UNICODE_MAPPING: UaClauses = UaClauses {
        one: Some("7.21.7"),
        two: Some("8.4.5.8"),
    };
}

/// Which rule groups a verdict ran.
///
/// The groups are [`crate::pdfa`]'s, keyed by the machinery they need, minus
/// colour, which ISO 14289 asks nothing of. Two more are named by the design
/// and do not exist yet — annotations inside the structure tree, and real
/// content over the recording device — and their clauses are in
/// [`Verdict::abstained`] rather than here, because a group that has no rules
/// cannot be said to have run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Coverage {
    /// The catalog and the object graph, which need no machinery past the
    /// COS document: the metadata stream's shape, `/DisplayDocTitle`,
    /// optional content configurations, embedded file specifications,
    /// reference XObjects, encryption and XFA.
    pub syntax: bool,
    /// The `pdfuaid` claim and `dc:title`, read from the XMP packet.
    pub metadata: bool,
    /// The logical structure tree: `/MarkInfo`, the tree root, its elements.
    pub structure: bool,
    /// The fonts the pages draw with.
    pub fonts: bool,
}

impl Coverage {
    /// Every group this build has rules for, which is the default request.
    pub const IMPLEMENTED: Coverage = Coverage {
        syntax: true,
        metadata: true,
        structure: true,
        fonts: true,
    };

    /// The object graph alone, which reaches for nothing past it.
    pub const SYNTAX: Coverage = Coverage {
        syntax: true,
        metadata: false,
        structure: false,
        fonts: false,
    };

    /// The claim alone: one pull-parse of a small packet, and nothing else.
    pub const METADATA: Coverage = Coverage {
        syntax: false,
        metadata: true,
        structure: false,
        fonts: false,
    };

    /// The structure tree alone, which reads no font program.
    pub const STRUCTURE: Coverage = Coverage {
        syntax: false,
        metadata: false,
        structure: true,
        fonts: false,
    };

    /// The font group alone.
    pub const FONTS: Coverage = Coverage {
        syntax: false,
        metadata: false,
        structure: false,
        fonts: true,
    };

    /// Whether every group this build has rules for ran.
    ///
    /// **Not** "the file conforms" even when the findings are empty:
    /// [`Verdict::abstained`] is never empty, and an empty finding list with
    /// complete coverage means "nothing this build decides was broken".
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.syntax && self.metadata && self.structure && self.fonts
    }
}

impl core::fmt::Display for Coverage {
    /// The groups that ran, comma-separated, or `nothing`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        for (ran, name) in [
            (self.syntax, "syntax"),
            (self.metadata, "metadata"),
            (self.structure, "structure"),
            (self.fonts, "fonts"),
        ] {
            if !ran {
                continue;
            }
            if !first {
                f.write_str(", ")?;
            }
            f.write_str(name)?;
            first = false;
        }
        if first {
            f.write_str("nothing")?;
        }
        Ok(())
    }
}

/// Why a clause was not decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AbstentionClass {
    /// A reader could decide it; the machinery is not built yet.
    Staged,
    /// No reader alone decides it: it is a judgement about meaning.
    Undecidable,
}

impl AbstentionClass {
    /// The word the census prints, which is never a rate.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            AbstentionClass::Staged => "staged",
            AbstentionClass::Undecidable => "undecidable",
        }
    }
}

/// One clause of one part this build does not decide, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UaGap {
    /// The part whose clause this is.
    pub part: UaPart,
    /// The clause, as that part numbers it.
    pub clause: &'static str,
    /// What the clause asks that is not decided.
    pub rule: &'static str,
    /// Why not: the missing machinery for a staged gap, the judgement for an
    /// undecidable one.
    pub because: &'static str,
}

/// A clause the verdict did not decide, in the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abstention {
    /// Staged or undecidable.
    pub class: AbstentionClass,
    /// The gap, as [`STAGED`] or [`UNDECIDABLE`] lists it.
    pub gap: &'static UaGap,
}

/// Every clause a reader could decide and this build does not yet.
///
/// Uncomfortable to read on purpose, as [`crate::pdfa::STAGED`] is: each entry
/// is a place an empty finding list says less than it looks like it says.
pub const STAGED: &[UaGap] = &[
    UaGap {
        part: UaPart::One,
        clause: "7.1",
        rule: "content is either an /Artifact or tagged real content, and \
               neither nests inside the other",
        because: "it is a question about every operator a page draws and the \
                  marked-content scopes open at it, which is the real-content \
                  group over the recording device (design/pdfua.md milestone \
                  5); a walk over dictionaries cannot answer it",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.2",
        rule: "the content models of tables, lists and tables of contents, \
               and table regularity across row and column spans",
        because: "the grammar is decidable from each element's standard type \
                  and kids, which the structure reader has; the regularity \
                  half needs the grid model design/table-reconstruction.md \
                  shares (milestone 3)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.2",
        rule: "a /Lang inside a marked-content property list well formed, and \
               the natural language determinable for alternative text, \
               expansions, annotations, form fields, outline entries and every \
               text item a page draws",
        because: "the catalog's and every structure element's /Lang are judged; \
                  a property list is in a content stream, and the per-item half \
                  needs the inheritance from parent elements and the \
                  marked-content scopes of the real-content group (milestone 5)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.4.4",
        rule: "one unnumbered H per structure node, and H never mixed with Hn",
        because: "decidable from the tree; not written (milestone 3)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.5",
        rule: "every TD reachable from a TH through /Scope or /Headers",
        because: "the attributes are read now; the header association over \
                  the table's grid is not written (milestone 3)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.7",
        rule: "a Formula's alternative description",
        because: "the Figure rule's twin, held back until the empty-/Alt \
                  reading the corpus states lands with it (milestone 2)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.9",
        rule: "every Note carries a unique /ID",
        because: "the /ID is read now; the rule is not written (milestone 3)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.15",
        rule: "no dynamic XFA form",
        because: "dynamic is the XFA packet's own statement \
                  (config/acrobat/acrobat7/dynamicRender = required), so the \
                  rule is a read of the packet's XML, which nothing here does; \
                  forbidding every XFA form instead would report a static one, \
                  which part 1 admits",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.18",
        rule: "annotations inside the structure tree: nested in Annot, Link \
               and Form elements, described by /Contents or /Alt, /Tabs /S on \
               their pages, no TrapNet, PrinterMark an artifact, a media \
               clip's /CT and /Alt",
        because: "the join between Page::annotations() and the tree's /OBJR \
                  kids is not built (milestone 4)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.20",
        rule: "a form XObject's content in one structure element however \
               often it is drawn",
        because: "it counts form invocations against /MCID ownership, which is \
                  the real-content group (milestone 5)",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.21",
        rule: "the font clauses that ask what a drawn code selects: every glyph \
               present in the program (7.21.4.1), CharSet and CIDSet complete \
               (7.21.4.2), /Widths against the program (7.21.5), the cmap \
               subtables a TrueType program carries (7.21.6), a /Differences \
               name on the Adobe Glyph List (7.21.6), a glyph name on it for an \
               unmapped Type 1 or Type 3 font and a /ToUnicode for a font drawn \
               only at rendering mode 3 (7.21.7), no .notdef drawn (7.21.8), \
               and a predefined CMap's collection against the CIDFont's \
               (7.21.3.1)",
        because: "the font group runs everything a font dictionary and its \
                  embedded CMap decide. These need the code-to-glyph mapping of \
                  every drawn code into the program, the Adobe Glyph List as \
                  vendored data, or the predefined CMaps' own collections, which \
                  are built in only behind the cmap-predefined feature; the PDF/A \
                  group stages the same four for the same reasons",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.2",
        rule: "the structure grammar ISO 32000-2 and ISO/TS 32005 define, the \
               namespaced role maps and structure types, a Document element at \
               the root, and real content against artifacts",
        because: "the grammar is milestone 3; the namespace rules (8.2.4) are \
                  stated over PDF 2.0's namespaces, whose type lists this build \
                  does not hold for 2.0 (the PDF 2.0 namespaces row), so part 1's \
                  structure-type rule does not run here; real content is \
                  milestone 5",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.4",
        rule: "the font clauses that ask what a drawn code selects (glyph \
               presence, widths, cmap subtables, Adobe Glyph List names, \
               .notdef, a predefined CMap's collection), a /Lang inside a \
               marked-content property list, and Private Use Area code points \
               without /ActualText",
        because: "the same machinery as ISO 14289-1 7.21's residue; the PUA rule \
                  needs the code-to-Unicode mapping of drawn glyphs and the \
                  marked-content scopes they are drawn in (milestone 6)",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.6",
        rule: "no Private Use Area code point in a text string",
        because: "milestone 6",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.8",
        rule: "every intra-document destination a structure destination",
        because: "milestone 6",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.9",
        rule: "annotations: artifacts when invisible, enclosed in Annot or \
               Link elements, described, /Tabs on their pages",
        because: "the annotation join (milestone 4)",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.10",
        rule: "widgets enclosed in Form elements, one per element",
        because: "the annotation join (milestone 4)",
    },
];

/// Every clause no reader alone decides.
///
/// Each reason is a judgement about meaning, in the words the design's
/// non-goals give it. These are limits rather than milestones: when the
/// roadmap row leaves, they belong in its named non-goals.
pub const UNDECIDABLE: &[UaGap] = &[
    UaGap {
        part: UaPart::One,
        clause: "7.1",
        rule: "that the tagging is correct: the content is tagged in the \
               order a person reads it, each element's type is the one its \
               content is, and nothing relies on colour or contrast alone",
        because: "whether the reading order is the author's, and whether a /P \
                  is a paragraph, is visible only to a person reading the page",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.2",
        rule: "that the natural language stated is the language the text is in",
        because: "a reader can tell a language tag is well formed and cannot \
                  tell it is the right one",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.3",
        rule: "that a figure's /Alt describes the picture",
        because: "an /Alt that lies is a well-formed /Alt",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.4",
        rule: "that a heading is a heading",
        because: "a /P around a heading reads exactly like a /P around a \
                  sentence",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.5",
        rule: "that a table's header cells are the headers its data is read by",
        because: "the association is checkable as structure; that it is the \
                  right one is a judgement about the table's meaning",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.7",
        rule: "that every mathematical expression is inside a Formula",
        because: "whether content is mathematics is a reading of it",
    },
    UaGap {
        part: UaPart::One,
        clause: "7.9",
        rule: "that notes and references are tagged as such",
        because: "whether text is a footnote is a reading of it",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.2",
        rule: "that the tagging is correct: reading order and each element's \
               type semantically appropriate",
        because: "the same judgement as ISO 14289-1 7.1, under PDF 2.0's \
                  structure types",
    },
    UaGap {
        part: UaPart::Two,
        clause: "8.4",
        rule: "that the natural language stated is the language the text is \
               in, and that alternative text says what the content is",
        because: "the same judgement as ISO 14289-1 7.2 and 7.3",
    },
];

/// What the PDF/UA validator made of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// The part the file claims, when it claims one this build knows.
    pub part: Option<UaPart>,
    /// Everything wrong that this build looked for, numbered by the claimed
    /// part — or by ISO 14289-1 when the file claims none, which is a
    /// presentation choice: the kind is what a caller matches on.
    pub findings: Vec<ConformanceFinding>,
    /// Which rule groups ran.
    pub coverage: Coverage,
    /// Every clause of the part this build did not decide, staged or
    /// undecidable. Never empty.
    pub abstained: Vec<Abstention>,
}

impl Verdict {
    /// Whether this build found anything wrong.
    ///
    /// **Not** "the file conforms": [`Verdict::abstained`] is beside it, and
    /// is what says how much was not looked at.
    #[must_use]
    pub fn found_nothing(&self) -> bool {
        self.findings.is_empty()
    }
}

/// A finding before it is placed: the clause table and what was wrong.
pub(crate) struct UaRaw {
    pub(crate) rule: UaClauses,
    pub(crate) object: Option<tinker_pdf_cos::ObjRef>,
    pub(crate) kind: FindingKind,
}

impl UaRaw {
    pub(crate) fn file(rule: UaClauses, kind: FindingKind) -> UaRaw {
        UaRaw {
            rule,
            object: None,
            kind,
        }
    }
}

/// Validates `document` against the part of ISO 14289 it claims.
pub(crate) fn validate(document: &Document, groups: Coverage) -> Verdict {
    validate_counting(document, groups).0
}

/// [`validate`], also returning what each group's machinery was reached for:
/// `(metadata, fonts, colour)`, in the order [`Machinery::reaches`] counts.
pub(crate) fn validate_counting(
    document: &Document,
    groups: Coverage,
) -> (Verdict, (u32, u32, u32)) {
    let machinery = Machinery::new(crate::pdfa::Coverage {
        metadata: groups.metadata,
        syntax: groups.syntax,
        structure: groups.structure,
        fonts: groups.fonts,
        colour: false,
    });
    let mut raw: Vec<UaRaw> = Vec::new();

    // The claim is read whatever was asked for, because every other group
    // numbers its findings by the part — the same reasoning, and the same
    // counted reach, as `pdfa::validate_counting`.
    machinery.reach(RuleGroup::Metadata);
    let mut claim = Vec::new();
    let part = claim_of(document, &mut claim);
    if groups.metadata {
        raw.append(&mut claim);
    }
    let numbering = part.unwrap_or(UaPart::One);

    if machinery.reach(RuleGroup::Syntax) {
        syntax::rules(&document.inner, numbering, &mut raw);
    }
    if groups.structure {
        structure::rules(document, &machinery, numbering, &mut raw);
    }
    if groups.fonts {
        fonts::rules(&document.inner, &machinery, numbering, &mut raw);
    }

    let findings = raw
        .into_iter()
        .filter_map(|raw| {
            raw.rule.of(numbering).map(|clause| ConformanceFinding {
                clause: Clause(clause.to_string()),
                object: raw.object,
                kind: raw.kind,
            })
        })
        .collect();
    let abstained = abstentions(numbering);
    let verdict = Verdict {
        part,
        findings,
        coverage: groups,
        abstained,
    };
    (verdict, machinery.reaches())
}

/// The staged and undecidable clauses of `part`, staged first.
fn abstentions(part: UaPart) -> Vec<Abstention> {
    let staged = STAGED.iter().map(|gap| (AbstentionClass::Staged, gap));
    let undecidable = UNDECIDABLE
        .iter()
        .map(|gap| (AbstentionClass::Undecidable, gap));
    staged
        .chain(undecidable)
        .filter(|(_, gap)| gap.part == part)
        .map(|(class, gap)| Abstention { class, gap })
        .collect()
}

// ---- the claim -------------------------------------------------------------

/// The PDF/UA identification schema's namespace.
const UA_ID_NAMESPACE: &str = "http://www.aiim.org/pdfua/ns/id/";

/// The prefix the identification schema fixes.
const UA_ID_PREFIX: &str = "pdfuaid";

/// The Dublin Core schema's namespace, whose `title` is the document's.
const DC_NAMESPACE: &str = "http://purl.org/dc/elements/1.1/";

/// What the packet says about PDF/UA.
#[derive(Debug, Default, PartialEq, Eq)]
struct Identification {
    part: Option<String>,
    revision: Option<String>,
    /// Whether the packet carries a `dc:title` property at all, which is
    /// what veraPDF's 7.1-9 and 8.11.1-1 test (`dc_title != null`).
    title: bool,
    /// `(property, prefix)` for every identification property written under
    /// a prefix other than `pdfuaid`.
    misprefixed: Vec<(String, String)>,
}

/// Reads `pdfuaid:part` and, for part 2, `pdfuaid:rev`, reporting what is
/// wrong with the claim.
fn claim_of(document: &Document, out: &mut Vec<UaRaw>) -> Option<UaPart> {
    let packet = document.xmp_metadata();
    let identification = packet.as_deref().map(identification).unwrap_or_default();

    // ISO 14289-1 7.1 and ISO 14289-2 8.11.1: the catalog's packet names
    // the document. Read in the same pass as the claim, because it is the
    // same packet; a packet that is not there at all is the syntax group's
    // finding about the stream, not this one's about a property.
    if packet.is_some() && !identification.title {
        out.push(UaRaw::file(
            clauses::METADATA_STREAM,
            FindingKind::DocumentTitleMissing,
        ));
    }

    for (property, found) in &identification.misprefixed {
        out.push(UaRaw::file(
            clauses::IDENTIFICATION,
            FindingKind::PdfUaIdentifierPrefix {
                property: property.clone(),
                found: found.clone(),
            },
        ));
    }

    let Some(text) = identification.part else {
        out.push(UaRaw::file(
            clauses::IDENTIFICATION,
            FindingKind::PdfUaIdentifierMissing,
        ));
        return None;
    };
    let Some(part) = text
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(UaPart::from_number)
    else {
        out.push(UaRaw::file(
            clauses::IDENTIFICATION,
            FindingKind::PdfUaPartUnknown { declared: text },
        ));
        return None;
    };

    // ISO 14289-2 identifies its revision as well as its part, the way ISO
    // 19005-4 does: `pdfuaid:rev`, a four-digit year. The published UA-2
    // profile asks for "2024"; a four-digit year is the reading that admits
    // an amendment the profile has not met yet, which is the safe direction.
    if part == UaPart::Two {
        match identification.revision {
            Some(revision) => {
                let trimmed = revision.trim();
                if trimmed.len() != 4 || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
                    out.push(UaRaw::file(
                        clauses::IDENTIFICATION,
                        FindingKind::PdfUaRevisionMalformed { declared: revision },
                    ));
                }
            }
            None => out.push(UaRaw::file(
                clauses::IDENTIFICATION,
                FindingKind::PdfUaRevisionMissing,
            )),
        }
    }
    Some(part)
}

/// Whether a name is in the PDF/UA identification schema.
///
/// The prefix **or** the resolved namespace, for the reason `pdfa::is_pdfaid`
/// gives at length: real packets use the conventional prefix without a
/// visible binding, and legal RDF binds the URI to another prefix. And never
/// `pdfaid`: reading one identification schema through the other is what once
/// made 434 PDF/UA files read as PDF/A claims.
fn is_uaid(name: &Name<'_>) -> bool {
    name.prefix() == Some(UA_ID_PREFIX) || name.namespace() == Some(UA_ID_NAMESPACE)
}

/// The identification properties, in either RDF spelling.
fn identification(packet: &[u8]) -> Identification {
    const PROPERTIES: &[&str] = &["part", "rev", "amd", "corr"];
    let mut found = Identification::default();
    let packet = crate::pdfa::readable(packet);
    let Ok(source) = Source::new(&packet) else {
        return found;
    };
    let limits = tinker_pdf_xml::Limits::default();

    let note_prefix = |name: &Name<'_>, found: &mut Identification| {
        let local = name.local();
        if !PROPERTIES.contains(&local) || name.prefix() == Some(UA_ID_PREFIX) {
            return;
        }
        let entry = (local.to_string(), name.prefix().unwrap_or("").to_string());
        if !found.misprefixed.contains(&entry) {
            found.misprefixed.push(entry);
        }
    };

    let mut collecting: Option<&'static str> = None;
    for event in source.reader(&limits) {
        let Ok(event) = event else {
            break;
        };
        match event {
            Event::Start(element) => {
                for attribute in element.attributes() {
                    let name = attribute.name();
                    if !is_uaid(name) {
                        continue;
                    }
                    note_prefix(name, &mut found);
                    match name.local() {
                        "part" if found.part.is_none() => {
                            found.part = Some(attribute.value().to_string());
                        }
                        "rev" if found.revision.is_none() => {
                            found.revision = Some(attribute.value().to_string());
                        }
                        _ => {}
                    }
                }
                if element.local() == "title"
                    && (element.name().namespace() == Some(DC_NAMESPACE)
                        || element.name().prefix() == Some("dc"))
                {
                    found.title = true;
                }
                collecting = if is_uaid(element.name()) {
                    note_prefix(element.name(), &mut found);
                    match element.local() {
                        "part" if found.part.is_none() => Some("part"),
                        "rev" if found.revision.is_none() => Some("rev"),
                        _ => None,
                    }
                } else {
                    None
                };
            }
            Event::Text(text) | Event::Cdata(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match collecting {
                    Some("part") => found.part = Some(trimmed.to_string()),
                    Some("rev") => found.revision = Some(trimmed.to_string()),
                    _ => {}
                }
            }
            Event::End(_) => collecting = None,
            _ => {}
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(description: &str) -> String {
        format!(
            r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
{description}</rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#
        )
    }

    /// Both RDF spellings, and never `pdfaid:part` — the confusion that read
    /// 434 PDF/UA files as PDF/A claims.
    #[test]
    fn the_ua_identifier_is_read_in_both_spellings_and_is_not_the_pdfa_one() {
        let attribute = packet(
            r#"<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/" pdfuaid:part="1"/>"#,
        );
        assert_eq!(
            identification(attribute.as_bytes()).part.as_deref(),
            Some("1")
        );

        let element = packet(
            r#"<rdf:Description rdf:about="" xmlns:pdfuaid="http://www.aiim.org/pdfua/ns/id/"><pdfuaid:part>2</pdfuaid:part><pdfuaid:rev>2024</pdfuaid:rev></rdf:Description>"#,
        );
        let read = identification(element.as_bytes());
        assert_eq!(read.part.as_deref(), Some("2"));
        assert_eq!(read.revision.as_deref(), Some("2024"));

        let pdfa = packet(
            r#"<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/" pdfaid:part="2" pdfaid:conformance="B"/>"#,
        );
        assert_eq!(identification(pdfa.as_bytes()).part, None);
        assert_eq!(identification(b"").part, None);
    }

    /// The namespace bound to another prefix is still the claim, and is a
    /// finding about its spelling.
    #[test]
    fn a_claim_under_another_prefix_is_read_and_reported() {
        let other = packet(
            r#"<rdf:Description rdf:about="" xmlns:ua="http://www.aiim.org/pdfua/ns/id/" ua:part="1"/>"#,
        );
        let read = identification(other.as_bytes());
        assert_eq!(read.part.as_deref(), Some("1"));
        assert_eq!(
            read.misprefixed,
            vec![("part".to_string(), "ua".to_string())]
        );
    }

    /// The staged and undecidable lists are filtered by part and are never
    /// empty for either: an empty abstention list would read as complete.
    #[test]
    fn every_part_abstains_on_something_in_both_classes() {
        for part in [UaPart::One, UaPart::Two] {
            let abstained = abstentions(part);
            assert!(abstained.iter().all(|a| a.gap.part == part));
            for class in [AbstentionClass::Staged, AbstentionClass::Undecidable] {
                assert!(
                    abstained.iter().any(|a| a.class == class),
                    "{part} has no {} clause",
                    class.word()
                );
            }
        }
    }

    /// The laziness requirement, counted with PDF/A's own instrument: a
    /// structure sweep reaches for the claim and the tree and never for the
    /// font group, and the full request reaches for each group once.
    #[test]
    fn a_structure_sweep_never_reaches_for_the_font_group() {
        let mut builder = crate::DocumentBuilder::new();
        builder.add_base_font(b"F1", b"Helvetica");
        builder.add_page(200.0, 200.0, |page| {
            page.tagged(b"P", |page| page.text(b"F1", 12.0, 10.0, 10.0, "x"));
        });
        let doc = crate::Document::open(builder.finish()).expect("the fixture opens");

        let (verdict, (metadata, fonts, colour)) = validate_counting(&doc, Coverage::STRUCTURE);
        assert_eq!(fonts, 0, "a structure sweep reached for the font group");
        assert_eq!(colour, 0, "PDF/UA has no colour group to reach for");
        assert_eq!(metadata, 1, "the claim is read, and counted");
        assert!(!verdict.coverage.fonts);

        let (_, (_, fonts, colour)) = validate_counting(&doc, Coverage::IMPLEMENTED);
        assert_eq!((fonts, colour), (1, 0));
    }

    #[test]
    fn a_partial_request_is_not_complete_coverage() {
        assert!(Coverage::IMPLEMENTED.is_complete());
        assert!(!Coverage::STRUCTURE.is_complete());
        assert_eq!(Coverage::default().to_string(), "nothing");
        assert_eq!(
            Coverage::IMPLEMENTED.to_string(),
            "syntax, metadata, structure, fonts"
        );
        assert!(!Coverage::SYNTAX.is_complete());
    }
}
