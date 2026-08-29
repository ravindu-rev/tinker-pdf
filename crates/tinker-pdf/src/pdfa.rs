//! PDF/A conformance (ISO 19005), read.
//!
//! The question is not "is this a PDF/A" but "**which** PDF/A does this file
//! claim to be, and which clauses of that flavour does it break". Those are
//! different questions and only the second is useful: a file that claims
//! PDF/A-2b and breaks one metadata rule is a different problem from a file
//! that claims nothing, and a validator that answers `false` to both has told
//! a caller nothing they can act on.
//!
//! So a verdict is a **list of findings**, each naming its clause and, where
//! there is one, the object it is about. Conformance is the list being empty.
//! There is no boolean that discards the list.
//!
//! # Why the flavour comes from the metadata and not from the structure
//!
//! ISO 19005 puts the claim in an XMP packet: `pdfaid:part` and, for parts 1
//! to 3, `pdfaid:conformance`. Nothing else in a PDF says which flavour it is
//! meant to be, so a validator with no claim to check against has nothing to
//! do — and saying so is the honest answer rather than picking a flavour and
//! reporting against it.
//!
//! Measured over the 4 605 files in the fetched corpora: 3 231 carry an XMP
//! packet and **2 481 declare a `pdfaid:part`** — 839 part 1, 1 075 part 2, 28
//! part 3, 516 part 4. Two do not declare any of those: one says part 9 and
//! one says part 0, neither of which exists, which is why an unknown part is a
//! *finding* rather than an absence.
//!
//! **Part 4 carries no conformance letter**, and the corpus confirms it: 1 941
//! files declare one against 2 459 that declare a part, and the difference is
//! very nearly the 516 part-4 files. So a level on a part-4 file and a missing
//! level on a part-1-to-3 file are both findings, in opposite directions.
//!
//! # What this module does not do yet
//!
//! Milestone 1 of `docs/design/pdfa.md`: the verdict types, the flavour, and
//! the rule table's shape. The rule *groups* — syntax, font, colour, XMP — are
//! milestones 2 to 5, and until they land a clean verdict means "nothing this
//! build checks was broken" rather than "this file conforms".
//! [`Verdict::coverage`] is what says which, and it is not decoration: a
//! validator that cannot say what it did not check is a validator whose empty
//! answer cannot be read.

use std::borrow::Cow;
use std::cell::Cell;

use tinker_pdf_cos::ObjRef;
use tinker_pdf_xml::{Event, Source};

use crate::Document;

mod colour;
mod content;
mod fonts;
mod syntax;
mod xmp;

/// A finding before its clause number is known.
///
/// Rules fire while the flavour is still being read — the metadata group is
/// what finds out which part the file claims, and every other group needs that
/// answer to number its clauses — so a rule names the *rule* and the clause is
/// resolved once, at the end. [`ClauseTable`] is that mapping, and it is data
/// rather than a branch inside each rule, which is what the design doc means
/// by "the mapping table is data, not duplicated rules".
pub(crate) struct Raw {
    /// Which rule fired, as a clause number per part.
    pub(crate) rule: ClauseTable,
    /// The object it is about, when there is one (ruling 10).
    pub(crate) object: Option<ObjRef>,
    /// What was wrong.
    pub(crate) kind: FindingKind,
}

impl Raw {
    /// A finding about the file rather than about an object inside it.
    pub(crate) fn file(rule: ClauseTable, kind: FindingKind) -> Raw {
        Raw {
            rule,
            object: None,
            kind,
        }
    }

    fn resolve(self, part: Option<Part>) -> ConformanceFinding {
        ConformanceFinding {
            clause: Clause(self.rule.of(part).to_string()),
            object: self.object,
            kind: self.kind,
        }
    }
}

/// One rule's clause number in each part that numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClauseTable {
    one: &'static str,
    two_three: &'static str,
    four: &'static str,
}

impl ClauseTable {
    /// The clause number `part` gives this rule.
    ///
    /// A file that claimed no part is numbered as part 1 would number it,
    /// which is a presentation choice rather than a validation one: the `kind`
    /// is what a caller matches on, and a finding carrying a plausible clause
    /// number beats a finding carrying none.
    fn of(self, part: Option<Part>) -> &'static str {
        match part {
            Some(Part::Two | Part::Three) => self.two_three,
            Some(Part::Four) => self.four,
            _ => self.one,
        }
    }
}

/// Every rule's clause number, per part.
///
/// The same defect is numbered differently by each part — a stream's external
/// file reference is 6.1.7 in part 1, 6.1.7.1 in parts 2 and 3 and 6.1.6 in
/// part 4 — so the number is data about the flavour being checked rather than
/// a property of the check.
pub(crate) mod clauses {
    use super::ClauseTable;

    /// The file header (6.1.2 in every part).
    pub(crate) const FILE_HEADER: ClauseTable = ClauseTable {
        one: "6.1.2",
        two_three: "6.1.2",
        four: "6.1.2",
    };

    /// The file trailer, which is also where part 4 puts its rule about the
    /// document information dictionary.
    pub(crate) const FILE_TRAILER: ClauseTable = ClauseTable {
        one: "6.1.3",
        two_three: "6.1.3",
        four: "6.1.3",
    };

    /// Encryption. Part 4 renumbers the clause that forbids it.
    pub(crate) const ENCRYPTION: ClauseTable = ClauseTable {
        one: "6.1.3",
        two_three: "6.1.3",
        four: "6.1.2",
    };

    /// Stream objects, including the external-file keys.
    pub(crate) const STREAM_OBJECTS: ClauseTable = ClauseTable {
        one: "6.1.7",
        two_three: "6.1.7.1",
        four: "6.1.6",
    };

    /// Filters.
    pub(crate) const FILTERS: ClauseTable = ClauseTable {
        one: "6.1.10",
        two_three: "6.1.7.1",
        four: "6.1.6",
    };

    /// The permissions dictionary. Part 1 has no rule about it; the number
    /// there is the nearest file-structure clause and is never reached,
    /// because the rule does not run for part 1.
    pub(crate) const PERMISSIONS: ClauseTable = ClauseTable {
        one: "6.1.12",
        two_three: "6.1.12",
        four: "6.1.11",
    };

    /// The document catalog dictionary, which is part 4's clause for the
    /// catalog's `/Version`.
    pub(crate) const CATALOG: ClauseTable = ClauseTable {
        one: "6.1.12",
        two_three: "6.1.13",
        four: "6.1.12",
    };

    /// Embedded files.
    pub(crate) const EMBEDDED_FILES: ClauseTable = ClauseTable {
        one: "6.1.11",
        two_three: "6.8",
        four: "6.9",
    };

    /// Optional content, which part 1 forbids outright.
    pub(crate) const OPTIONAL_CONTENT: ClauseTable = ClauseTable {
        one: "6.1.13",
        two_three: "6.9",
        four: "6.10",
    };

    /// Interactive forms, which is where XFA is refused.
    pub(crate) const INTERACTIVE_FORMS: ClauseTable = ClauseTable {
        one: "6.9",
        two_three: "6.4",
        four: "6.4",
    };

    /// Actions.
    pub(crate) const ACTIONS: ClauseTable = ClauseTable {
        one: "6.6.1",
        two_three: "6.5.1",
        four: "6.6.1",
    };

    /// Trigger events, the additional-actions dictionaries.
    pub(crate) const TRIGGERS: ClauseTable = ClauseTable {
        one: "6.6.2",
        two_three: "6.5.2",
        four: "6.6.3",
    };

    /// The metadata stream itself.
    pub(crate) const METADATA: ClauseTable = ClauseTable {
        one: "6.7.2",
        two_three: "6.6.2.1",
        four: "6.7.2",
    };

    /// Version and conformance level identification: the `pdfaid` claim.
    pub(crate) const FLAVOUR_ID: ClauseTable = ClauseTable {
        one: "6.7.11",
        two_three: "6.6.4",
        four: "6.7.3",
    };

    /// The document information dictionary's agreement with the XMP.
    pub(crate) const INFO_XMP: ClauseTable = ClauseTable {
        one: "6.7.3",
        two_three: "6.1.5",
        four: "6.1.3",
    };

    // ---- the font group (milestone 5) ------------------------------------
    //
    // Part 1 gives fonts a clause of their own at 6.3. Parts 2 to 4 moved them
    // *inside* graphics — 6.2.11 in parts 2 and 3, 6.2.10 in part 4 — so the
    // same rule is numbered three ways and the sub-clause tails line up one
    // for one under each head. The corpus's own directory names are the check
    // on this table: `PDF_A-1b/6.3 Fonts/6.3.5 Font subsets` and
    // `PDF_A-2b/6.2 Graphics/6.2.11 Fonts/6.2.11.4 Embedding` are the same
    // subject filed under two numbers.

    /// Embedding: every font program present in the file (6.3.4 / 6.2.11.4.1 /
    /// 6.2.10.4.1).
    pub(crate) const FONT_EMBEDDING: ClauseTable = ClauseTable {
        one: "6.3.4",
        two_three: "6.2.11.4.1",
        four: "6.2.10.4.1",
    };

    /// Font subsets: the six-letter tag and the descriptor's own name
    /// (6.3.5 / 6.2.11.4.2 / 6.2.10.4.1).
    pub(crate) const FONT_SUBSETS: ClauseTable = ClauseTable {
        one: "6.3.5",
        two_three: "6.2.11.4.2",
        four: "6.2.10.4.1",
    };

    /// Character encodings (6.3.7 / 6.2.11.6 / 6.2.10.6).
    pub(crate) const FONT_ENCODINGS: ClauseTable = ClauseTable {
        one: "6.3.7",
        two_three: "6.2.11.6",
        four: "6.2.10.6",
    };

    /// Unicode character maps (6.3.8 / 6.2.11.7 / 6.2.10.7).
    pub(crate) const FONT_UNICODE: ClauseTable = ClauseTable {
        one: "6.3.8",
        two_three: "6.2.11.7",
        four: "6.2.10.7",
    };

    /// Composite fonts: the CIDFont dictionary (6.3.3.2 / 6.2.11.3.2 /
    /// 6.2.10.3.2).
    pub(crate) const CID_FONTS: ClauseTable = ClauseTable {
        one: "6.3.3.2",
        two_three: "6.2.11.3.2",
        four: "6.2.10.3.2",
    };

    // ---- the colour group (milestone 5) ----------------------------------

    /// The output intent (6.2.2 in part 1, 6.2.3 in parts 2 to 4).
    pub(crate) const OUTPUT_INTENT: ClauseTable = ClauseTable {
        one: "6.2.2",
        two_three: "6.2.3",
        four: "6.2.3",
    };

    /// ICCBased colour spaces (6.2.3.2 / 6.2.4.2).
    pub(crate) const ICC_SPACES: ClauseTable = ClauseTable {
        one: "6.2.3.2",
        two_three: "6.2.4.2",
        four: "6.2.4.2",
    };

    /// The uncalibrated - device - colour spaces (6.2.3.3 / 6.2.4.3).
    pub(crate) const DEVICE_SPACES: ClauseTable = ClauseTable {
        one: "6.2.3.3",
        two_three: "6.2.4.3",
        four: "6.2.4.3",
    };

    /// Rendering intents (6.2.9 in part 1, 6.2.6 in parts 2 and 3, and part
    /// 4's extended-graphics-state clause, which is where it puts them).
    pub(crate) const RENDERING_INTENTS: ClauseTable = ClauseTable {
        one: "6.2.9",
        two_three: "6.2.6",
        four: "6.2.5",
    };

    /// Transparency, which part 1 forbids outright at 6.4 and parts 2 to 4
    /// constrain at 6.2.10 and 6.2.9.
    pub(crate) const TRANSPARENCY: ClauseTable = ClauseTable {
        one: "6.4",
        two_three: "6.2.10",
        four: "6.2.9",
    };
}

/// One rule this build does not run yet, and what it is waiting for.
///
/// A staged rule is a **named refusal** rather than a silent pass.
/// [`Coverage`] says which rule *groups* ran; this says which rules inside a
/// group that did run are still missing, which is the difference between "we
/// checked and it was fine" and "we did not check". Milestone 4's ledger
/// classifies a disagreement against this list, so a ledger row claiming "a
/// known staged rule" has to name one that is actually here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagedRule {
    /// The clause it would cite, as parts 1 to 3 number it.
    pub clause: &'static str,
    /// What the rule would check.
    pub rule: &'static str,
    /// Why it is not running.
    pub because: &'static str,
}

/// Every rule the syntax and metadata groups know they are not running.
///
/// This list is deliberately uncomfortable to read. Each entry is a place
/// where a clean verdict means less than it looks like it means, and the
/// alternative — leaving them out — is a validator whose silence cannot be
/// interpreted.
pub const STAGED: &[StagedRule] = &[
    StagedRule {
        clause: "6.1.6",
        rule: "string objects: hexadecimal strings with an odd digit count or \
               a non-hexadecimal character",
        because: "the lexer normalises both away before a rule could see them; \
                  catching it needs a second pass over the raw bytes of every \
                  string, which is a lexer change rather than a rule",
    },
    StagedRule {
        clause: "6.1.8",
        rule: "indirect objects: the EOL markers around `obj`, `endobj`, \
               `stream` and `endstream`",
        because: "the same reason — the parser has consumed the whitespace by \
                  the time an object exists to have a rule applied to it",
    },
    StagedRule {
        clause: "6.1.12",
        rule: "implementation limits: string length, name length, integer and \
               real magnitude, dictionary size, nesting depth",
        because: "the limits differ per part and several of them are `should` \
                  rather than `shall`; a rule guessed at the wrong side of \
                  that line reports conforming files as broken",
    },
    StagedRule {
        clause: "6.5.2",
        rule: "trigger events for part 4",
        because: "ISO 19005-4 6.6.3 permits some additional-action entries and \
                  forbids others rather than forbidding the entry, and this \
                  build has not read that list closely enough to enforce it",
    },
    StagedRule {
        clause: "6.8",
        rule: "part 2: an embedded file must itself be PDF/A",
        because: "it needs a recursive validation of the attachment, which is \
                  a validator calling itself on untrusted bytes and wants its \
                  own depth bound before it exists",
    },
    StagedRule {
        clause: "6.6.2.3",
        rule: "XMP properties must belong to a predefined schema or be \
               described by an extension schema",
        because: "it needs the predefined-schema property tables from the XMP \
                  specification as vendored data; guessing them produces false \
                  positives on conforming files, which is worse than not \
                  checking. This is the single largest staged rule by corpus \
                  count and the ledger says so",
    },
    StagedRule {
        clause: "6.1.5",
        rule: "parts 2 and 3: the document information dictionary's agreement                with the XMP packet",
        because: "ISO 19005-1 6.7.3 states the requirement plainly and the rule                   runs there. Whether ISO 19005-2 kept it when it moved the                   subject from clause 6.7 to clause 6.1 could not be                   established from the clause text available here, and running                   it on an uncertain reading reports conforming files as broken",
    },
    StagedRule {
        clause: "6.6.1",
        rule: "part 4 level E: the forbidden-action list",
        because: "PDF/A-4e is the engineering level and exists to permit 3D                   artwork and rich media that the other levels forbid. How far                   that relaxation reaches into the action list is not                   established here, so the prohibition does not run for level E                   rather than running on a guess",
    },
    StagedRule {
        clause: "6.1.10",
        rule: "the LZWDecode filter inside an inline image",
        because: "an inline image's dictionary lives inside a content stream,                   and opening one is the interpreter's job rather than this                   group's — the rule covers every filter reachable from the                   cross-reference table and no filter that is not",
    },
    StagedRule {
        clause: "6.2.2",
        rule: "the destination profile's own conformance: its ICC version, \
               its device class, and whether it is a well-formed profile at \
               all",
        because: "tinker-pdf-color's icc::Profile::parse is a transform \
                  builder, not a validator. It refuses a profile it cannot \
                  build a transform from - a v4 profile whose only route to \
                  the connection space is an mAB tag, for one - and such a \
                  profile conforms to ICC.1 perfectly well. Reporting every \
                  one of them would report conforming files, so a profile \
                  this build cannot read leaves the intent's colour space \
                  unknown and the rules that need it do not fire",
    },
    StagedRule {
        clause: "6.2.3.4",
        rule: "Separation and DeviceN: the tint transform function, and two \
               colourants of the same name having the same transform",
        because: "the alternate space is read and judged, which is the half \
                  that decides whether the colour can be reproduced. The tint \
                  transform is a PDF function, and comparing two of them for \
                  equality means comparing sampled or PostScript-calculator \
                  functions - a definition of equality this build has not \
                  written down",
    },
    StagedRule {
        clause: "6.2.4",
        rule: "images: the /Interpolate prohibition, /Alternates, /OPI, and \
               the JPEG2000 constraints parts 2 to 4 add",
        because: "an image's colour space is judged with every other colour \
                  space. These are separate prohibitions on the image \
                  dictionary, and the JPEG2000 half needs the codestream's \
                  own header rather than the PDF's",
    },
    StagedRule {
        clause: "6.2.5",
        rule: "form and reference XObjects: /Ref, PostScript XObjects, /OPI, \
               and the /Subtype2 that makes a reference XObject one",
        because: "prohibitions on an XObject dictionary rather than on a \
                  colour, filed under the graphics clause because that is \
                  where the standard files them. Not written",
    },
    StagedRule {
        clause: "6.2.8",
        rule: "the extended graphics state beyond transparency: /TR, /TR2, \
               /HTP and the halftone constraints",
        because: "the transparency keys of an /ExtGState are read for part 1 \
                  and the rendering intent for every part. The transfer \
                  function and halftone prohibitions are a separate list this \
                  build has not established from the clause text available",
    },
    StagedRule {
        clause: "6.2.10",
        rule: "content streams: the operators a conforming stream may use, \
               and the resources every name in it must resolve to",
        because: "the walk this group runs over content streams is a \
                  tokenizer with a text state, not an interpreter, and \
                  deciding that an operator is forbidden means knowing the \
                  operand stack it was given. That is a renderer's job",
    },
    StagedRule {
        clause: "6.4",
        rule: "transparency in parts 2 to 4: the blending colour space a \
               group declares, isolation and knockout, and the soft masks \
               those parts permit",
        because: "part 1 forbids transparency outright and that rule runs. \
                  Parts 2 to 4 permit it and constrain it, and the \
                  constraints are about what a group composites in rather \
                  than about whether it exists - the same reading of 11.6.6 \
                  this group already uses to excuse a device colour space, \
                  turned into a rule, which is a larger step than reusing it",
    },
    StagedRule {
        clause: "6.3.6",
        rule: "font metrics: the /Widths array against the embedded \
               program's own advances",
        because: "the advance is one call into tinker-pdf-font away, and the \
                  mapping from a character code to the glyph whose advance it \
                  is, is not: a symbolic TrueType font resolves a code \
                  through a (3, 0) cmap subtable with the code offset into \
                  the private-use area, a Type 1 font through the program's \
                  own encoding vector, and a rule that got either wrong would \
                  report conforming files by the hundred",
    },
    StagedRule {
        clause: "6.3.3.3",
        rule: "composite fonts: the CMap, its agreement with the CIDFont's \
               /CIDSystemInfo, and the predefined-CMap list a /Encoding name \
               must come from",
        because: "the predefined CMaps are a published table this build \
                  carries behind the cmap-predefined feature rather than as \
                  validation data, so a default build and a --no-default-\
                  features build would disagree about whether a file \
                  conforms. A conformance verdict that depends on a cargo \
                  feature is not a verdict",
    },
    StagedRule {
        clause: "6.3.2",
        rule: "font types: which of ISO 32000's font subtypes each part \
               admits, and part 4's prohibition on Type 3",
        because: "part 1 admits Type 3 and part 4 does not, and the clause \
                  that says so also carries the conditions under which each \
                  of the others is admitted. This build checks what a font \
                  carries rather than whether its kind is on the list",
    },
    StagedRule {
        clause: "6.3.8",
        rule: "Unicode character maps: whether the glyph names an /Encoding \
               dictionary's /Differences array introduces are on the Adobe \
               Glyph List, and whether the page drew one that is not",
        because: "the corpus settles this one rather than the clause text: \
                  6-3-8-t01-fail-b.pdf and 6-3-8-t01-pass-e.pdf carry the same \
                  two Type 1 fonts, the same /Encoding dictionary and no \
                  /ToUnicode on either, and are annotated fail and pass. What \
                  separates them is not in the font dictionary at all, so no \
                  rule over font dictionaries can find it — it needs the glyph \
                  list as vendored data and the glyphs the content stream \
                  drew. The half that *is* in the dictionary runs",
    },
    StagedRule {
        clause: "6.3.9",
        rule: "the .notdef glyph, and /ActualText where a mapping is absent",
        because: "both are about what a content stream draws rather than \
                  about what a font dictionary carries, and finding out what \
                  is drawn is the interpreter's job rather than this group's",
    },
    StagedRule {
        clause: "6.1.4",
        rule: "the cross-reference table's own syntax: subsection header \
                spacing, the prohibition on a cross-reference stream in a \
                part 1 file, and hybrid-reference files",
        because: "the reader merges every revision's table into one \
                   before a rule could see how any of them was spelled. \
                   Catching this needs the per-section bytes, which the \
                   strict structural validator in tinker-pdf-cos already \
                   walks and this group does not",
    },
    StagedRule {
        clause: "6.1.7",
        rule: "stream objects: the EOL after the stream keyword, the \
                endstream keyword's own EOL, and Length against the \
                actual byte count",
        because: "the same reason as 6.1.8 - the parser has resolved the \
                   extent and consumed the whitespace by the time a \
                   stream object exists to have a rule applied to it",
    },
    StagedRule {
        clause: "6.1.9",
        rule: "inline image dictionaries",
        because: "an inline image lives inside a content stream, and \
                   opening one is the interpreter's job rather than this \
                   group's. Every filter reachable from the cross- \
                   reference table is checked; no filter that is not",
    },
    StagedRule {
        clause: "6.1.12",
        rule: "the contents of a permissions dictionary: what the DocMDP \
                transform inside Perms says",
        because: "this build checks which keys Perms carries, which is \
                   the clause's first sentence, and not what the \
                   signature reference dictionary under DocMDP permits",
    },
    StagedRule {
        clause: "6.3.1",
        rule: "annotations: the permitted subtypes, the flags an \
                annotation dictionary must and must not set, and the \
                appearance stream every annotation needs",
        because: "milestone 5's neighbour rather than milestone 5 \
                   itself. The appearance rules need the annotation \
                   appearance machinery and the colour rules need an \
                   output intent, and neither is in this group",
    },
    StagedRule {
        clause: "6.4",
        rule: "transparency in part 1, and interactive form field \
                appearances in parts 2 to 4",
        because: "part 1's transparency prohibition needs the graphics \
                   state, and the form rules need the appearance streams \
                   the annotation group will bring. Both are milestone 5",
    },
    StagedRule {
        clause: "6.5",
        rule: "part 1's annotation rules",
        because: "part 1 numbers annotations at 6.5 where parts 2 to 4 \
                   number them at 6.3. The rules are staged for the same \
                   reason and the clause number is the only difference",
    },
    StagedRule {
        clause: "6.6.3",
        rule: "part 4's trigger events",
        because: "ISO 19005-4 6.6.3 permits some additional-action \
                   entries and forbids others rather than forbidding the \
                   entry. The same refusal as the 6.5.2 row, under part \
                   4's clause number",
    },
    StagedRule {
        clause: "6.7",
        rule: "logical structure: the tagged structure tree level A \
                requires, its artefacts, and natural language \
                specification",
        because: "docs/design/tagged-pdf.md owns the structure tree, and \
                   docs/design/pdfa.md's non-goals stage level A behind \
                   it rather than claiming it wrongly",
    },
    StagedRule {
        clause: "6.7.8",
        rule: "XMP extension schemas: the description a packet must \
                carry for a property outside the predefined schemas",
        because: "the other half of the predefined-schema rule, and \
                   staged with it. A validator that checked the extension \
                   schema without the schema tables would be checking the \
                   exception to a rule it does not enforce",
    },
    StagedRule {
        clause: "6.7.11",
        rule: "version identification beyond the part, level and \
                revision: whether the packet's pdfaid schema is described \
                where the part requires a description",
        because: "this build reads the claim and checks it against the \
                   part's own table of levels. What it does not do is \
                   check the claim against the schema description the \
                   packet is supposed to carry for it",
    },
    StagedRule {
        clause: "6.8",
        rule: "embedded files: the AF back-reference from a document, \
                page or annotation to the file specification, and the \
                MIME type an embedded file stream declares",
        because: "this build checks the keys a file specification must \
                   carry. The reference to it from elsewhere in the \
                   document, and whether the attachment is itself PDF/A, \
                   are separate walks",
    },
    StagedRule {
        clause: "6.10",
        rule: "optional content configuration: the entries a part 2-to-4 \
                file's OCProperties may carry",
        because: "part 1 forbids optional content outright and that rule \
                   runs. The parts that permit it constrain the \
                   configuration dictionary, which is a rule about D and \
                   AS this build has not written",
    },
    StagedRule {
        clause: "6.11",
        rule: "alternate presentations and page transitions",
        because: "two fixtures and no rule. The clause is short and the \
                   rule is simply not written, which is a gap rather than \
                   a difficulty",
    },
    StagedRule {
        clause: "6.12",
        rule: "the document requirements dictionary",
        because: "one fixture and no rule. The same gap as the 6.11 row, \
                   and the same honest answer",
    },
    StagedRule {
        clause: "6.9",
        rule: "part 1's interactive form rules, and part 4's embedded \
                files",
        because: "part 1's 6.9 needs the appearance and font machinery \
                   its fixtures turn on, and part 4's 6.9 is the \
                   embedded-file row under part 4's clause number",
    },
];

/// Which machinery a reach is for.
///
/// Public because the counter that records reaches is what makes the design
/// doc's laziness requirement a property rather than a comment, and a private
/// enum whose variants nothing constructs is a dead-code warning rather than a
/// guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleGroup {
    /// The XMP pull parser.
    Metadata,
    /// The COS document, which every group has.
    Syntax,
    /// `tinker-pdf-font`.
    Fonts,
    /// `tinker-pdf-color`.
    Colour,
}

/// The machinery a rule group may reach for, built lazily per group.
///
/// The design doc's requirement is that a syntax-only sweep over 2 907 files
/// "never parses a font program it does not need". That is a property of the
/// code rather than of a comment, so it is **counted**: every reach past the
/// COS document goes through [`Machinery::reach`], which records the ask
/// whether or not it yields anything, and a syntax-only validation of a
/// document that embeds a font must leave the font and colour counters at
/// zero.
///
/// The counter is not vacuous. The metadata group reaches for the XML parser
/// on every packet, so the mechanism is exercised on nearly every document in
/// the corpus, and the unit tests assert both directions: the metadata reach
/// happens and the font reach does not.
#[derive(Debug, Default)]
pub(crate) struct Machinery {
    groups: Coverage,
    metadata: Cell<u32>,
    fonts: Cell<u32>,
    colour: Cell<u32>,
}

impl Machinery {
    fn new(groups: Coverage) -> Machinery {
        Machinery {
            groups,
            ..Machinery::default()
        }
    }

    /// Records a reach for `group`'s machinery and says whether it is there.
    ///
    /// A rule calls this *before* building anything, so a rule in a group that
    /// was not asked for costs the ask and nothing else.
    pub(crate) fn reach(&self, group: RuleGroup) -> bool {
        let (counter, enabled) = match group {
            RuleGroup::Metadata => (&self.metadata, self.groups.metadata),
            RuleGroup::Syntax => return self.groups.syntax,
            RuleGroup::Fonts => (&self.fonts, self.groups.fonts),
            RuleGroup::Colour => (&self.colour, self.groups.colour),
        };
        counter.set(counter.get().saturating_add(1));
        enabled
    }

    /// How many times each group's machinery was reached for.
    fn reaches(&self) -> (u32, u32, u32) {
        (self.metadata.get(), self.fonts.get(), self.colour.get())
    }
}

/// Which part of ISO 19005 a file claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// ISO 19005-1, on PDF 1.4.
    One,
    /// ISO 19005-2, on PDF 1.7.
    Two,
    /// ISO 19005-3, which is part 2 plus arbitrary embedded files.
    Three,
    /// ISO 19005-4, on PDF 2.0. It has no conformance levels.
    Four,
}

impl Part {
    /// The `pdfaid:part` value.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Part::One => 1,
            Part::Two => 2,
            Part::Three => 3,
            Part::Four => 4,
        }
    }

    fn from_number(value: i64) -> Option<Part> {
        match value {
            1 => Some(Part::One),
            2 => Some(Part::Two),
            3 => Some(Part::Three),
            4 => Some(Part::Four),
            _ => None,
        }
    }

    /// Whether this part defines the level.
    ///
    /// Parts 1 to 3 have the accessibility and basic levels, parts 2 and 3 add
    /// Unicode, and **part 4 has its own two**: `E` for engineering and `F`
    /// for embedded files. Part 4 is also the only one where declaring no
    /// level at all is correct — plain PDF/A-4 is a flavour.
    ///
    /// This table was wrong when it was written. It said part 4 had no levels,
    /// on a reading of the part's name rather than of its clauses, and the
    /// corpus census disagreed immediately: 16 files declare `E` and 10
    /// declare `F`, in directories the veraPDF suite calls `PDF_A-4e` and
    /// `PDF_A-4f`.
    #[must_use]
    pub fn allows(self, level: Level) -> bool {
        matches!(
            (self, level),
            (Part::One, Level::A | Level::B)
                | (Part::Two | Part::Three, Level::A | Level::B | Level::U)
                | (Part::Four, Level::E | Level::F)
        )
    }

    /// Whether omitting the level is correct for this part.
    #[must_use]
    pub fn level_optional(self) -> bool {
        self == Part::Four
    }
}

/// The conformance level, for the parts that have one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Accessible: level B plus a tagged structure tree and Unicode mapping.
    A,
    /// Basic: the file renders the same way everywhere.
    B,
    /// Unicode: level B plus every glyph mapped to Unicode. Parts 2 and 3.
    U,
    /// Engineering: part 4 only, permitting 3D and rich media.
    E,
    /// Embedded files: part 4 only, permitting arbitrary attachments.
    F,
}

impl Level {
    /// The `pdfaid:conformance` letter.
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Level::A => 'A',
            Level::B => 'B',
            Level::U => 'U',
            Level::E => 'E',
            Level::F => 'F',
        }
    }

    fn from_letter(text: &str) -> Option<Level> {
        match text.trim() {
            "A" | "a" => Some(Level::A),
            "B" | "b" => Some(Level::B),
            "U" | "u" => Some(Level::U),
            "E" | "e" => Some(Level::E),
            "F" | "f" => Some(Level::F),
            _ => None,
        }
    }
}

/// What a file claims to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flavour {
    /// The part.
    pub part: Part,
    /// The level, absent for part 4 and for a part 1-to-3 file that omitted
    /// it — [`FindingKind::LevelMissing`] is what distinguishes the two.
    pub level: Option<Level>,
}

impl core::fmt::Display for Flavour {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.level {
            Some(level) => write!(f, "PDF/A-{}{}", self.part.number(), level.letter()),
            None => write!(f, "PDF/A-{}", self.part.number()),
        }
    }
}

/// The clause a finding is about.
///
/// A string rather than an enum because the clause numbering differs between
/// parts for the same rule — a file-structure rule is 6.1.x in parts 1 to 3
/// and elsewhere in part 4 — so the number is data about the flavour being
/// checked rather than a variant of the check.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Clause(pub String);

impl core::fmt::Display for Clause {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What was wrong.
///
/// Closed, like `WarningKind`: a caller can match every variant and a new one
/// is a compile error where it is handled rather than a string nobody reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// No XMP packet at all, so there is no claim to check.
    MetadataMissing,
    /// An XMP packet with no `pdfaid:part`.
    NoFlavourClaimed,
    /// `pdfaid:part` names a part ISO 19005 does not define.
    PartUnknown {
        /// What the file said.
        declared: String,
    },
    /// `pdfaid:conformance` names a letter the part does not define.
    LevelUnknown {
        /// What the file said.
        declared: String,
    },
    /// A part 1-to-3 file with no conformance level.
    LevelMissing,
    /// A part 4 file with no `pdfaid:rev`.
    ///
    /// ISO 19005-4 identifies its amendment by a four-digit year alongside the
    /// part, which parts 1 to 3 have no equivalent of. A part 4 file without
    /// one has not said which PDF/A-4 it is.
    RevisionMissing,
    /// A `pdfaid:rev` that is not the four-digit year the part asks for.
    RevisionMalformed {
        /// What the file said.
        declared: String,
    },
    /// A conformance level that exists, on a part that does not define it —
    /// `U` on part 1, or `B` on part 4.
    LevelNotInPart {
        /// What the file said.
        declared: String,
    },
    /// The XMP packet is there and will not parse.
    MetadataUnreadable,
    /// The document is encrypted. Every part of ISO 19005 forbids it: a file
    /// nobody can open without a key is not archival.
    Encrypted,

    // ---- the syntax group (milestone 2) ----------------------------------
    //
    // Each of these cites the ISO 19005 clause it comes from in
    // `pdfa/syntax.rs`, beside the reading of that clause it implements.
    /// No `%PDF-` header anywhere near the front of the file (6.1.2).
    HeaderMissing,
    /// The header is present but not at byte zero (6.1.2).
    HeaderNotAtStart {
        /// Where it actually starts.
        at: u64,
    },
    /// The header names a version the claimed part does not admit (6.1.2).
    HeaderVersionNotInPart {
        /// What the header said.
        declared: String,
    },
    /// The header line does not end at a single EOL marker — trailing spaces,
    /// or a blank line before the comment (6.1.2).
    HeaderNotFollowedBySingleEol,
    /// No comment line after the header line (6.1.2).
    HeaderCommentMissing,
    /// The comment after the header does not begin with four bytes above 127,
    /// so a transfer program sniffing the file calls it text (6.1.2).
    HeaderCommentNotBinary,
    /// The trailer has no `/ID` (6.1.3).
    FileIdentifierMissing,
    /// The trailer's `/ID` is not the two strings ISO 32000-1 14.4 defines
    /// (6.1.3).
    FileIdentifierMalformed,
    /// A stream whose data lives in another file (6.1.7).
    ExternalStream {
        /// Which of `/F`, `/FFilter`, `/FDecodeParms` was there.
        key: String,
    },
    /// A filter the part forbids by name — `LZWDecode` (6.1.10).
    FilterForbidden {
        /// The filter, as the file spelled it.
        filter: String,
    },
    /// A filter outside the set ISO 32000-2 defines, in a part that admits
    /// only those (6.1.6).
    FilterNotStandard {
        /// The filter, as the file spelled it.
        filter: String,
    },
    /// A `/Crypt` filter naming something other than `/Identity` (6.1.10).
    CryptFilterNotIdentity,
    /// An action of a type the part forbids (6.6.1 / 6.5.1).
    ActionForbidden {
        /// The `/S` value.
        action: String,
    },
    /// A `/Named` action outside the four page-navigation ones (6.6.1).
    NamedActionForbidden {
        /// The `/N` value, empty when there was none.
        name: String,
    },
    /// An additional-actions dictionary (6.6.2 / 6.5.2).
    TriggerEventsForbidden,
    /// An embedded file in a part that admits none (6.1.11).
    EmbeddedFileForbidden,
    /// A file specification without a key its part requires (6.8 / 6.9).
    EmbeddedFileKeyMissing {
        /// Which key.
        key: String,
    },
    /// The catalog's `/Perms` carries a key the part does not admit (6.1.12).
    PermissionsEntryForbidden {
        /// Which key.
        key: String,
    },
    /// Optional content in a part that forbids it (6.1.13).
    OptionalContentForbidden,
    /// An XFA form (6.9 / 6.4).
    XfaForbidden,
    /// A catalog asking a reader to render an XFA form (6.9 / 6.4).
    NeedsRenderingForbidden,
    /// Part 4: a document information dictionary with no `/PieceInfo` to
    /// justify it (6.1.3).
    InfoDictionaryForbidden,
    /// Part 4: an `/Info` entry other than `/ModDate` (6.1.3).
    InfoEntryForbidden {
        /// Which entry.
        key: String,
    },
    /// Part 4: the catalog's `/Version` is not `2.n` (6.1.12).
    CatalogVersionMalformed {
        /// What the catalog said.
        declared: String,
    },

    // ---- the metadata group (milestone 3) --------------------------------
    /// An `/Info` entry that the XMP packet does not carry, or carries with a
    /// different value (6.7.3 in part 1, 6.1.5 in parts 2 and 3).
    InfoXmpMismatch {
        /// The `/Info` key, which names its XMP property in the clause table.
        key: String,
    },

    // ---- the font group (milestone 5) ------------------------------------
    //
    // Each cites the clause it comes from in `pdfa/fonts.rs`, beside this
    // build's reading of that clause.
    /// A font with no embedded program. The standard 14 are not an exception:
    /// ISO 19005 has no such list (6.3.4 / 6.2.11.4.1).
    FontNotEmbedded {
        /// The font dictionary's `/Subtype`, so a caller can tell a missing
        /// `/FontFile2` from a missing `/FontFile`.
        subtype: String,
    },
    /// The descriptor carries a font-file key the font's `/Subtype` does not
    /// admit, or a `/FontFile3` whose own `/Subtype` is outside the set
    /// (6.3.4 / 6.2.11.4.1).
    FontProgramSubtypeMismatch {
        /// Which font-file key.
        key: String,
        /// What was declared, either by the font or by the program stream.
        declared: String,
    },
    /// The embedded program will not parse as the format its key declares
    /// (6.3.4 / 6.2.11.4.1).
    FontProgramUnreadable {
        /// Which font-file key.
        key: String,
    },
    /// A subset tag that is not six upper-case letters and a `+`
    /// (6.3.5 / 6.2.11.4.2).
    SubsetTagMalformed {
        /// The `/BaseFont` name as the file spelled it.
        declared: String,
    },
    /// No `/ToUnicode`, where the level requires one (6.3.8 / 6.2.11.7).
    ToUnicodeMissing,
    /// A symbolic TrueType font carrying an `/Encoding` (6.3.7 / 6.2.11.6).
    SymbolicFontHasEncoding,
    /// A non-symbolic TrueType font whose `/Encoding` is outside the two the
    /// clause admits (6.3.7 / 6.2.11.6).
    EncodingNotStandard {
        /// What the font said.
        declared: String,
    },
    /// An `/Encoding` dictionary carrying `/Differences` where the part
    /// forbids it (6.3.7).
    EncodingDifferencesForbidden,
    /// A CIDFont without a complete `/CIDSystemInfo` (6.3.3.2 / 6.2.11.3.2).
    CidSystemInfoIncomplete {
        /// Which key was missing or of the wrong type; `CIDSystemInfo` when
        /// the dictionary itself was absent.
        key: String,
    },
    /// A `/CIDFontType2` whose `/CIDToGIDMap` is neither `/Identity` nor a
    /// stream (6.3.3.2 / 6.2.11.3.2).
    CidToGidMapMalformed {
        /// What the font said, empty when the entry was absent.
        declared: String,
    },

    // ---- the colour group (milestone 5) ----------------------------------
    /// `/OutputIntents` is not an array of dictionaries, or an entry is
    /// missing a key its clause requires (6.2.2 / 6.2.3).
    OutputIntentMalformed {
        /// Which key, or `OutputIntents` for the array itself.
        key: String,
    },
    /// A `GTS_PDFA1` output intent with no `/DestOutputProfile`
    /// (6.2.2 / 6.2.3).
    DestOutputProfileMissing,
    /// Two `GTS_PDFA1` output intents naming different destination profiles
    /// (6.2.2 / 6.2.3).
    OutputIntentsDisagree,
    /// A device colour space used in a file with no PDF/A output intent and no
    /// `/Default…` space standing in for it (6.2.3.3 / 6.2.4.3).
    DeviceColourWithoutOutputIntent {
        /// `DeviceGray`, `DeviceRGB` or `DeviceCMYK`.
        space: String,
    },
    /// A device colour space used under an output intent whose destination
    /// profile is for a different kind of device (6.2.3.3 / 6.2.4.3).
    DeviceColourNotInOutputIntent {
        /// `DeviceRGB` or `DeviceCMYK`.
        space: String,
        /// The destination profile's data colour space, as its ICC signature.
        profile: String,
    },
    /// An `ICCBased` stream whose `/N` is absent, or disagrees with the number
    /// of channels its profile declares (6.2.3.2 / 6.2.4.2).
    IccStreamMalformed {
        /// What was wrong, as the key it is about.
        key: String,
    },
    /// A `/RenderingIntent` outside the four ISO 32000-1 8.6.5.8 defines
    /// (6.2.9 / 6.2.6).
    RenderingIntentUnknown {
        /// What the file said.
        declared: String,
    },
    /// Transparency in a part 1 file, which forbids it outright (6.4).
    TransparencyForbidden {
        /// Which construct: the group, the soft mask, the blend mode or the
        /// constant alpha.
        feature: String,
    },
}

/// One thing wrong with the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformanceFinding {
    /// Which clause.
    pub clause: Clause,
    /// The object it is about, where there is one.
    pub object: Option<ObjRef>,
    /// What was wrong.
    pub kind: FindingKind,
}

impl core::fmt::Display for ConformanceFinding {
    /// `6.1.2 object 12 0: <what was wrong>`, or the same without the object.
    ///
    /// The clause comes first because it is what a reader looks for: a person
    /// reading a list of findings is checking them against a standard, and the
    /// standard is organised by clause. The object is omitted rather than
    /// printed as `none` when there is none, because a rule about the file as
    /// a whole has no object and saying so on every line is noise.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.clause)?;
        if let Some(object) = self.object {
            write!(f, " object {} {}", object.num, object.gen)?;
        }
        write!(f, ": {:?}", self.kind)
    }
}

/// Which rule groups a verdict actually ran.
///
/// Not decoration. A verdict with no findings from a validator that checked
/// one group of four means "nothing in that group was broken", and a caller
/// told only "no findings" would read it as "conforms". This is the difference,
/// made impossible to ignore by being in the same struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Coverage {
    /// The flavour claim and the XMP packet around it.
    pub metadata: bool,
    /// File structure: encryption, version, filters, actions, annotations.
    pub syntax: bool,
    /// Fonts: embedding, widths, Unicode mapping.
    pub fonts: bool,
    /// Colour: output intents and device spaces.
    pub colour: bool,
}

impl core::fmt::Display for Coverage {
    /// The groups that ran, comma-separated, or `nothing`.
    ///
    /// Named groups rather than a count, because "3 of 4 groups" does not tell
    /// a caller *which* rules a clean verdict is silent about — and that is the
    /// whole reason this type exists.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut first = true;
        for (ran, name) in [
            (self.metadata, "metadata"),
            (self.syntax, "syntax"),
            (self.fonts, "fonts"),
            (self.colour, "colour"),
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

impl Coverage {
    /// Every group this build has rules for.
    ///
    /// The default `Document::validate_pdfa` request. Not `ALL`: asking for
    /// a group with no rules and being told it ran is the one answer a
    /// verdict must never give.
    pub const IMPLEMENTED: Coverage = Coverage {
        metadata: true,
        syntax: true,
        fonts: true,
        colour: true,
    };

    /// The font group alone.
    ///
    /// The expensive sweep, and the one the laziness counter exists to keep
    /// out of the others: asking for this is asking for every embedded font
    /// program in the file to be parsed.
    pub const FONTS: Coverage = Coverage {
        metadata: false,
        syntax: false,
        fonts: true,
        colour: false,
    };

    /// The syntax group alone.
    ///
    /// The sweep the design doc requires to be cheap: no font program parsed,
    /// no ICC profile read, nothing but the COS document and the rules that
    /// need only it.
    ///
    /// The **flavour claim is still read**, and that is not an exception to
    /// the rule so much as a statement of what the rule is about. Which part
    /// a file claims decides which syntax rules apply and what clause number
    /// each finding cites, so a syntax sweep that skipped it would be
    /// enforcing no particular standard. It costs one pull-parse of a packet
    /// measured in kilobytes; a font program is what the requirement is
    /// about, and the counter behind [`Machinery`] is what proves the
    /// difference rather than asserting it.
    pub const SYNTAX: Coverage = Coverage {
        metadata: false,
        syntax: true,
        fonts: false,
        colour: false,
    };

    /// The metadata group alone.
    pub const METADATA: Coverage = Coverage {
        metadata: true,
        syntax: false,
        fonts: false,
        colour: false,
    };

    /// Whether every group ran, which is the only state in which an empty
    /// finding list means the file conforms.
    #[must_use]
    pub fn is_complete(self) -> bool {
        self.metadata && self.syntax && self.fonts && self.colour
    }
}

/// What the validator made of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// What the file claims to be, when it claims anything.
    pub flavour: Option<Flavour>,
    /// Everything wrong that this build looked for.
    pub findings: Vec<ConformanceFinding>,
    /// Which rule groups ran.
    pub coverage: Coverage,
}

impl Verdict {
    /// Whether this build found anything wrong.
    ///
    /// **Not** "the file conforms" — see [`Verdict::coverage`]. Named for what
    /// it is so that a caller reaching for a boolean gets the honest one.
    #[must_use]
    pub fn found_nothing(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Validates `document` against the flavour it claims, running `groups`.
///
/// The flavour is read whatever groups were asked for, because every other
/// group numbers its clauses by the part and a rule that does not know which
/// standard it is enforcing is not enforcing one. Only the findings *about the
/// claim itself* belong to the metadata group.
pub(crate) fn validate(document: &Document, groups: Coverage) -> Verdict {
    validate_counting(document, groups).0
}

/// [`validate`], also returning what each group's machinery was reached for.
///
/// The counts are the design doc's laziness requirement made checkable: a
/// syntax-only sweep must leave the font and colour counters at zero, and the
/// unit tests assert it on a document that embeds a font program. Not exposed
/// on the facade — a caller has no use for it and [`Coverage`] is the answer
/// to the question they do ask.
pub(crate) fn validate_counting(
    document: &Document,
    groups: Coverage,
) -> (Verdict, (u32, u32, u32)) {
    let machinery = Machinery::new(groups);
    let mut raw: Vec<Raw> = Vec::new();

    // The claim is read whatever was asked for, and the ask is counted: it is
    // an XML parse of a small packet, and a counter that did not record it
    // would be telling a comfortable story rather than what happened.
    machinery.reach(RuleGroup::Metadata);
    let mut claim = Vec::new();
    let flavour = flavour_of(document, &mut claim);
    if groups.metadata {
        raw.append(&mut claim);
    }

    // 6.1.3 in parts 1 to 3, 6.1.2 in part 4: an encrypted file is not
    // archival, whatever else is right about it. Reported before any other
    // rule because a file nobody can open without a key is the one finding
    // worth having, and checked here rather than in `syntax` because it needs
    // no machinery at all.
    if groups.syntax && document.is_encrypted() {
        raw.push(Raw::file(clauses::ENCRYPTION, FindingKind::Encrypted));
    }
    syntax::rules(&document.inner, &machinery, flavour, &mut raw);
    // Guarded at the call site rather than inside, and that is the laziness
    // requirement rather than a style: [`Machinery::reach`] records the *ask*
    // whether or not it is granted, so a group whose entry point is called
    // unconditionally would move its counter on every syntax-only sweep in the
    // corpus. The metadata group is called the same way for the same reason.
    //
    // Injection, counted: dropping this `if` fails 1 of the workspace's 3 581
    // tests, `a_syntax_only_sweep_never_reaches_for_a_font_program`, which is
    // the only test that reads the counter for a group it did not ask for.
    if groups.fonts {
        fonts::rules(&document.inner, &machinery, flavour, &mut raw);
    }
    if groups.colour {
        colour::rules(&document.inner, &machinery, flavour, &mut raw);
    }
    if groups.metadata {
        xmp::rules(document, &machinery, flavour, &mut raw);
    }

    let part = flavour.map(|f| f.part);
    let verdict = Verdict {
        flavour,
        findings: raw.into_iter().map(|r| r.resolve(part)).collect(),
        coverage: Coverage {
            metadata: groups.metadata,
            syntax: groups.syntax,
            fonts: groups.fonts,
            colour: groups.colour,
        },
    };
    (verdict, machinery.reaches())
}

/// Reads `pdfaid:part` and `pdfaid:conformance` out of the XMP packet.
fn flavour_of(document: &Document, findings: &mut Vec<Raw>) -> Option<Flavour> {
    let Some(packet) = document.xmp_metadata() else {
        findings.push(Raw::file(clauses::METADATA, FindingKind::MetadataMissing));
        return None;
    };

    let Some((part_text, level_text, revision_text)) = pdfaid(&packet) else {
        findings.push(Raw::file(
            clauses::FLAVOUR_ID,
            if packet.is_empty() {
                FindingKind::MetadataMissing
            } else {
                FindingKind::NoFlavourClaimed
            },
        ));
        return None;
    };

    let Some(part) = part_text
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(Part::from_number)
    else {
        findings.push(Raw::file(
            clauses::FLAVOUR_ID,
            FindingKind::PartUnknown {
                declared: part_text,
            },
        ));
        return None;
    };

    let level = match level_text {
        Some(text) => match Level::from_letter(&text) {
            Some(level) if part.allows(level) => Some(level),
            Some(_) => {
                findings.push(Raw::file(
                    clauses::FLAVOUR_ID,
                    FindingKind::LevelNotInPart { declared: text },
                ));
                None
            }
            None => {
                findings.push(Raw::file(
                    clauses::FLAVOUR_ID,
                    FindingKind::LevelUnknown { declared: text },
                ));
                None
            }
        },
        None if part.level_optional() => None,
        None => {
            findings.push(Raw::file(clauses::FLAVOUR_ID, FindingKind::LevelMissing));
            None
        }
    };

    // ISO 19005-4 6.7.3 identifies the amendment as well as the part:
    // `pdfaid:rev` is a four-digit year, and parts 1 to 3 have no equivalent
    // of it. A part 4 file that omits it, or writes something that is not a
    // year, has not finished saying which standard it claims.
    if part == Part::Four {
        match revision_text {
            Some(text)
                if text.trim().len() == 4 && text.trim().bytes().all(|b| b.is_ascii_digit()) => {}
            Some(text) => findings.push(Raw::file(
                clauses::FLAVOUR_ID,
                FindingKind::RevisionMalformed { declared: text },
            )),
            None => findings.push(Raw::file(clauses::FLAVOUR_ID, FindingKind::RevisionMissing)),
        }
    }

    Some(Flavour { part, level })
}

/// `pdfaid:part` and `pdfaid:conformance`, wherever RDF put them.
///
/// XMP is RDF/XML and RDF admits the same statement as an attribute or as a
/// child element — `<rdf:Description pdfaid:part="2">` and
/// `<rdf:Description><pdfaid:part>2</pdfaid:part></rdf:Description>` say the
/// same thing. Both forms are in the corpus, so both are read; a reader that
/// handled one would report thousands of files as claiming nothing.
///
/// # The namespace check, and why the first attempt was wrong
///
/// This matched on the **local name** alone, on the argument that real files
/// bind the `pdfaid` prefix in a parent element the packet does not always
/// carry, and that "nothing else in an XMP packet is called `part`".
///
/// The corpus census disagreed within a minute: **PDF/UA declares
/// `pdfuaid:part`**, an entirely different standard's version number in an
/// entirely different namespace, and 434 of the corpus's files were read as
/// claiming a PDF/A part they say nothing about. The local name was never
/// distinctive; it only looked that way from inside PDF/A.
///
/// So the prefix must be `pdfaid`, **or** the resolved namespace must be
/// `http://www.aiim.org/pdfa/ns/id/`. Either alone is too strict — a packet
/// that binds the URI to some other prefix is legal RDF, and a packet that
/// uses the conventional prefix without a visible binding is what real files
/// do — and together they reject `pdfuaid` while accepting both.
const PDFA_ID_NAMESPACE: &str = "http://www.aiim.org/pdfa/ns/id/";

/// Whether a name is in the PDF/A identification schema.
fn is_pdfaid(name: &tinker_pdf_xml::Name<'_>) -> bool {
    name.prefix() == Some("pdfaid") || name.namespace() == Some(PDFA_ID_NAMESPACE)
}

/// An XMP packet's bytes, in an encoding the XML reader can take.
///
/// # Why this exists, and it is a bug fix rather than a nicety
///
/// XMP (ISO 16684-1) permits a packet in UTF-8, UTF-16 **or UTF-32**, in
/// either byte order, with or without a byte order mark. `tinker-pdf-xml`
/// decodes UTF-8 and UTF-16, which is everything XML 1.0 requires of a
/// conforming processor and is therefore the right line for a general XML
/// reader to draw. It is the wrong line here: a PDF/A file whose packet is
/// UTF-32 conforms, and reading it as unparseable reported eight conforming
/// files in the veraPDF corpus as broken metadata.
///
/// So the transcode happens in the facade, where the bytes are known to be an
/// XMP packet, rather than in the leaf, where they are known only to be XML.
/// Ruling 8's line — format semantics stay in the facade — is the same line.
///
/// Returns the packet unchanged when it is not UTF-32, which is nearly always.
fn readable(packet: &[u8]) -> Cow<'_, [u8]> {
    match utf32_endianness(packet) {
        Some(big_endian) => Cow::Owned(from_utf32(packet, big_endian)),
        None => Cow::Borrowed(packet),
    }
}

/// Whether `packet` is UTF-32, and if so whether it is big-endian.
///
/// The byte order marks come first, because `FF FE 00 00` is a UTF-32LE mark
/// and its first two bytes are a *UTF-16LE* mark — a reader that checked two
/// bytes would decode a UTF-32 packet as UTF-16 and produce a string of NULs.
/// Without a mark, XMP's own rule is used: the first character of a packet is
/// `<`, so the null padding around it says the width and the order.
fn utf32_endianness(packet: &[u8]) -> Option<bool> {
    match packet.get(..4)? {
        [0x00, 0x00, 0xFE, 0xFF] => Some(true),
        [0xFF, 0xFE, 0x00, 0x00] => Some(false),
        [0x00, 0x00, 0x00, b] if *b != 0 => Some(true),
        [b, 0x00, 0x00, 0x00] if *b != 0 => Some(false),
        _ => None,
    }
}

/// Transcodes UTF-32 to UTF-8, dropping the byte order mark and anything that
/// is not a scalar value.
///
/// A code unit outside Unicode, or a surrogate, becomes nothing rather than a
/// replacement character: the packet is about to be parsed as XML, and a
/// replacement character inside an element name would turn a decoding problem
/// into a parse error that named the wrong thing.
fn from_utf32(packet: &[u8], big_endian: bool) -> Vec<u8> {
    let mut out = String::with_capacity(packet.len() / 4);
    for unit in packet.chunks_exact(4) {
        let bytes = [unit[0], unit[1], unit[2], unit[3]];
        let value = if big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        };
        // U+FEFF is the mark, which XML does not want to see as content.
        if value == 0xFEFF {
            continue;
        }
        if let Some(ch) = char::from_u32(value) {
            out.push(ch);
        }
    }
    out.into_bytes()
}

fn pdfaid(packet: &[u8]) -> Option<(String, Option<String>, Option<String>)> {
    let packet = readable(packet);
    let source = Source::new(&packet).ok()?;
    // An XMP packet carries no doctype and has no business carrying one, so
    // the strict mode is right here — unlike an XHTML content document, which
    // is why `Doctype::SkipExternalId` exists elsewhere in this crate.
    let limits = tinker_pdf_xml::Limits::default();
    let reader = source.reader(&limits);

    let (mut part, mut level, mut revision) = (None, None, None);
    // Which element's text is being collected, if any.
    let mut collecting: Option<&'static str> = None;

    for event in reader {
        let Ok(event) = event else {
            break;
        };
        match event {
            Event::Start(element) => {
                for attribute in element.attributes() {
                    if !is_pdfaid(attribute.name()) {
                        continue;
                    }
                    match attribute.name().local() {
                        "part" if part.is_none() => part = Some(attribute.value().to_string()),
                        "conformance" if level.is_none() => {
                            level = Some(attribute.value().to_string());
                        }
                        "rev" if revision.is_none() => {
                            revision = Some(attribute.value().to_string());
                        }
                        _ => {}
                    }
                }
                collecting = if is_pdfaid(element.name()) {
                    match element.local() {
                        "part" if part.is_none() => Some("part"),
                        "conformance" if level.is_none() => Some("conformance"),
                        "rev" if revision.is_none() => Some("rev"),
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
                    Some("part") => part = Some(trimmed.to_string()),
                    Some("conformance") => level = Some(trimmed.to_string()),
                    Some("rev") => revision = Some(trimmed.to_string()),
                    _ => {}
                }
            }
            Event::End(_) => collecting = None,
            _ => {}
        }
    }

    part.map(|part| (part, level, revision))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(packet: &str) -> Option<(String, Option<String>)> {
        pdfaid(packet.as_bytes()).map(|(part, level, _)| (part, level))
    }

    #[test]
    fn the_attribute_form_and_the_element_form_say_the_same_thing() {
        let attributes = read(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r">
               <rdf:Description xmlns:pdfaid="p" pdfaid:part="2"
                                pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta>"#,
        );
        assert_eq!(attributes, Some(("2".into(), Some("B".into()))));

        let elements = read(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r">
               <rdf:Description xmlns:pdfaid="p"><pdfaid:part>2</pdfaid:part>
               <pdfaid:conformance>B</pdfaid:conformance></rdf:Description>
               </rdf:RDF></x:xmpmeta>"#,
        );
        assert_eq!(elements, attributes, "RDF admits both and both are read");
    }

    #[test]
    fn a_packet_with_no_claim_reads_as_no_claim_rather_than_as_a_guess() {
        assert_eq!(
            read(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="r"/></x:xmpmeta>"#),
            None
        );
    }

    #[test]
    fn markup_that_will_not_parse_yields_nothing_rather_than_half_an_answer() {
        assert_eq!(read("<x:xmpmeta><unclosed>"), None);
        assert_eq!(read(""), None);
        assert_eq!(pdfaid(&[0xFF, 0xFE, 0x00]), None);
    }

    /// The level table, which was wrong when it was first written: it said
    /// part 4 had none, and the corpus census found 26 files declaring one.
    #[test]
    fn each_part_admits_exactly_the_levels_iso_19005_gives_it() {
        for (part, allowed) in [
            (Part::One, &[Level::A, Level::B][..]),
            (Part::Two, &[Level::A, Level::B, Level::U]),
            (Part::Three, &[Level::A, Level::B, Level::U]),
            (Part::Four, &[Level::E, Level::F]),
        ] {
            for level in [Level::A, Level::B, Level::U, Level::E, Level::F] {
                assert_eq!(
                    part.allows(level),
                    allowed.contains(&level),
                    "{part:?} and {level:?}"
                );
            }
        }
        // And part 4 is the only one where declaring none is correct.
        assert!(Part::Four.level_optional());
        for part in [Part::One, Part::Two, Part::Three] {
            assert!(!part.level_optional(), "{part:?}");
        }
    }

    #[test]
    fn a_flavour_renders_the_way_the_standard_names_it() {
        assert_eq!(
            Flavour {
                part: Part::Two,
                level: Some(Level::B)
            }
            .to_string(),
            "PDF/A-2B"
        );
        assert_eq!(
            Flavour {
                part: Part::Four,
                level: None
            }
            .to_string(),
            "PDF/A-4"
        );
    }

    #[test]
    fn every_part_number_round_trips() {
        for part in [Part::One, Part::Two, Part::Three, Part::Four] {
            assert_eq!(Part::from_number(i64::from(part.number())), Some(part));
        }
        // The two the corpus actually contains, and neither exists.
        assert_eq!(Part::from_number(9), None);
        assert_eq!(Part::from_number(0), None);
    }

    #[test]
    fn a_verdict_that_checked_one_group_does_not_claim_conformance() {
        let partial = Coverage {
            metadata: true,
            ..Coverage::default()
        };
        assert!(!partial.is_complete());
        assert!(Coverage {
            metadata: true,
            syntax: true,
            fonts: true,
            colour: true
        }
        .is_complete());
    }

    // ---- the laziness requirement, counted -------------------------------

    /// A document that embeds a font program, an ICC profile and an XMP
    /// packet, so a rule that reached for any of the three would have
    /// something to reach for.
    ///
    /// The font bytes are a real `sfnt` header — the four-byte tag, the table
    /// count and a directory entry — rather than filler, because a parser that
    /// rejected them at the first byte would not have been reached far enough
    /// to count as having been reached.
    fn document_with_a_font_program() -> crate::Document {
        let packet = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"
 pdfaid:part="2" pdfaid:conformance="B"/></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
        let sfnt: &[u8] = &[
            0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x03, 0x00, 0x04, b'h', b'e',
            b'a', b'd', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1C, 0x00, 0x00, 0x00, 0x36,
        ];

        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R >>".to_vec(),
            ),
            (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                   /Resources << /Font << /F1 5 0 R >> >> >>"
                    .to_vec(),
            ),
        ];
        let mut metadata = format!(
            "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
            packet.len()
        )
        .into_bytes();
        metadata.extend_from_slice(packet);
        metadata.extend_from_slice(b"\nendstream");
        objects.push((4, metadata));
        objects.push((
            5,
            b"<< /Type /Font /Subtype /TrueType /BaseFont /Acme \
               /FontDescriptor 6 0 R >>"
                .to_vec(),
        ));
        objects.push((
            6,
            b"<< /Type /FontDescriptor /FontName /Acme /Flags 4 /FontFile2 7 0 R >>".to_vec(),
        ));
        let mut program = format!(
            "<< /Length {} /Length1 {} >>\nstream\n",
            sfnt.len(),
            sfnt.len()
        )
        .into_bytes();
        program.extend_from_slice(sfnt);
        program.extend_from_slice(b"\nendstream");
        objects.push((7, program));

        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0u64; objects.len() + 1];
        for (num, body) in &objects {
            offsets[*num as usize] = out.len() as u64;
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len() as u64;
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for entry in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{entry:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n\
                 {xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        crate::Document::open(out).expect("the fixture opens")
    }

    /// The design doc's laziness requirement, as a measurement.
    ///
    /// *"Machinery is built lazily per group, so a syntax-only sweep over
    /// 2 907 files never parses a font program it does not need."* Every reach
    /// past the COS document is counted, so the requirement is the font and
    /// colour counters staying at zero on a document that has both a font
    /// program and a resource dictionary pointing at it.
    ///
    /// **Injection, counted.** A rule was added to `syntax::dictionary` that
    /// reaches for [`RuleGroup::Fonts`] on any dictionary carrying a
    /// `/FontFile2` and calls `tinker_pdf_font::Sfnt::parse`. It fails
    /// **four of the workspace's 3 375 tests** and no others: this one,
    /// [`the_full_request_reaches_no_further_than_the_short_one`],
    /// [`asking_for_a_group_with_no_rules_does_not_make_it_have_run`], and
    /// `pdfa_syntax.rs`'s
    /// `the_syntax_group_names_no_font_or_colour_machinery`, which catches the
    /// import rather than the call and so fires even for a reach that is never
    /// executed.
    ///
    /// The first attempt at that injection fired **nothing**, and the reason is
    /// worth keeping: it tested `is_stream && dict.contains_key(FontFile2)`,
    /// and a `/FontFile2` lives in the font *descriptor*, which is a plain
    /// dictionary. An injection that misses is not evidence the guard works —
    /// it is evidence the injection was wrong — and the difference is only
    /// visible because the count was taken rather than assumed.
    #[test]
    fn a_syntax_only_sweep_never_reaches_for_a_font_program() {
        let document = document_with_a_font_program();
        let (verdict, (metadata, fonts, colour)) = validate_counting(&document, Coverage::SYNTAX);

        assert_eq!(fonts, 0, "the syntax group parsed a font program");
        assert_eq!(colour, 0, "the syntax group read a colour profile");
        // Not vacuous: the counter does move, for the one parse a syntax sweep
        // genuinely needs — the flavour claim, which decides which rules apply.
        assert_eq!(metadata, 1, "the flavour claim is read, and counted");
        assert!(verdict.coverage.syntax);
        assert!(!verdict.coverage.fonts);
    }

    /// The full request reaches for each group's machinery exactly as often as
    /// it has rules to run, and no oftener.
    ///
    /// Milestone 5 changed this test's numbers and that is the point of having
    /// it: before the font group existed the font counter was zero here, and a
    /// counter that stayed at zero once the group landed would have meant the
    /// group was not running.
    #[test]
    fn the_full_request_reaches_each_group_it_asked_for_once() {
        let document = document_with_a_font_program();
        let (_, (metadata, fonts, colour)) = validate_counting(&document, Coverage::IMPLEMENTED);
        assert_eq!(
            metadata, 2,
            "once for the claim and once for the properties"
        );
        assert_eq!(fonts, 1, "the font group runs, and asks once");
        assert_eq!(colour, 1, "and so does the colour group");
    }

    /// Milestone 5 closed the last group, and this is what that means.
    ///
    /// The earlier version of this test asserted that a group *asked for* with
    /// no rules must not report itself as having run — first about fonts, then
    /// about colour. With four groups of four implemented that property has
    /// nothing left to be about, so it is replaced rather than deleted by the
    /// one that outlives it: the coverage a verdict reports is the request it
    /// was given, and a group's machinery is reached only where the request
    /// asked for it.
    ///
    /// This is also the first commit at which an empty finding list from
    /// [`Coverage::IMPLEMENTED`] can mean "this file conforms" rather than
    /// "nothing this build checks was broken", which is what
    /// [`Coverage::is_complete`] says and why it is asserted here.
    #[test]
    fn coverage_reports_the_request_and_the_counters_agree_with_it() {
        let document = document_with_a_font_program();
        assert!(Coverage::IMPLEMENTED.is_complete());
        let (verdict, _) = validate_counting(&document, Coverage::IMPLEMENTED);
        assert!(verdict.coverage.is_complete());

        // One group at a time: asking for fonts alone leaves the colour
        // counter at zero and does not claim colour ran.
        let (verdict, (_, fonts, colour)) = validate_counting(&document, Coverage::FONTS);
        assert_eq!((fonts, colour), (1, 0));
        assert!(verdict.coverage.fonts);
        assert!(!verdict.coverage.colour);
        assert!(!verdict.coverage.is_complete());
    }
}
