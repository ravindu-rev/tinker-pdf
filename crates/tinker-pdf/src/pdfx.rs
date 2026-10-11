//! PDF/X conformance (ISO 15930), read — `docs/design/pdfx.md`.
//!
//! The question [`crate::pdfa`] answers for ISO 19005, asked of the print
//! exchange standard: **which** PDF/X level does this file claim, and which of
//! that level's requirements does it break? The answer has the shape
//! [`crate::pdfua`] gave it — findings, the rule groups that ran, and, in the
//! same struct, every clause this build did not decide — because the honest
//! answer for ISO 15930 is mostly the third list.
//!
//! # The adjudicator, stated before the rules
//!
//! ISO 19005 has an annotated corpus and every PDF/A rule is held to it. ISO
//! 15930 has **none**: no annotated PDF/X conformance suite exists that this
//! project could find, and the requirement bodies of every part are sold. So
//! this validator has a **false-positive bar and no false-negative bar**, and
//! even the first is thin — the two published suites are a PDF/X-3:2002 file
//! set and a PDF/X-4 one, and the real claims the fetched corpora carry name
//! no 2003 level, so no third-party file anywhere in reach claims a level these
//! rules run under. What makes a rule *fire* is a fixture built here with a
//! near-miss twin (`tests/pdfx_rules.rs`), and a twin is this project's reading
//! in both directions: a clause read wrongly is read wrongly twice.
//!
//! # Where the rules come from
//!
//! *Application Notes for PDF/X Standards*, Version 4 (CGATS SC6 TF1,
//! September 2006), as `docs/design/pdfx.md` quotes it — a free restatement of
//! the 2003 levels that says of itself "if there is a conflict between these
//! Application Notes and any part of ISO 15930:2003, the standard will always
//! take precedence". Every rule here is a transcription of a **secondary
//! source**, cites the note's section, and runs only under the two levels the
//! notes cover: PDF/X-1a:2003 (ISO 15930-4) and PDF/X-3:2003 (ISO 15930-6).
//!
//! # Where the clause numbers come from
//!
//! Under PDF/X-1a:2003 a finding is numbered by **ISO 15930-4's table of
//! contents**, which the design records from the published preview: 6.2
//! colour, 6.3 fonts, 6.5 data compression, 6.6 trapping, 6.8 bounding boxes,
//! 6.10 PostScript, 6.11 the Encrypt dictionary, 6.13 annotations, 6.16
//! transparency. The number is the clause whose title names the subject, and
//! nothing finer: the subclauses are past the preview's last page.
//!
//! Under PDF/X-3:2003 no number of ISO 15930-6's is in hand — not even its
//! contents — so a finding cites the **application note's own section**,
//! written `AN 2.11`. That is the source the rule was transcribed from, and a
//! clause number made up to look like the standard's would be the one thing
//! here that is worse than an honest secondary citation. One rule, the private
//! `/Info` keys (AN 2.29), has no title in 15930-4's contents that plainly owns
//! it, and cites the note under both levels.
//!
//! # Where the claim is, and what it costs
//!
//! ISO 15930 puts the claim in the document information dictionary —
//! `GTS_PDFXVersion`, with `GTS_PDFXConformance` beside it for the 2001
//! levels — and 15930-4 clause 5 says nothing else counts: "Neither the version
//! number in the header of a PDF file, nor the value of the Version key in the
//! Catalog of a PDF file shall be used". So reading a PDF/X flavour is one
//! trailer-reachable dictionary and **no XML parse**, which the unit tests
//! count with PDF/A's own instrument. A claim made only in an XMP packet is,
//! for the 2003 levels, no claim.
//!
//! A file that claims nothing is not judged at all: every rule here is a rule
//! *of a level*, and a file that names none has not asked to be held to one.

use tinker_pdf_cos::{decode_text_string, CosDocument, Dict, ObjRef, Object};

use crate::pdfa::{Clause, ConformanceFinding, FindingKind, Machinery, RuleGroup};
use crate::Document;

mod print;
mod syntax;

/// Which PDF/X level a file claims, among the ones this build can name.
///
/// Six, of which **two are validated** ([`XFlavour::is_validated`]): the 2003
/// levels the application notes restate. The other four are identified so the
/// verdict can say what was claimed and that it was not checked, which is a
/// different answer from "nothing was claimed".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum XFlavour {
    /// PDF/X-1:2001 (ISO 15930-1) — the version key with no PDF/X-1a
    /// conformance key beside it. The level that admitted OPI and encryption;
    /// the design names it a non-goal and every later part dropped it.
    X1_2001,
    /// PDF/X-1a:2001 (ISO 15930-1): `GTS_PDFXVersion (PDF/X-1:2001)` and
    /// `GTS_PDFXConformance (PDF/X-1a:2001)`, as 15930-4 clause 5 states.
    X1a2001,
    /// PDF/X-3:2002 (ISO 15930-3).
    X3_2002,
    /// PDF/X-1a:2003 (ISO 15930-4): `GTS_PDFXVersion (PDF/X-1a:2003)`.
    X1a2003,
    /// PDF/X-3:2003 (ISO 15930-6): `GTS_PDFXVersion (PDF/X-3:2003)`.
    X3_2003,
    /// PDF/X-4 (ISO 15930-7:2010), by the version string real producers write.
    X4,
}

impl XFlavour {
    /// Every flavour, in the order the parts were published.
    pub const ALL: [XFlavour; 6] = [
        XFlavour::X1_2001,
        XFlavour::X1a2001,
        XFlavour::X3_2002,
        XFlavour::X1a2003,
        XFlavour::X3_2003,
        XFlavour::X4,
    ];

    /// The level's name, as its own identification spells it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            XFlavour::X1_2001 => "PDF/X-1:2001",
            XFlavour::X1a2001 => "PDF/X-1a:2001",
            XFlavour::X3_2002 => "PDF/X-3:2002",
            XFlavour::X1a2003 => "PDF/X-1a:2003",
            XFlavour::X3_2003 => "PDF/X-3:2003",
            XFlavour::X4 => "PDF/X-4",
        }
    }

    /// The part of ISO 15930 that defines the level.
    #[must_use]
    pub fn standard(self) -> &'static str {
        match self {
            XFlavour::X1_2001 | XFlavour::X1a2001 => "ISO 15930-1:2001",
            XFlavour::X3_2002 => "ISO 15930-3:2002",
            XFlavour::X1a2003 => "ISO 15930-4:2003",
            XFlavour::X3_2003 => "ISO 15930-6:2003",
            XFlavour::X4 => "ISO 15930-7:2010",
        }
    }

    /// Whether this build runs rules under the level.
    ///
    /// The 2003 levels only, because they are the ones the one free
    /// restatement of the requirements covers. Every other level's verdict
    /// is its claim and a list of what was not read.
    #[must_use]
    pub fn is_validated(self) -> bool {
        matches!(self, XFlavour::X1a2003 | XFlavour::X3_2003)
    }

    /// The level a claim names, if this build knows it.
    ///
    /// Exact comparison, as the notes state the values: a version string
    /// with a trailing space is a claim this build does not identify, and
    /// says so, rather than one it guesses at.
    fn of(claim: &XClaim) -> Option<XFlavour> {
        match claim.version.as_str() {
            "PDF/X-1a:2003" => Some(XFlavour::X1a2003),
            "PDF/X-3:2003" => Some(XFlavour::X3_2003),
            "PDF/X-3:2002" => Some(XFlavour::X3_2002),
            "PDF/X-4" => Some(XFlavour::X4),
            "PDF/X-1:2001" => Some(if claim.conformance.as_deref() == Some("PDF/X-1a:2001") {
                XFlavour::X1a2001
            } else {
                XFlavour::X1_2001
            }),
            _ => None,
        }
    }
}

impl core::fmt::Display for XFlavour {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// What the document information dictionary says, read as text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XClaim {
    /// `GTS_PDFXVersion`.
    pub version: String,
    /// `GTS_PDFXConformance`, which the 2001 levels use and the 2003 levels
    /// do not need.
    pub conformance: Option<String>,
}

/// One rule's clause under each level that runs it.
///
/// `None` is a level whose sources give the rule no citation, and the rule
/// does not run there — the shape `pdfua::UaClauses` has, for the same
/// reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct XClauses {
    x1a_2003: Option<&'static str>,
    x3_2003: Option<&'static str>,
}

impl XClauses {
    /// The clause `flavour` numbers this rule, if it runs there at all.
    pub(crate) fn of(self, flavour: XFlavour) -> Option<&'static str> {
        match flavour {
            XFlavour::X1a2003 => self.x1a_2003,
            XFlavour::X3_2003 => self.x3_2003,
            _ => None,
        }
    }
}

/// Every rule's citation, per level: ISO 15930-4's clause under
/// PDF/X-1a:2003, the application note's section under PDF/X-3:2003.
pub(crate) mod clauses {
    use super::XClauses;

    /// AN 2.11: "None of the PDF/X standards covered by these application
    /// notes permit the use of PDF-based encryption." 15930-4 6.11, the
    /// Encrypt dictionary.
    pub(crate) const ENCRYPTION: XClauses = XClauses {
        x1a_2003: Some("6.11"),
        x3_2003: Some("AN 2.11"),
    };

    /// AN 2.8: any lossless filter "other than LZW"; "JBIG2 compression may
    /// not be used". 15930-4 6.5, data compression.
    pub(crate) const COMPRESSION: XClauses = XClauses {
        x1a_2003: Some("6.5"),
        x3_2003: Some("AN 2.8"),
    };

    /// AN 2.17: `/Trapped` required, "a name object — /True or /False — and
    /// not a boolean". 15930-4 6.6, trapping.
    pub(crate) const TRAPPING: XClauses = XClauses {
        x1a_2003: Some("6.6"),
        x3_2003: Some("AN 2.17"),
    };

    /// AN 2.10: the page boxes. 15930-4 6.8, bounding boxes.
    pub(crate) const BOXES: XClauses = XClauses {
        x1a_2003: Some("6.8"),
        x3_2003: Some("AN 2.10"),
    };

    /// AN 2.28: annotations outside the bleed, printer's marks outside the
    /// trim. 15930-4 6.13, annotations.
    pub(crate) const ANNOTATIONS: XClauses = XClauses {
        x1a_2003: Some("6.13"),
        x3_2003: Some("AN 2.28"),
    };

    /// AN 2.29: private `/Info` keys are text strings. No title in 15930-4's
    /// contents plainly owns the rule, so the note is cited under both.
    pub(crate) const INFO_KEYS: XClauses = XClauses {
        x1a_2003: Some("AN 2.29"),
        x3_2003: Some("AN 2.29"),
    };

    /// AN 2.16: the output intent, and colour against it. 15930-4 6.2,
    /// colour.
    pub(crate) const COLOUR: XClauses = XClauses {
        x1a_2003: Some("6.2"),
        x3_2003: Some("AN 2.16"),
    };

    /// AN 2.25: transparency is prohibited in the 2003 levels. 15930-4 6.16.
    pub(crate) const TRANSPARENCY: XClauses = XClauses {
        x1a_2003: Some("6.16"),
        x3_2003: Some("AN 2.25"),
    };

    /// AN 2.26: no PostScript XObject and no `PS` operator. 15930-4 6.10,
    /// PostScript XObjects and the `PS` operator.
    pub(crate) const POSTSCRIPT: XClauses = XClauses {
        x1a_2003: Some("6.10"),
        x3_2003: Some("AN 2.26"),
    };

    /// AN 2.18: every font used is embedded. 15930-4 6.3, fonts.
    pub(crate) const FONTS: XClauses = XClauses {
        x1a_2003: Some("6.3"),
        x3_2003: Some("AN 2.18"),
    };
}

/// Which rule groups a verdict ran.
///
/// Three, by machinery as [`crate::pdfa`]'s are. There is no metadata group:
/// the 2003 levels put nothing in XMP, and the claim is read from `/Info`
/// whatever was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Coverage {
    /// The COS document alone: encryption, filters, `/Trapped`, the page
    /// boxes, annotations' rectangles, the `/Info` keys.
    pub syntax: bool,
    /// The content walk and the output intent: colour spaces against the
    /// intent, transparency, PostScript.
    pub print: bool,
    /// The fonts the pages draw with.
    pub fonts: bool,
}

impl Coverage {
    /// Every group this build has rules for, which is the default request.
    pub const IMPLEMENTED: Coverage = Coverage {
        syntax: true,
        print: true,
        fonts: true,
    };

    /// The object graph alone, which reaches for nothing past it.
    pub const SYNTAX: Coverage = Coverage {
        syntax: true,
        print: false,
        fonts: false,
    };

    /// The content walk and the output intent alone.
    pub const PRINT: Coverage = Coverage {
        syntax: false,
        print: true,
        fonts: false,
    };

    /// The font group alone.
    pub const FONTS: Coverage = Coverage {
        syntax: false,
        print: false,
        fonts: true,
    };

    /// Whether every group this build has rules for ran.
    ///
    /// **Not** "the file conforms" even with no findings: under a validated
    /// level, [`Verdict::abstained`] is never empty.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.syntax && self.print && self.fonts
    }
}

impl core::fmt::Display for Coverage {
    /// The groups that ran, comma-separated, or `nothing`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        for (ran, name) in [
            (self.syntax, "syntax"),
            (self.print, "print"),
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
    /// The requirement is in a source in hand and this build does not decide
    /// it yet: the machinery is missing, or the reading is not settled.
    Staged,
    /// The requirement's text is not in any source in hand. Not a judgement
    /// call and not a missing walk: a rule written from a clause *title* is a
    /// guess, and none is written.
    Unread,
}

impl AbstentionClass {
    /// The word the census prints, which is never a rate.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            AbstentionClass::Staged => "staged",
            AbstentionClass::Unread => "unread",
        }
    }
}

/// One clause of one level this build does not decide, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XGap {
    /// The level whose clause this is, or `None` for a claim this build does
    /// not identify.
    pub flavour: Option<XFlavour>,
    /// The clause: ISO 15930's number where its contents give one, the
    /// application note's section otherwise, the part where neither is in
    /// hand.
    pub clause: &'static str,
    /// What is not decided.
    pub rule: &'static str,
    /// Why not.
    pub because: &'static str,
}

/// A clause the verdict did not decide, in the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abstention {
    /// Staged or unread.
    pub class: AbstentionClass,
    /// The gap, as [`STAGED`] or [`UNREAD`] lists it.
    pub gap: &'static XGap,
}

/// What a whole level's gap says when no rule of it is transcribed.
const NO_TEXT_2001: &str = "the application notes v4 cover the 2003 levels only and defer the \
     2001 and 2002 ones to their Version 3, which was not found, and the \
     part's requirement bodies are past its published preview's last page";

/// Every requirement in a source in hand that this build does not decide.
pub const STAGED: &[XGap] = &[
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.5",
        rule: "LZW or JBIG2 inside an inline image's own dictionary",
        because: "an inline image lives inside a content stream, and the \
                  content walk this build shares with PDF/A steps over it at \
                  the byte level; the rule covers every stream the \
                  cross-reference table reaches and nothing else - the same \
                  gap PDFA_STAGED names for ISO 19005",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.8",
        rule: "LZW or JBIG2 inside an inline image's own dictionary",
        because: "as under PDF/X-1a:2003",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.2",
        rule: "DeviceRGB under a CMYK intent where the walk does not record \
               the space: a shading's own /ColorSpace, a pattern's cells, an \
               inline image",
        because: "the shared content walk records the spaces cs and CS \
                  select, an image XObject's, and the alternates under them; \
                  it does not open a shading dictionary or an inline image, \
                  for ISO 19005 or for this",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.16",
        rule: "DeviceRGB under a CMYK intent, and device-independent colour, \
               where the walk does not record the space: a shading's own \
               /ColorSpace, a pattern's cells, an inline image",
        because: "as under PDF/X-1a:2003",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.2",
        rule: "an output intent naming a registered characterization: its \
               identifier looked up in the ICC registry, and the device it \
               names held against the page's colour",
        because: "the registry is data with a date on it, and the design \
                  makes vendoring it a decision to take when a file turns on \
                  it; the identifier is checked for shape here, and an intent \
                  with no embedded profile says what device it is for in no \
                  form this build reads",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.16",
        rule: "an output intent naming a registered characterization: its \
               identifier looked up in the ICC registry, and the device it \
               names held against the page's colour",
        because: "as under PDF/X-1a:2003",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.3",
        rule: "a font only invisible text (rendering mode 3) is shown in",
        because: "the notes say every font used is embedded and do not say \
                  whether a font that paints nothing is used; it is read as \
                  ISO 19005 reads 'used for rendering', which errs towards \
                  silence, and the part's own text would settle it",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.18",
        rule: "a font only invisible text (rendering mode 3) is shown in",
        because: "as under PDF/X-1a:2003",
    },
];

/// Every requirement whose text is in no source this build was written from.
pub const UNREAD: &[XGap] = &[
    // ---- PDF/X-1a:2003, numbered by ISO 15930-4's contents ----------------
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.1",
        rule: "data structure",
        because: "a clause title in 15930-4's contents with no restatement in \
                  the application notes as the design quotes them",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.4",
        rule: "file specifications",
        because: "a clause title with no restatement in the notes as quoted",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.7",
        rule: "file identification, past the version string: whatever else \
               the clause asks of /Info or the trailer",
        because: "the notes' 2.3 restates one sentence of it - GTS_PDFXVersion \
                  in /Info equal to (PDF/X-1a:2003) - and that sentence is how \
                  the claim is read, so a file that does not carry it is not \
                  judged under this level at all; the clause's own body is \
                  past the preview's last page",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.9",
        rule: "extended graphics state",
        because: "a clause title with no restatement in the notes as quoted; \
                  which graphics-state entries it forbids is exactly what a \
                  title does not say",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.12",
        rule: "alternate images",
        because: "a clause title with no restatement in the notes as quoted",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.14",
        rule: "actions and JavaScripts",
        because: "a clause title with no restatement in the notes as quoted",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.15",
        rule: "BX and EX",
        because: "a clause title with no restatement in the notes as quoted",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.17",
        rule: "viewer preferences",
        because: "a clause title with no restatement in the notes as quoted",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.13",
        rule: "TrapNet annotations",
        because: "the design names a rule of their own beside PrinterMark's \
                  and quotes none of it, so a TrapNet annotation is held to \
                  neither annotation rule here",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.2",
        rule: "device-independent colour - ICCBased, CalRGB, Lab, a Default \
               space - for printing elements",
        because: "the notes' 3.1.1 forbids an ICCBased space for printing \
                  elements in the 2001 levels, as the design quotes it, and \
                  the 2003 level's own sentence is not in hand",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.2",
        rule: "the output intent dictionary's other entries, as the notes' \
               Table 2 gives its four shapes",
        because: "the design records that Table 2 states the shapes verbatim \
                  and does not reproduce it; what runs is the sentence the \
                  design does quote - a registered characterization by \
                  identifier and registry name, otherwise an embedded profile",
    },
    XGap {
        flavour: Some(XFlavour::X1a2003),
        clause: "6.5",
        rule: "JPXDecode, and the other filters the PDF version the 2003 \
               levels build on does not define",
        because: "the notes as quoted forbid LZW and JBIG2 and call JPEG the \
                  only lossy filter; whether JPEG 2000, defined after that \
                  version, falls inside the sentence is not in the text in hand",
    },
    // ---- PDF/X-3:2003 ------------------------------------------------------
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "15930-6",
        rule: "every requirement of ISO 15930-6 the application notes do not \
               restate",
        because: "neither the part's text nor its table of contents is in any \
                  source in hand, so not even its clause titles can be listed \
                  here; the rules that run under this level are the notes' \
                  and cite the notes",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.28",
        rule: "TrapNet annotations",
        because: "as under PDF/X-1a:2003",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.16",
        rule: "the output intent dictionary's other entries, as the notes' \
               Table 2 gives its four shapes",
        because: "as under PDF/X-1a:2003",
    },
    XGap {
        flavour: Some(XFlavour::X3_2003),
        clause: "AN 2.8",
        rule: "JPXDecode, and the other filters the PDF version the 2003 \
               levels build on does not define",
        because: "as under PDF/X-1a:2003",
    },
    // ---- the levels this build identifies and does not validate ----------
    XGap {
        flavour: Some(XFlavour::X1_2001),
        clause: "15930-1",
        rule: "every requirement of PDF/X-1:2001",
        because: "a non-goal (docs/design/pdfx.md): the 2001 level that \
                  admitted OPI and encryption, which the application notes \
                  call deprecated and every later part dropped",
    },
    XGap {
        flavour: Some(XFlavour::X1a2001),
        clause: "15930-1",
        rule: "every requirement of PDF/X-1a:2001",
        because: NO_TEXT_2001,
    },
    XGap {
        flavour: Some(XFlavour::X3_2002),
        clause: "15930-3",
        rule: "every requirement of PDF/X-3:2002",
        because: NO_TEXT_2001,
    },
    // ---- PDF/X-4, by ISO 15930-7:2010's contents -------------------------
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.1",
        rule: "general",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.2",
        rule: "non-print elements",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.3",
        rule: "complete exchange",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.4",
        rule: "colour",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.5",
        rule: "fonts",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.6",
        rule: "encoding of name objects",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.7",
        rule: "external and embedded files",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.8",
        rule: "stream filters",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.9",
        rule: "trapping",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.10",
        rule: "metadata and document identification",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.11",
        rule: "PDF/X-4 file identification",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.12",
        rule: "bounding boxes",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.13",
        rule: "extended graphics state",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.14",
        rule: "PostScript XObjects",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.15",
        rule: "encryption and access control",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.16",
        rule: "images",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.17",
        rule: "annotations",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.18",
        rule: "actions and JavaScripts",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.19",
        rule: "BX and EX",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.20",
        rule: "transparency",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.21",
        rule: "viewer preferences",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.22",
        rule: "alternate presentations",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.23",
        rule: "rendering intents",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.24",
        rule: "optional content",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.25",
        rule: "architectural limits",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.26",
        rule: "XFA forms",
        because: X4_UNREAD,
    },
    XGap {
        flavour: Some(XFlavour::X4),
        clause: "6.27",
        rule: "JPEG 2000 images",
        because: X4_UNREAD,
    },
    // ---- a claim no flavour above matches ---------------------------------
    XGap {
        flavour: None,
        clause: "GTS_PDFXVersion",
        rule: "every requirement of the level the claim names",
        because: "the version string is not one this build identifies: \
                  PDF/X-2 and PDF/X-5 are partial-exchange levels the design \
                  makes non-goals, and the strings PDF/X-4p and PDF/X-6 files \
                  carry are stated in no source in hand",
    },
];

/// The reason every X-4 clause gives.
const X4_UNREAD: &str = "ISO 15930-7's requirement bodies are past its published preview's \
     last page and the application notes predate it; the clause title is from \
     the preview's table of contents, and a rule written from a title is a guess";

/// What the PDF/X validator made of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// What `/Info` claims, when it carries `GTS_PDFXVersion` as text.
    pub claim: Option<XClaim>,
    /// The level the claim names, when this build identifies it.
    pub flavour: Option<XFlavour>,
    /// Everything wrong that this build looked for, under the claimed level.
    /// Always empty unless [`Verdict::flavour`] is a validated level.
    pub findings: Vec<ConformanceFinding>,
    /// Which rule groups ran — none, unless the level is validated.
    pub coverage: Coverage,
    /// Every clause of the claimed level this build did not decide. Empty
    /// only for a file that claims nothing.
    pub abstained: Vec<Abstention>,
}

impl Verdict {
    /// Whether this build found anything wrong.
    ///
    /// **Not** "the file conforms": [`Verdict::abstained`] says how much was
    /// not looked at, and [`Verdict::coverage`] what was.
    #[must_use]
    pub fn found_nothing(&self) -> bool {
        self.findings.is_empty()
    }
}

/// A finding before it is placed: the clause table and what was wrong.
pub(crate) struct XRaw {
    pub(crate) rule: XClauses,
    pub(crate) object: Option<ObjRef>,
    pub(crate) kind: FindingKind,
}

/// Validates `document` against the PDF/X level it claims.
pub(crate) fn validate(document: &Document, groups: Coverage) -> Verdict {
    validate_counting(document, groups).0
}

/// [`validate`], also returning what each group's machinery was reached for:
/// `(metadata, fonts, colour)`, in the order [`Machinery::reaches`] counts —
/// the print group's walk is counted as PDF/A's colour group's is.
pub(crate) fn validate_counting(
    document: &Document,
    groups: Coverage,
) -> (Verdict, (u32, u32, u32)) {
    let machinery = Machinery::new(crate::pdfa::Coverage {
        metadata: false,
        syntax: groups.syntax,
        structure: false,
        fonts: groups.fonts,
        colour: groups.print,
    });
    let doc = &document.inner;
    let claim = claim_of(doc);
    let flavour = claim.as_ref().and_then(XFlavour::of);

    let mut raw: Vec<XRaw> = Vec::new();
    let mut coverage = Coverage::default();
    if let Some(level) = flavour.filter(|f| f.is_validated()) {
        if machinery.reach(RuleGroup::Syntax) {
            syntax::rules(doc, &mut raw);
            coverage.syntax = true;
        }
        if groups.print {
            coverage.print = print::rules(doc, &machinery, level, &mut raw);
        }
        if groups.fonts {
            coverage.fonts = print::fonts(doc, &machinery, &mut raw);
        }
    }

    let findings = match flavour {
        Some(level) => raw
            .into_iter()
            .filter_map(|raw| {
                raw.rule.of(level).map(|clause| ConformanceFinding {
                    clause: Clause(clause.to_string()),
                    object: raw.object,
                    kind: raw.kind,
                })
            })
            .collect(),
        None => Vec::new(),
    };
    let abstained = if claim.is_some() {
        abstentions(flavour)
    } else {
        Vec::new()
    };
    let verdict = Verdict {
        claim,
        flavour,
        findings,
        coverage,
        abstained,
    };
    (verdict, machinery.reaches())
}

/// The staged and unread clauses of `flavour`, staged first.
fn abstentions(flavour: Option<XFlavour>) -> Vec<Abstention> {
    let staged = STAGED.iter().map(|gap| (AbstentionClass::Staged, gap));
    let unread = UNREAD.iter().map(|gap| (AbstentionClass::Unread, gap));
    staged
        .chain(unread)
        .filter(|(_, gap)| gap.flavour == flavour)
        .map(|(class, gap)| Abstention { class, gap })
        .collect()
}

// ---- the claim -------------------------------------------------------------

/// The document information dictionary, and the reference it is under when
/// it is indirect (ruling 10's object for a finding about it).
pub(crate) fn info(doc: &CosDocument) -> Option<(Dict, Option<ObjRef>)> {
    let key = doc.intern(b"Info");
    let at = doc.trailer().get_ref(key);
    let resolved = doc.resolve_key(doc.trailer(), key);
    resolved.as_dict().map(|dict| (dict.clone(), at))
}

/// `GTS_PDFXVersion` and `GTS_PDFXConformance`, as text.
///
/// A value that is not a string is no claim: 14.3.3 makes every `/Info`
/// value this standard uses a text string, and a name spelled
/// `/PDF/X-1a:2003` is not the string the notes give.
fn claim_of(doc: &CosDocument) -> Option<XClaim> {
    let (info, _) = info(doc)?;
    let version = text(doc, &info, b"GTS_PDFXVersion")?;
    let conformance = text(doc, &info, b"GTS_PDFXConformance");
    Some(XClaim {
        version,
        conformance,
    })
}

/// One `/Info` entry as a text string (7.9.2.2), if it is one.
fn text(doc: &CosDocument, info: &Dict, key: &[u8]) -> Option<String> {
    let value = doc.resolve_key(info, doc.intern(key));
    match value.as_ref() {
        Object::String(string) => Some(decode_text_string(&string.bytes)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claiming(version: Option<&str>, conformance: Option<&str>) -> Document {
        let mut builder = crate::DocumentBuilder::new();
        if let Some(version) = version {
            builder.set_info(b"GTS_PDFXVersion", version);
        }
        if let Some(conformance) = conformance {
            builder.set_info(b"GTS_PDFXConformance", conformance);
        }
        builder.add_page(200.0, 200.0, |_| {});
        Document::open(builder.finish()).expect("the fixture opens")
    }

    /// Every identification form the design's first milestone names, read
    /// from `/Info` alone.
    #[test]
    fn each_version_string_names_its_level() {
        for (version, conformance, expected) in [
            ("PDF/X-1a:2003", None, Some(XFlavour::X1a2003)),
            ("PDF/X-3:2003", None, Some(XFlavour::X3_2003)),
            ("PDF/X-3:2002", None, Some(XFlavour::X3_2002)),
            ("PDF/X-4", None, Some(XFlavour::X4)),
            (
                "PDF/X-1:2001",
                Some("PDF/X-1a:2001"),
                Some(XFlavour::X1a2001),
            ),
            ("PDF/X-1:2001", None, Some(XFlavour::X1_2001)),
            (
                "PDF/X-1:2001",
                Some("PDF/X-1:2001"),
                Some(XFlavour::X1_2001),
            ),
            ("PDF/X-1a:2003 ", None, None),
            ("", None, None),
            ("PDF/X-5g", None, None),
        ] {
            let verdict = claiming(Some(version), conformance).validate_pdfx();
            assert_eq!(verdict.flavour, expected, "{version:?} {conformance:?}");
            assert_eq!(
                verdict.claim.as_ref().map(|c| c.version.as_str()),
                Some(version)
            );
            assert!(
                !verdict.abstained.is_empty(),
                "{version:?}: a claim always names what was not decided"
            );
        }
    }

    /// No key, no claim, no rule run and nothing abstained on: a file that
    /// names no level has not asked to be held to one.
    #[test]
    fn a_file_claiming_nothing_is_not_judged() {
        let verdict = claiming(None, None).validate_pdfx();
        assert_eq!(verdict.claim, None);
        assert_eq!(verdict.flavour, None);
        assert!(verdict.findings.is_empty());
        assert!(verdict.abstained.is_empty());
        assert_eq!(verdict.coverage, Coverage::default());
    }

    /// The design's laziness exit criterion: reading a PDF/X flavour costs
    /// no XML parse — the metadata counter stays at zero — and each group
    /// that is asked for is reached for once.
    #[test]
    fn a_pdfx_claim_is_read_without_an_xml_parse() {
        let doc = claiming(Some("PDF/X-1a:2003"), None);
        let (verdict, (metadata, fonts, colour)) = validate_counting(&doc, Coverage::SYNTAX);
        assert_eq!(verdict.flavour, Some(XFlavour::X1a2003));
        assert_eq!(metadata, 0, "a PDF/X claim parsed XML");
        assert_eq!((fonts, colour), (0, 0), "a syntax sweep reached further");
        assert_eq!(verdict.coverage, Coverage::SYNTAX);

        let (verdict, (metadata, fonts, colour)) = validate_counting(&doc, Coverage::IMPLEMENTED);
        assert_eq!((metadata, fonts, colour), (0, 1, 1));
        assert!(verdict.coverage.is_complete());

        // An unvalidated level reaches for nothing at all.
        let x4 = claiming(Some("PDF/X-4"), None);
        let (verdict, reaches) = validate_counting(&x4, Coverage::IMPLEMENTED);
        assert_eq!(reaches, (0, 0, 0));
        assert_eq!(verdict.coverage, Coverage::default());
    }

    /// Each level's gaps are its own, both validated levels name staged and
    /// unread clauses, and every identified level abstains on something.
    #[test]
    fn every_claimed_level_abstains_on_something() {
        for flavour in XFlavour::ALL {
            let abstained = abstentions(Some(flavour));
            assert!(!abstained.is_empty(), "{flavour} abstains on nothing");
            assert!(abstained.iter().all(|a| a.gap.flavour == Some(flavour)));
            if flavour.is_validated() {
                for class in [AbstentionClass::Staged, AbstentionClass::Unread] {
                    assert!(
                        abstained.iter().any(|a| a.class == class),
                        "{flavour} has no {} clause",
                        class.word()
                    );
                }
            }
        }
        assert_eq!(abstentions(None).len(), 1);
        // The X-4 list is 15930-7's contents, 6.1 to 6.27, one each.
        assert_eq!(abstentions(Some(XFlavour::X4)).len(), 27);
    }

    #[test]
    fn a_partial_request_is_not_complete_coverage() {
        assert!(Coverage::IMPLEMENTED.is_complete());
        assert!(!Coverage::SYNTAX.is_complete());
        assert_eq!(Coverage::default().to_string(), "nothing");
        assert_eq!(Coverage::IMPLEMENTED.to_string(), "syntax, print, fonts");
    }
}
