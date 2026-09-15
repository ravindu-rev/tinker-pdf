//! The predefined XMP schemas, **one table per revision of the XMP
//! specification**: which properties a revision defined, and what value type
//! each of them declares.
//!
//! These are the tables behind the value-type half of ISO 19005-1 6.7.2 and
//! ISO 19005-2 6.6.2.3 — the staged-rules row of `docs/ROADMAP.md`, whose exit
//! criterion is that the staged count moves down and the agreement ratchet up,
//! one ledger class at a time. They join the metadata rule group `pdfa/xmp.rs`
//! delivered at milestone 3 of `docs/design/pdfa.md`.
//!
//! # Which revision each part cites
//!
//! | part | revision | table |
//! | --- | --- | --- |
//! | ISO 19005-1 | XMP Specification, **January 2004** | [`PREDEFINED_2004`] |
//! | ISO 19005-2, ISO 19005-3 | XMP Specification, **September 2005** | [`PREDEFINED_2005`] |
//! | ISO 19005-4 | — | none; the part carries no such requirement |
//!
//! The evidence is the veraPDF conformance suite, and it comes from two
//! directions that cannot both be a coincidence. **Its fixtures state their
//! own expectation in their own words**: all 366 part-1 membership fixtures
//! say the property is or is not "in XMP 2004" and all 549 part-2 ones say
//! "in XMP 2005", with no counterexample either way. **Its machine-readable
//! profiles bind the revision to the part by name**: `PDFA-1B.xml` calls
//! `isPredefinedInXMP2004` and `PDFA-2B.xml` calls `isPredefinedInXMP2005`.
//!
//! **This file said something different until the commit that replaced its
//! table, and the mistake is traceable to one document.** PDF Association
//! TechNote 0008 is titled *Predefined XMP Properties in **PDF/A-1***. It says
//! the applicable revision is XMP 2004 and that this version "differs
//! significantly from earlier and later revisions", and both statements are
//! true — **of part 1**, which is the only part it is about. Read as though it
//! said "ISO 19005", it turned into a claim about parts 2 and 3 as well, and
//! that claim spread to five files. It was wrong in each of them, and the
//! corpus had already said so: see `photoshop:SupplementalCategories` below,
//! which is the disagreement a part-2 fixture reported one clause after the
//! generalisation was written down.
//!
//! # Provenance
//!
//! Two specifications, transcribed rather than vendored — the tabulated facts
//! (property name, namespace, preferred prefix, value type), not the
//! documents' text. Both are recorded under "The predefined XMP schemas'
//! property tables" in `THIRDPARTY.md`, with the byte count and SHA-256 of the
//! file each was read from.
//!
//! | table | document | chapter 4 "XMP Schemas" | schemas | properties |
//! | --- | --- | --- | ---: | ---: |
//! | [`PREDEFINED_2004`] | *XMP Specification*, January 2004, 94 pp | pp. 37–58 | 11 | 169 |
//! | [`PREDEFINED_2005`] | *XMP Specification*, September 2005, 112 pp | pp. 39–70 | 14 | 274 |
//!
//! Every row was read off the page it is printed on, and **read a second time
//! by two checks that do not share a failure mode with the first**:
//!
//! - **Against the revision beside it.** 168 properties are printed in both
//!   documents. 166 declare the same value type in both. The two that differ
//!   are `photoshop:SupplementalCategories`, which the September 2005
//!   changelog records changing (below), and `exif:GPSMeasureMode`, which
//!   January 2004 p. 57 prints as `Closed Choice of Integer` and September
//!   2005 p. 68 prints as `Text` — a difference neither changelog mentions and
//!   both pages state plainly.
//! - **Against the form these tables already carried.** The previous revision
//!   of this file recorded one of four serialisation *forms* per property.
//!   Collapsing each freshly read value type back to its form — `Lang Alt` to
//!   a language alternative, any of the three containers to an array, a named
//!   structure type to a structure, everything else to a simple value —
//!   reproduces **all 443** of them, and the property set is unchanged:
//!   the same 443 `(namespace, name)` pairs, neither added to nor removed
//!   from. So this revision moves the *type* column and nothing else, which is
//!   what keeps the membership half of the rule where it was.
//!
//! Two cells needed a second look for a reason the collapse cannot show,
//! because both reduce to the form the old table already carried:
//! `dc:relation`, whose row in both documents omits the Category cell every
//! other row has and which both print as `bag Text`; and `xmpMM:RenditionOf`,
//! whose cell reads `(deprecated) ResourceRef`. Neither is a disagreement —
//! they are the two places a reader is most likely to make one.
//!
//! The per-property page numbers stay out of the table because nothing in the
//! engine reads them.
//!
//! # What a value type is here, and what is done with it
//!
//! [`Shape`] is how the value is arrayed and [`Item`] is what one value is.
//! The split is not decoration: **the two halves are answered from different
//! input**, and that is the whole reason this file can now say more than it
//! used to.
//!
//! - A [`Shape`] is a statement about the RDF/XML serialisation — an
//!   `rdf:Bag`, an `rdf:Seq`, an `rdf:Alt`, a language alternative, or none of
//!   those — and is settled by the element names alone.
//! - An [`Item`] is a statement about the *text*, and judging it means reading
//!   the value against a published grammar. [`Item::judged`] is the list of
//!   the ones this build reads, and it is short on purpose: see
//!   [`Item::judged`]'s own documentation for which types are deliberately
//!   left alone and what the corpus says about each.
//!
//! # What these tables are, and the one thing they are not
//!
//! **Both are read as membership lists, and both are read for value types.**
//! ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 require every property to belong
//! to a predefined schema *or* be described by an extension schema, and these
//! tables are one half of that answer: the revision each part cites, as that
//! revision printed it. The other half is [`super::xmp_extension`], which
//! reads 6.7.8's extension schemas — a packet's own way of declaring a
//! property these tables cannot know. A membership rule that ran before that
//! exception was read would report every conforming file that uses one, which
//! is why the two landed in one commit and neither before the other.
//!
//! **What these tables are not is a list of what a conforming file may carry.**
//! Two namespaces ISO 19005 defines for itself appear in nearly every
//! conforming file and in no revision of the XMP specification: `pdfaid` and
//! the `pdfaExtension` vocabularies. `xmp_extension::is_an_iso_19005_namespace`
//! is where that is said, and the membership rule asks it first — a table
//! patched to carry those names would be a claim about a document that never
//! printed them.
//!
//! **And one name is a disagreement between two published sources.** The
//! suite's fixtures are themselves a list of membership claims — 422 distinct
//! ones across the two parts, each of the form *the property X, which is (not)
//! permitted in \<schema\> in XMP 2004/2005* — and **421 of the 422 agree with
//! these tables exactly**, schema by schema and name by name. The one that
//! does not is `xmpMM:InstanceID`, which `PDF_A-1b` `6-7-2-t09-fail-q` says is
//! permitted in XMP 2004: the string does not occur anywhere in the 94 pages
//! of the January 2004 document, and September 2005 introduces it with an
//! editorial marker its own author left in the file (`<< new InstanceID
//! stuff>>`, p45). **The table is still not patched.** `PDF_A-1b`
//! `6-7-2-t09-pass-q` writes the property and the suite annotates it
//! conforming, so a strict reading reports a conforming file — and the
//! membership rule therefore admits that one name under part 1, as a named
//! exception in `xmp::is_a_member` with the argument beside it. A row here
//! would be a claim about what the January 2004 document prints; the exception
//! there is a claim about what a conformance suite says, which is what it
//! actually is.
//!
//! **A second name is the same shape of disagreement, about a value rather
//! than a name.** September 2005 p. 41 prints `xmp:Rating` as `Closed Choice
//! of Integer`, and `PDF_A-2b` `6-6-2-3-1-t07-pass-m` writes `1.0` into it and
//! is annotated conforming, with no failing twin. The row below says what the
//! page says; [`Item::judged`] declines to read that one property's value, for
//! the same reason and with the same argument written beside it.
//!
//! # `photoshop:SupplementalCategories`, which is what the revisions are for
//!
//! One property in these tables has a different form under part 1 from under
//! parts 2 and 3, and it is worth the space because of what finding it cost.
//! The veraPDF suite pins it from both sides in *both* places it appears, and
//! the two places disagree:
//!
//! | fixture | value written | annotated |
//! | --- | --- | --- |
//! | `PDF_A-1b` `6-7-2-t13-pass-l` | text | conforming |
//! | `PDF_A-1b` `6-7-2-t13-fail-l` | `rdf:Bag` | non-conforming |
//! | `PDF_A-2b` `6-6-2-3-1-t13-pass-l` | `rdf:Bag` | conforming |
//! | `PDF_A-2b` `6-6-2-3-1-t13-fail-l` | `rdf:Seq` | non-conforming |
//!
//! Under one table for all parts that was a contradiction, and it was carried
//! as an override — a hand-written exception saying "the suite wins here, per
//! part", bolted beside a table that could not explain why. It is now an
//! ordinary row in each of two ordinary tables: `Text` on page 47 of January
//! 2004, `bag Text` on page 55 of September 2005. The September 2005
//! specification's own changelog records the change under April 2005 —
//! *"Corrected value type for photoshop:SupplementalCategories, changed 'Text'
//! to 'bag Text'"* — so the standards, the conformance suite and this file now
//! say the same thing for the same reason, and nothing here overrides
//! anything.
//!
//! The fourth row of that table is also the smallest example of what [`Shape`]
//! buys: `rdf:Seq` where `bag Text` is declared was, under one `Array` form
//! for all three containers, indistinguishable from the conforming spelling.

use super::Part;

/// How a property's value is arrayed.
///
/// This is the half of a value type an RDF/XML serialisation answers on its
/// own, and it is now the specification's three containers rather than one
/// `Array`: the suite has 35 fixtures whose only defect is an array written as
/// the wrong one of the three — 22 an `rdf:Bag` where `seq` is declared, 11 an
/// `rdf:Seq` where `bag` is, and 2 an `rdf:Seq` where `alt` is — and
/// collapsing the three made every one of them read as conforming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// Not an array: one value, written as an attribute, as element text, or
    /// as a structure when [`Item`] is one.
    One,
    /// `bag X` — an unordered array, serialised `rdf:Bag`.
    Bag,
    /// `seq X` — an ordered array, serialised `rdf:Seq`.
    Seq,
    /// `alt X` — an alternative array, serialised `rdf:Alt`.
    Alt,
    /// `Lang Alt` — an `rdf:Alt` of text whose items carry `xml:lang`.
    ///
    /// Separated from [`Shape::Alt`] because the corpus says to: reading a
    /// bare `rdf:Alt` as satisfying a Lang Alt agrees with **four fewer**
    /// corpus files — two under `PDF_A-1b/6.7 Metadata` and two under
    /// `PDF_A-2b/6.6 Metadata`, the bar 1 948 to 1 944 — and gains no
    /// conforming file, so an alternative array with no language on its items
    /// is a finding rather than a leniency. The
    /// converse is a leniency, and [`Written::satisfies`] says why.
    LangAlt,
}

/// What one value is, in the specification's own words.
///
/// Every variant is a value type one of the two documents prints in a Value
/// Type cell; none is a widening of several. The ones this build reads a value
/// against are [`Item::judged`], which is a short list with a measurement
/// behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    /// `Text` — "A Unicode string."
    Text,
    /// `ProperName` — a name of a person or organization.
    ProperName,
    /// `AgentName` — the name of a program.
    AgentName,
    /// `MIMEType` — a text value identifying a file format, RFC 2046.
    MimeType,
    /// `Locale` — a closed choice identifying a language, RFC 3066.
    Locale,
    /// `XPath` — an XML Path Language expression.
    XPath,
    /// `RenditionClass` — colon-separated tokens from an open choice.
    RenditionClass,
    /// `URI` — an Internet Uniform Resource Identifier.
    Uri,
    /// `URL` — an Internet Uniform Resource Locator.
    Url,
    /// `GPSCoordinate` — a latitude or longitude in one of two printed forms.
    GpsCoordinate,
    /// `Integer` — "an arbitrary length decimal numeric string with an
    /// optional leading `+` or `-` sign".
    Integer,
    /// `Real` — "a decimal numeric string with an optional single decimal
    /// point and an optional leading `+` or `-` sign".
    Real,
    /// `Rational` — two integers separated by a solidus.
    Rational,
    /// `Boolean` — "Allowed values are True or False".
    Boolean,
    /// `Date` — a subset of ISO 8601, in the six forms the specification
    /// prints.
    Date,
    /// `Closed Choice of Integer` — an integer from a list printed beside it.
    ChoiceOfInteger,
    /// `Closed Choice of Text` / `open Choice of Text` / `open Choice` — a
    /// string, from a list where the choice is closed.
    ChoiceOfText,
    /// `Seq of points (Integer, Integer)` — `crs:ToneCurve`'s items, which the
    /// specification describes rather than gives a lexis for.
    Point,
    /// One of the named structure types, carried by name so a finding can say
    /// which: `ResourceRef`, `Dimensions`, `Thumbnail` and the rest.
    Structure(&'static str),
}

/// What the packet actually wrote, which is what a [`Shape`] is compared
/// against.
///
/// A separate type from [`Shape`] because the two are not the same question: a
/// [`Shape`] is what the standard asks for and a [`Written`] is what a
/// serialisation can be observed to be, and `Structure` is observable without
/// being askable — the declaration for a structure is `Shape::One` with an
/// [`Item::Structure`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Written {
    /// An attribute, or an element whose content is text.
    Simple,
    /// `rdf:parseType="Resource"`, a nested `rdf:Description`, or the
    /// attribute shorthand.
    Structure,
    /// `rdf:Bag`.
    Bag,
    /// `rdf:Seq`.
    Seq,
    /// `rdf:Alt` whose items carry no `xml:lang`.
    Alt,
    /// `rdf:Alt` whose items carry `xml:lang`.
    LangAlt,
}

impl Written {
    /// Whether this serialisation satisfies a declared value type.
    ///
    /// Equality but for two asymmetries, each of which is a leniency and
    /// therefore has to earn its place:
    ///
    /// - A **structure** satisfies `Shape::One` when, and only when, the
    ///   declared item is one. `xmpDM:projectRef` written as a string is the
    ///   defect a fixture exists for, and so is a string property written as a
    ///   structure.
    /// - A **language alternative satisfies a declared `alt`**, because a
    ///   `Lang Alt` *is* an alternative array — the language qualifier is an
    ///   addition to it, not a different container. The converse is not true
    ///   and is not granted: a bare `rdf:Alt` where `Lang Alt` is declared is
    ///   a finding, which is what four corpus files say and no conforming
    ///   file contradicts.
    fn satisfies(self, shape: Shape, item: Item) -> bool {
        match shape {
            Shape::One => match self {
                Written::Simple => !matches!(item, Item::Structure(_)),
                Written::Structure => matches!(item, Item::Structure(_)),
                _ => false,
            },
            Shape::Bag => self == Written::Bag,
            Shape::Seq => self == Written::Seq,
            Shape::Alt => self == Written::Alt || self == Written::LangAlt,
            Shape::LangAlt => self == Written::LangAlt,
        }
    }

    /// How a finding names this serialisation.
    pub(super) fn describe(self) -> &'static str {
        match self {
            Written::Simple => "a simple value",
            Written::Structure => "a structure",
            Written::Bag => "an unordered array",
            Written::Seq => "an ordered array",
            Written::Alt => "an alternative array",
            Written::LangAlt => "a language alternative",
        }
    }
}

/// A value type a grammar is published for, and this build reads a value
/// against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Judged {
    /// September 2005 p. 77, January 2004 p. 63.
    Integer,
    /// September 2005 p. 77, January 2004 p. 63.
    Real,
    /// September 2005 p. 74, January 2004 p. 62.
    Boolean,
    /// September 2005 p. 75; January 2004 p. 62 by reference.
    Date,
}

impl Judged {
    /// Whether `text` is an instance of this type, by the grammar the
    /// specification prints for it.
    ///
    /// Every arm follows the printed lexis and nothing else. Where the
    /// document gives a range as well as a shape — the month, the day, the
    /// hour — the range is part of the grammar and is checked; where it gives
    /// only a shape, only the shape is.
    pub(super) fn admits(self, text: &str) -> bool {
        match self {
            // "The string consists of an arbitrary length decimal numeric
            // string with an optional leading "+" or "-" sign."
            Judged::Integer => {
                let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            }
            // "Consists of a decimal numeric string with an optional single
            // decimal point and an optional leading "+" or "-" sign."
            Judged::Real => {
                let body = text.strip_prefix(['+', '-']).unwrap_or(text);
                let mut halves = body.split('.');
                let whole = halves.next().unwrap_or_default();
                let fraction = halves.next().unwrap_or_default();
                halves.next().is_none()
                    && !(whole.is_empty() && fraction.is_empty())
                    && whole.bytes().all(|byte| byte.is_ascii_digit())
                    && fraction.bytes().all(|byte| byte.is_ascii_digit())
            }
            // "Allowed values are True or False (the strings should be
            // spelled exactly as shown)."
            Judged::Boolean => text == "True" || text == "False",
            Judged::Date => is_a_date(text),
        }
    }

    /// How a finding names the type, in the specification's own word.
    pub(super) fn describe(self) -> &'static str {
        match self {
            Judged::Integer => "an integer",
            Judged::Real => "a real",
            Judged::Boolean => "a boolean",
            Judged::Date => "a date",
        }
    }
}

/// Whether `text` is an XMP `Date`.
///
/// **The two revisions give the same grammar two ways, and this is the one
/// place a rule could have been built on only one of them.** September 2005
/// p. 75 prints it in full: *"A date-time value which is represented using a
/// subset of ISO RFC 8601 formatting, as described in
/// `http://www.w3.org/TR/NOTE-datetime`. The following formats are
/// supported"*, and then the six forms below. January 2004 p. 62 prints one
/// sentence — *"A date which is represented using ISO 8601 formatting, as
/// described in `http://www.w3.org/TR/NOTE-datetime`"* — and no forms at all,
/// so part 1's grammar is the W3C note itself. The note lists the same six
/// forms with the same field ranges, which is why one function serves both
/// parts; had it not, this would be two.
///
/// The forms:
///
/// ```text
/// YYYY
/// YYYY-MM
/// YYYY-MM-DD
/// YYYY-MM-DDThh:mmTZD
/// YYYY-MM-DDThh:mm:ssTZD
/// YYYY-MM-DDThh:mm:ss.sTZD
/// ```
///
/// with `s` "one or more digits representing a decimal fraction of a second"
/// and `TZD` "a time zone designator (Z or +hh:mm or -hh:mm)". The ranges
/// beside each field — month 01 through 12, day 01 through 31, hour 00 through
/// 23, minute and second 00 through 59 — are printed in the same list and are
/// checked with the shape.
///
/// **The time zone designator is not optional in the three forms that carry a
/// time**, and that is the one place where following the printed grammar could
/// cost a conforming file. It does not: of the 831 files the corpus annotates
/// conforming, the only ones carrying a time with no designator, or with a
/// two-digit one, are six under `PDF_A-4` and `PDF_A-4e` — and ISO 19005-4
/// carries no predefined-schema requirement, so the rule never runs there.
/// `part_carries_the_predefined_schema_rule` is where that is enforced, and it
/// is enforced for a different reason, which is why this note names the files.
fn is_a_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let field = |from: usize, to: usize, low: u32, high: u32| -> bool {
        let Some(slice) = text.get(from..to) else {
            return false;
        };
        slice.bytes().all(|byte| byte.is_ascii_digit())
            && slice
                .parse::<u32>()
                .is_ok_and(|n| (low..=high).contains(&n))
    };

    // YYYY
    if bytes.len() == 4 {
        return field(0, 4, 0, 9999);
    }
    // YYYY-MM
    if bytes.len() == 7 {
        return bytes[4] == b'-' && field(0, 4, 0, 9999) && field(5, 7, 1, 12);
    }
    if bytes.len() < 10 {
        return false;
    }
    let date = bytes[4] == b'-'
        && bytes[7] == b'-'
        && field(0, 4, 0, 9999)
        && field(5, 7, 1, 12)
        && field(8, 10, 1, 31);
    if !date {
        return false;
    }
    // YYYY-MM-DD
    if bytes.len() == 10 {
        return true;
    }
    if bytes[10] != b'T' {
        return false;
    }
    let rest = &text[11..];
    // The designator starts at the first `Z`, `+` or `-` after the `T`.
    let Some(at) = rest.find(['Z', '+', '-']) else {
        return false;
    };
    if !is_a_designator(&rest[at..]) {
        return false;
    }
    let clock = &rest[..at];
    let mut parts = clock.split(':');
    let (Some(hour), Some(minute)) = (parts.next(), parts.next()) else {
        return false;
    };
    if !two_digits(hour, 0, 23) || !two_digits(minute, 0, 59) {
        return false;
    }
    match parts.next() {
        // YYYY-MM-DDThh:mmTZD
        None => true,
        Some(second) => {
            parts.next().is_none()
                && match second.split_once('.') {
                    // YYYY-MM-DDThh:mm:ss.sTZD
                    Some((whole, fraction)) => {
                        two_digits(whole, 0, 59)
                            && !fraction.is_empty()
                            && fraction.bytes().all(|byte| byte.is_ascii_digit())
                    }
                    // YYYY-MM-DDThh:mm:ssTZD
                    None => two_digits(second, 0, 59),
                }
        }
    }
}

/// Whether `text` is `Z`, `+hh:mm` or `-hh:mm`.
fn is_a_designator(text: &str) -> bool {
    if text == "Z" {
        return true;
    }
    let Some(offset) = text.strip_prefix(['+', '-']) else {
        return false;
    };
    match offset.split_once(':') {
        Some((hour, minute)) => two_digits(hour, 0, 23) && two_digits(minute, 0, 59),
        None => false,
    }
}

/// Whether `text` is exactly two digits within `low..=high`.
fn two_digits(text: &str, low: u32, high: u32) -> bool {
    text.len() == 2
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text.parse::<u32>().is_ok_and(|n| (low..=high).contains(&n))
}

impl Item {
    /// The grammar this build reads a value against, or `None` for a type it
    /// deliberately does not.
    ///
    /// **The list is short because the corpus is what decided it.** Grouping
    /// all 168 fixtures that turned on this rule by what their value actually
    /// violates gives: 96 an integer that is not one, 35 an array of the wrong
    /// kind, 17 a date that is not one, 10 a boolean that is not one, 9 a real
    /// that is not one, and one `xmpMM:InstanceID`, a name no January 2004
    /// page prints. Nothing else.
    ///
    /// **What says the short list is long enough is what is left over.** The
    /// two clause directories this rule serves — `PDF_A-1b/6.7 Metadata/6.7.2
    /// Properties` and `PDF_A-2b/…/6.6.2.3.1 General` — hold 468 `-fail-`
    /// fixtures between them. With the four types below read, **467 of the 468
    /// are reported on**, and the one that is not is the `xmpMM:InstanceID`
    /// fixture above, which no table entry exists to judge. There is nothing
    /// left for a `Rational`, `URI`, `URL`, `GPSCoordinate`, `XPath`,
    /// `Locale`, `MIMEType`, `ProperName`, `AgentName` or `RenditionClass`
    /// rule to catch, so none is written: a grammar written against no test is
    /// a guess, and a guess that is too strict reports conforming files. The
    /// same two directories hold 448 `-pass-` fixtures and this build reports
    /// on **none** of them.
    ///
    /// **The closed and open choices are read for their base type and not for
    /// their vocabulary**, and that is the same decision made once more. 51 of
    /// the 168 are fixtures for a closed or open choice, and **not one of them
    /// writes a syntactically valid integer that is merely outside a printed
    /// list**: every one fails because the value is not an integer at all —
    /// `2.0`, `2/5`, `Pos - 1`, `Some Value`, `value: 3`. Reading
    /// the vocabularies would also be reading a moving target: the September
    /// 2005 changelog records correcting `exif:ColorSpace`'s "uncalibrated"
    /// value from −32768 to 65535, so the two revisions print different
    /// admissible sets for one property, and a rule built on the wrong one
    /// reports a conforming file.
    ///
    /// **`xmp:Rating` is the one property whose value is not read at all.**
    /// September 2005 p. 41 declares it `Closed Choice of Integer`;
    /// `PDF_A-2b` `6-6-2-3-1-t07-pass-m` writes `1.0` into it, is annotated
    /// conforming, and has no failing twin. That is a published statement that
    /// the value is admissible, against a published statement that the type is
    /// `Integer`, and the same rule settles it as settled `xmpMM:InstanceID`
    /// one clause above: where a conformance suite calls a file conforming,
    /// this build does not report it. The exception is one property wide and
    /// lives at the call site in `xmp::schema_value_types`, not here, so that
    /// this function stays a statement about types.
    pub(super) fn judged(self) -> Option<Judged> {
        match self {
            Item::Integer | Item::ChoiceOfInteger => Some(Judged::Integer),
            Item::Real => Some(Judged::Real),
            Item::Boolean => Some(Judged::Boolean),
            Item::Date => Some(Judged::Date),
            Item::Text
            | Item::ProperName
            | Item::AgentName
            | Item::MimeType
            | Item::Locale
            | Item::XPath
            | Item::RenditionClass
            | Item::Uri
            | Item::Url
            | Item::GpsCoordinate
            | Item::Rational
            | Item::ChoiceOfText
            | Item::Point
            | Item::Structure(_) => None,
        }
    }
}

/// One predefined schema, and the value type each of its properties declares.
pub(super) struct Schema {
    /// The namespace URI, which is what a packet is matched on.
    pub(super) uri: &'static str,
    /// The prefix the specification prints beside the schema, carried so a
    /// finding can name a property the way a reader would write it. Both
    /// revisions call it the *preferred* prefix; it is never matched.
    pub(super) prefix: &'static str,
    /// The properties, **sorted by name**, so the lookup below is a binary
    /// search and the iteration order is one order on every target (ruling 4).
    pub(super) properties: &'static [(&'static str, Shape, Item)],
}

/// The value type `local` declares in `uri`, under the revision `part` cites.
///
/// `None` if that revision's table does not name the property — which is not a
/// statement that the property is unknown, only that nothing here can judge
/// it.
pub(super) fn value_type(uri: &str, local: &str, part: Part) -> Option<(Shape, Item)> {
    let schema = table_of(part)?.iter().find(|schema| schema.uri == uri)?;
    let index = schema
        .properties
        .binary_search_by(|(name, _, _)| (*name).cmp(local))
        .ok()?;
    let (_, shape, item) = schema.properties[index];
    Some((shape, item))
}

/// Whether the serialisation `written` satisfies the declared `(shape, item)`.
pub(super) fn satisfies(written: Written, shape: Shape, item: Item) -> bool {
    written.satisfies(shape, item)
}

/// The serialisation a declared value type asks for, for a finding's text.
pub(super) fn wanted(shape: Shape, item: Item) -> Written {
    match shape {
        Shape::One => {
            if matches!(item, Item::Structure(_)) {
                Written::Structure
            } else {
                Written::Simple
            }
        }
        Shape::Bag => Written::Bag,
        Shape::Seq => Written::Seq,
        Shape::Alt => Written::Alt,
        Shape::LangAlt => Written::LangAlt,
    }
}

/// The table the revision `part` cites, or `None` for a part that cites none.
fn table_of(part: Part) -> Option<&'static [Schema]> {
    match part {
        Part::One => Some(PREDEFINED_2004),
        Part::Two | Part::Three => Some(PREDEFINED_2005),
        // Part 4 asks this file nothing, and `None` is what it should get if
        // it ever does. `xmp::part_carries_the_predefined_schema_rule` is
        // false for it, so the walk that calls `value_type` does not run there
        // at all — ISO 19005-4 dropped the requirement rather than renumbering
        // it, and the conformance suite has no counterpart to `6.7.2
        // Properties` or `6.6.2.3 Schemas` anywhere under `PDF_A-4`. The arm
        // is written out rather than left to an `unreachable!` because the
        // honest answer is the same either way: part 4 is drafted against ISO
        // 16684-1, a third revision neither table above transcribes, so there
        // is no table to route it to. `None` means "nothing here can judge
        // it", which is the direction that cannot report a conforming file.
        Part::Four => None,
    }
}

/// The preferred prefix for a namespace either revision knows, for a finding's
/// own text.
///
/// Not routed by part, because a prefix is a spelling rather than a judgement:
/// every namespace January 2004 declared survives into September 2005 under
/// the same prefix, so the two tables never disagree about one. Both are
/// searched anyway, so this does not silently depend on that staying true.
pub(super) fn prefix_of(uri: &str) -> Option<&'static str> {
    PREDEFINED_2005
        .iter()
        .chain(PREDEFINED_2004)
        .find(|schema| schema.uri == uri)
        .map(|schema| schema.prefix)
}

/// The eleven schemas of the **January 2004** revision, sorted by URI: the
/// revision ISO 19005-1 6.7.2 cites.
///
/// 169 properties, chapter 4 "XMP Schemas", pages 37-58.
pub(super) const PREDEFINED_2004: &[Schema] = &[
    Schema {
        uri: "http://ns.adobe.com/exif/1.0/",
        prefix: "exif",
        properties: &[
            ("ApertureValue", Shape::One, Item::Rational),
            ("BrightnessValue", Shape::One, Item::Rational),
            ("CFAPattern", Shape::One, Item::Structure("CFAPattern")),
            ("ColorSpace", Shape::One, Item::ChoiceOfInteger),
            ("ComponentsConfiguration", Shape::Seq, Item::ChoiceOfInteger),
            ("CompressedBitsPerPixel", Shape::One, Item::Rational),
            ("Contrast", Shape::One, Item::ChoiceOfInteger),
            ("CustomRendered", Shape::One, Item::ChoiceOfInteger),
            ("DateTimeDigitized", Shape::One, Item::Date),
            ("DateTimeOriginal", Shape::One, Item::Date),
            (
                "DeviceSettingDescription",
                Shape::One,
                Item::Structure("DeviceSettings"),
            ),
            ("DigitalZoomRatio", Shape::One, Item::Rational),
            ("ExifVersion", Shape::One, Item::ChoiceOfText),
            ("ExposureBiasValue", Shape::One, Item::Rational),
            ("ExposureIndex", Shape::One, Item::Rational),
            ("ExposureMode", Shape::One, Item::ChoiceOfInteger),
            ("ExposureProgram", Shape::One, Item::ChoiceOfInteger),
            ("ExposureTime", Shape::One, Item::Rational),
            ("FNumber", Shape::One, Item::Rational),
            ("FileSource", Shape::One, Item::ChoiceOfInteger),
            ("Flash", Shape::One, Item::Structure("Flash")),
            ("FlashEnergy", Shape::One, Item::Rational),
            ("FlashpixVersion", Shape::One, Item::ChoiceOfText),
            ("FocalLength", Shape::One, Item::Rational),
            ("FocalLengthIn35mmFilm", Shape::One, Item::Integer),
            (
                "FocalPlaneResolutionUnit",
                Shape::One,
                Item::ChoiceOfInteger,
            ),
            ("FocalPlaneXResolution", Shape::One, Item::Rational),
            ("FocalPlaneYResolution", Shape::One, Item::Rational),
            ("GPSAltitude", Shape::One, Item::Rational),
            ("GPSAltitudeRef", Shape::One, Item::ChoiceOfInteger),
            ("GPSAreaInformation", Shape::One, Item::Text),
            ("GPSDOP", Shape::One, Item::Rational),
            ("GPSDestBearing", Shape::One, Item::Rational),
            ("GPSDestBearingRef", Shape::One, Item::ChoiceOfText),
            ("GPSDestDistance", Shape::One, Item::Rational),
            ("GPSDestDistanceRef", Shape::One, Item::ChoiceOfText),
            ("GPSDestLatitude", Shape::One, Item::GpsCoordinate),
            ("GPSDestLongitude", Shape::One, Item::GpsCoordinate),
            ("GPSDifferential", Shape::One, Item::ChoiceOfInteger),
            ("GPSImgDirection", Shape::One, Item::Rational),
            ("GPSImgDirectionRef", Shape::One, Item::ChoiceOfText),
            ("GPSLatitude", Shape::One, Item::GpsCoordinate),
            ("GPSLongitude", Shape::One, Item::GpsCoordinate),
            ("GPSMapDatum", Shape::One, Item::Text),
            ("GPSMeasureMode", Shape::One, Item::ChoiceOfInteger),
            ("GPSProcessingMethod", Shape::One, Item::Text),
            ("GPSSatellites", Shape::One, Item::Text),
            ("GPSSpeed", Shape::One, Item::Rational),
            ("GPSSpeedRef", Shape::One, Item::ChoiceOfText),
            ("GPSStatus", Shape::One, Item::ChoiceOfText),
            ("GPSTimeStamp", Shape::One, Item::Date),
            ("GPSTrack", Shape::One, Item::Rational),
            ("GPSTrackRef", Shape::One, Item::ChoiceOfText),
            ("GPSVersionID", Shape::One, Item::Text),
            ("GainControl", Shape::One, Item::ChoiceOfInteger),
            ("ISOSpeedRatings", Shape::Seq, Item::Integer),
            ("ImageUniqueID", Shape::One, Item::Text),
            ("LightSource", Shape::One, Item::ChoiceOfInteger),
            ("MakerNote", Shape::One, Item::Text),
            ("MaxApertureValue", Shape::One, Item::Rational),
            ("MeteringMode", Shape::One, Item::ChoiceOfInteger),
            ("OECF", Shape::One, Item::Structure("OECF/SFR")),
            ("PixelXDimension", Shape::One, Item::Integer),
            ("PixelYDimension", Shape::One, Item::Integer),
            ("RelatedSoundFile", Shape::One, Item::Text),
            ("Saturation", Shape::One, Item::ChoiceOfInteger),
            ("SceneCaptureType", Shape::One, Item::ChoiceOfInteger),
            ("SceneType", Shape::One, Item::ChoiceOfInteger),
            ("SensingMethod", Shape::One, Item::ChoiceOfInteger),
            ("Sharpness", Shape::One, Item::ChoiceOfInteger),
            ("ShutterSpeedValue", Shape::One, Item::Rational),
            (
                "SpatialFrequencyResponse",
                Shape::One,
                Item::Structure("OECF/SFR"),
            ),
            ("SpectralSensitivity", Shape::One, Item::Text),
            ("SubjectArea", Shape::Seq, Item::Integer),
            ("SubjectDistance", Shape::One, Item::Rational),
            ("SubjectDistanceRange", Shape::One, Item::ChoiceOfInteger),
            ("SubjectLocation", Shape::Seq, Item::Integer),
            ("UserComment", Shape::LangAlt, Item::Text),
            ("WhiteBalance", Shape::One, Item::ChoiceOfInteger),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/pdf/1.3/",
        prefix: "pdf",
        properties: &[
            ("Keywords", Shape::One, Item::Text),
            ("PDFVersion", Shape::One, Item::Text),
            ("Producer", Shape::One, Item::AgentName),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/photoshop/1.0/",
        prefix: "photoshop",
        properties: &[
            ("AuthorsPosition", Shape::One, Item::Text),
            ("CaptionWriter", Shape::One, Item::ProperName),
            ("Category", Shape::One, Item::Text),
            ("City", Shape::One, Item::Text),
            ("Country", Shape::One, Item::Text),
            ("Credit", Shape::One, Item::Text),
            ("DateCreated", Shape::One, Item::Date),
            ("Headline", Shape::One, Item::Text),
            ("Instructions", Shape::One, Item::Text),
            ("Source", Shape::One, Item::Text),
            ("State", Shape::One, Item::Text),
            ("SupplementalCategories", Shape::One, Item::Text),
            ("TransmissionReference", Shape::One, Item::Text),
            ("Urgency", Shape::One, Item::Integer),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/tiff/1.0/",
        prefix: "tiff",
        properties: &[
            ("Artist", Shape::One, Item::ProperName),
            ("BitsPerSample", Shape::Seq, Item::Integer),
            ("Compression", Shape::One, Item::ChoiceOfInteger),
            ("Copyright", Shape::LangAlt, Item::Text),
            ("DateTime", Shape::One, Item::Date),
            ("ImageDescription", Shape::LangAlt, Item::Text),
            ("ImageLength", Shape::One, Item::Integer),
            ("ImageWidth", Shape::One, Item::Integer),
            ("Make", Shape::One, Item::ProperName),
            ("Model", Shape::One, Item::ProperName),
            ("Orientation", Shape::One, Item::ChoiceOfInteger),
            (
                "PhotometricInterpretation",
                Shape::One,
                Item::ChoiceOfInteger,
            ),
            ("PlanarConfiguration", Shape::One, Item::ChoiceOfInteger),
            ("PrimaryChromaticities", Shape::Seq, Item::Rational),
            ("ReferenceBlackWhite", Shape::Seq, Item::Rational),
            ("ResolutionUnit", Shape::One, Item::ChoiceOfInteger),
            ("SamplesPerPixel", Shape::One, Item::Integer),
            ("Software", Shape::One, Item::AgentName),
            ("TransferFunction", Shape::Seq, Item::Integer),
            ("WhitePoint", Shape::Seq, Item::Rational),
            ("XResolution", Shape::One, Item::Rational),
            ("YCbCrCoefficients", Shape::Seq, Item::Rational),
            ("YCbCrPositioning", Shape::One, Item::ChoiceOfInteger),
            ("YCbCrSubSampling", Shape::Seq, Item::ChoiceOfInteger),
            ("YResolution", Shape::One, Item::Rational),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/",
        prefix: "xmp",
        properties: &[
            ("Advisory", Shape::Bag, Item::XPath),
            ("BaseURL", Shape::One, Item::Url),
            ("CreateDate", Shape::One, Item::Date),
            ("CreatorTool", Shape::One, Item::AgentName),
            ("Identifier", Shape::Bag, Item::Text),
            ("MetadataDate", Shape::One, Item::Date),
            ("ModifyDate", Shape::One, Item::Date),
            ("Nickname", Shape::One, Item::Text),
            ("Thumbnails", Shape::Alt, Item::Structure("Thumbnail")),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/bj/",
        prefix: "xmpBJ",
        properties: &[("JobRef", Shape::Bag, Item::Structure("Job"))],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/mm/",
        prefix: "xmpMM",
        properties: &[
            ("DerivedFrom", Shape::One, Item::Structure("ResourceRef")),
            ("DocumentID", Shape::One, Item::Uri),
            ("History", Shape::Seq, Item::Structure("ResourceEvent")),
            ("LastURL", Shape::One, Item::Url),
            ("ManageTo", Shape::One, Item::Uri),
            ("ManageUI", Shape::One, Item::Uri),
            ("ManagedFrom", Shape::One, Item::Structure("ResourceRef")),
            ("Manager", Shape::One, Item::AgentName),
            ("ManagerVariant", Shape::One, Item::Text),
            ("RenditionClass", Shape::One, Item::RenditionClass),
            ("RenditionOf", Shape::One, Item::Structure("ResourceRef")),
            ("RenditionParams", Shape::One, Item::Text),
            ("SaveID", Shape::One, Item::Integer),
            ("VersionID", Shape::One, Item::Text),
            ("Versions", Shape::Seq, Item::Structure("Version")),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/rights/",
        prefix: "xmpRights",
        properties: &[
            ("Certificate", Shape::One, Item::Url),
            ("Marked", Shape::One, Item::Boolean),
            ("Owner", Shape::Bag, Item::ProperName),
            ("UsageTerms", Shape::LangAlt, Item::Text),
            ("WebStatement", Shape::One, Item::Url),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/t/pg/",
        prefix: "xmpTPg",
        properties: &[
            ("MaxPageSize", Shape::One, Item::Structure("Dimensions")),
            ("NPages", Shape::One, Item::Integer),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xmp/Identifier/qual/1.0/",
        prefix: "xmpidq",
        properties: &[("Scheme", Shape::One, Item::Text)],
    },
    Schema {
        uri: "http://purl.org/dc/elements/1.1/",
        prefix: "dc",
        properties: &[
            ("contributor", Shape::Bag, Item::ProperName),
            ("coverage", Shape::One, Item::Text),
            ("creator", Shape::Seq, Item::ProperName),
            ("date", Shape::Seq, Item::Date),
            ("description", Shape::LangAlt, Item::Text),
            ("format", Shape::One, Item::MimeType),
            ("identifier", Shape::One, Item::Text),
            ("language", Shape::Bag, Item::Locale),
            ("publisher", Shape::Bag, Item::ProperName),
            ("relation", Shape::Bag, Item::Text),
            ("rights", Shape::LangAlt, Item::Text),
            ("source", Shape::One, Item::Text),
            ("subject", Shape::Bag, Item::Text),
            ("title", Shape::LangAlt, Item::Text),
            ("type", Shape::Bag, Item::ChoiceOfText),
        ],
    },
];

/// The fourteen schemas of the **September 2005** revision, sorted by URI: the
/// revision ISO 19005-2 6.6.2.3 and ISO 19005-3 cite.
///
/// 274 properties, chapter 4 "XMP Schemas", pages 39-70. Three schemas the
/// January 2004 revision has no counterpart for -- Camera Raw (`crs`), XMP
/// Dynamic Media (`xmpDM`) and Additional EXIF Properties (`aux`) -- and 106
/// properties it does not name.
pub(super) const PREDEFINED_2005: &[Schema] = &[
    Schema {
        uri: "http://ns.adobe.com/camera-raw-settings/1.0/",
        prefix: "crs",
        properties: &[
            ("AutoBrightness", Shape::One, Item::Boolean),
            ("AutoContrast", Shape::One, Item::Boolean),
            ("AutoExposure", Shape::One, Item::Boolean),
            ("AutoShadows", Shape::One, Item::Boolean),
            ("BlueHue", Shape::One, Item::Integer),
            ("BlueSaturation", Shape::One, Item::Integer),
            ("Brightness", Shape::One, Item::Integer),
            ("CameraProfile", Shape::One, Item::Text),
            ("ChromaticAberrationB", Shape::One, Item::Integer),
            ("ChromaticAberrationR", Shape::One, Item::Integer),
            ("ColorNoiseReduction", Shape::One, Item::Integer),
            ("Contrast", Shape::One, Item::Integer),
            ("CropAngle", Shape::One, Item::Real),
            ("CropBottom", Shape::One, Item::Real),
            ("CropHeight", Shape::One, Item::Real),
            ("CropLeft", Shape::One, Item::Real),
            ("CropRight", Shape::One, Item::Real),
            ("CropTop", Shape::One, Item::Real),
            ("CropUnits", Shape::One, Item::Integer),
            ("CropWidth", Shape::One, Item::Real),
            ("Exposure", Shape::One, Item::Real),
            ("GreenHue", Shape::One, Item::Integer),
            ("GreenSaturation", Shape::One, Item::Integer),
            ("HasCrop", Shape::One, Item::Boolean),
            ("HasSettings", Shape::One, Item::Boolean),
            ("LuminanceSmoothing", Shape::One, Item::Integer),
            ("RawFileName", Shape::One, Item::Text),
            ("RedHue", Shape::One, Item::Integer),
            ("RedSaturation", Shape::One, Item::Integer),
            ("Saturation", Shape::One, Item::Integer),
            ("ShadowTint", Shape::One, Item::Integer),
            ("Shadows", Shape::One, Item::Integer),
            ("Sharpness", Shape::One, Item::Integer),
            ("Temperature", Shape::One, Item::Integer),
            ("Tint", Shape::One, Item::Integer),
            ("ToneCurve", Shape::Seq, Item::Point),
            ("ToneCurveName", Shape::One, Item::ChoiceOfText),
            ("Version", Shape::One, Item::Text),
            ("VignetteAmount", Shape::One, Item::Integer),
            ("VignetteMidpoint", Shape::One, Item::Integer),
            ("WhiteBalance", Shape::One, Item::ChoiceOfText),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/exif/1.0/",
        prefix: "exif",
        properties: &[
            ("ApertureValue", Shape::One, Item::Rational),
            ("BrightnessValue", Shape::One, Item::Rational),
            ("CFAPattern", Shape::One, Item::Structure("CFAPattern")),
            ("ColorSpace", Shape::One, Item::ChoiceOfInteger),
            ("ComponentsConfiguration", Shape::Seq, Item::ChoiceOfInteger),
            ("CompressedBitsPerPixel", Shape::One, Item::Rational),
            ("Contrast", Shape::One, Item::ChoiceOfInteger),
            ("CustomRendered", Shape::One, Item::ChoiceOfInteger),
            ("DateTimeDigitized", Shape::One, Item::Date),
            ("DateTimeOriginal", Shape::One, Item::Date),
            (
                "DeviceSettingDescription",
                Shape::One,
                Item::Structure("DeviceSettings"),
            ),
            ("DigitalZoomRatio", Shape::One, Item::Rational),
            ("ExifVersion", Shape::One, Item::ChoiceOfText),
            ("ExposureBiasValue", Shape::One, Item::Rational),
            ("ExposureIndex", Shape::One, Item::Rational),
            ("ExposureMode", Shape::One, Item::ChoiceOfInteger),
            ("ExposureProgram", Shape::One, Item::ChoiceOfInteger),
            ("ExposureTime", Shape::One, Item::Rational),
            ("FNumber", Shape::One, Item::Rational),
            ("FileSource", Shape::One, Item::ChoiceOfInteger),
            ("Flash", Shape::One, Item::Structure("Flash")),
            ("FlashEnergy", Shape::One, Item::Rational),
            ("FlashpixVersion", Shape::One, Item::ChoiceOfText),
            ("FocalLength", Shape::One, Item::Rational),
            ("FocalLengthIn35mmFilm", Shape::One, Item::Integer),
            (
                "FocalPlaneResolutionUnit",
                Shape::One,
                Item::ChoiceOfInteger,
            ),
            ("FocalPlaneXResolution", Shape::One, Item::Rational),
            ("FocalPlaneYResolution", Shape::One, Item::Rational),
            ("GPSAltitude", Shape::One, Item::Rational),
            ("GPSAltitudeRef", Shape::One, Item::ChoiceOfInteger),
            ("GPSAreaInformation", Shape::One, Item::Text),
            ("GPSDOP", Shape::One, Item::Rational),
            ("GPSDestBearing", Shape::One, Item::Rational),
            ("GPSDestBearingRef", Shape::One, Item::ChoiceOfText),
            ("GPSDestDistance", Shape::One, Item::Rational),
            ("GPSDestDistanceRef", Shape::One, Item::ChoiceOfText),
            ("GPSDestLatitude", Shape::One, Item::GpsCoordinate),
            ("GPSDestLongitude", Shape::One, Item::GpsCoordinate),
            ("GPSDifferential", Shape::One, Item::ChoiceOfInteger),
            ("GPSImgDirection", Shape::One, Item::Rational),
            ("GPSImgDirectionRef", Shape::One, Item::ChoiceOfText),
            ("GPSLatitude", Shape::One, Item::GpsCoordinate),
            ("GPSLongitude", Shape::One, Item::GpsCoordinate),
            ("GPSMapDatum", Shape::One, Item::Text),
            ("GPSMeasureMode", Shape::One, Item::Text),
            ("GPSProcessingMethod", Shape::One, Item::Text),
            ("GPSSatellites", Shape::One, Item::Text),
            ("GPSSpeed", Shape::One, Item::Rational),
            ("GPSSpeedRef", Shape::One, Item::ChoiceOfText),
            ("GPSStatus", Shape::One, Item::ChoiceOfText),
            ("GPSTimeStamp", Shape::One, Item::Date),
            ("GPSTrack", Shape::One, Item::Rational),
            ("GPSTrackRef", Shape::One, Item::ChoiceOfText),
            ("GPSVersionID", Shape::One, Item::Text),
            ("GainControl", Shape::One, Item::ChoiceOfInteger),
            ("ISOSpeedRatings", Shape::Seq, Item::Integer),
            ("ImageUniqueID", Shape::One, Item::Text),
            ("LightSource", Shape::One, Item::ChoiceOfInteger),
            ("MaxApertureValue", Shape::One, Item::Rational),
            ("MeteringMode", Shape::One, Item::ChoiceOfInteger),
            ("OECF", Shape::One, Item::Structure("OECF/SFR")),
            ("PixelXDimension", Shape::One, Item::Integer),
            ("PixelYDimension", Shape::One, Item::Integer),
            ("RelatedSoundFile", Shape::One, Item::Text),
            ("Saturation", Shape::One, Item::ChoiceOfInteger),
            ("SceneCaptureType", Shape::One, Item::ChoiceOfInteger),
            ("SceneType", Shape::One, Item::ChoiceOfInteger),
            ("SensingMethod", Shape::One, Item::ChoiceOfInteger),
            ("Sharpness", Shape::One, Item::ChoiceOfInteger),
            ("ShutterSpeedValue", Shape::One, Item::Rational),
            (
                "SpatialFrequencyResponse",
                Shape::One,
                Item::Structure("OECF/SFR"),
            ),
            ("SpectralSensitivity", Shape::One, Item::Text),
            ("SubjectArea", Shape::Seq, Item::Integer),
            ("SubjectDistance", Shape::One, Item::Rational),
            ("SubjectDistanceRange", Shape::One, Item::ChoiceOfInteger),
            ("SubjectLocation", Shape::Seq, Item::Integer),
            ("UserComment", Shape::LangAlt, Item::Text),
            ("WhiteBalance", Shape::One, Item::ChoiceOfInteger),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/exif/1.0/aux/",
        prefix: "aux",
        properties: &[
            ("Lens", Shape::One, Item::Text),
            ("SerialNumber", Shape::One, Item::Text),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/pdf/1.3/",
        prefix: "pdf",
        properties: &[
            ("Keywords", Shape::One, Item::Text),
            ("PDFVersion", Shape::One, Item::Text),
            ("Producer", Shape::One, Item::AgentName),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/photoshop/1.0/",
        prefix: "photoshop",
        properties: &[
            ("AuthorsPosition", Shape::One, Item::Text),
            ("CaptionWriter", Shape::One, Item::ProperName),
            ("Category", Shape::One, Item::Text),
            ("City", Shape::One, Item::Text),
            ("Country", Shape::One, Item::Text),
            ("Credit", Shape::One, Item::Text),
            ("DateCreated", Shape::One, Item::Date),
            ("Headline", Shape::One, Item::Text),
            ("Instructions", Shape::One, Item::Text),
            ("Source", Shape::One, Item::Text),
            ("State", Shape::One, Item::Text),
            ("SupplementalCategories", Shape::Bag, Item::Text),
            ("TransmissionReference", Shape::One, Item::Text),
            ("Urgency", Shape::One, Item::Integer),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/tiff/1.0/",
        prefix: "tiff",
        properties: &[
            ("Artist", Shape::One, Item::ProperName),
            ("BitsPerSample", Shape::Seq, Item::Integer),
            ("Compression", Shape::One, Item::ChoiceOfInteger),
            ("Copyright", Shape::LangAlt, Item::Text),
            ("DateTime", Shape::One, Item::Date),
            ("ImageDescription", Shape::LangAlt, Item::Text),
            ("ImageLength", Shape::One, Item::Integer),
            ("ImageWidth", Shape::One, Item::Integer),
            ("Make", Shape::One, Item::ProperName),
            ("Model", Shape::One, Item::ProperName),
            ("Orientation", Shape::One, Item::ChoiceOfInteger),
            (
                "PhotometricInterpretation",
                Shape::One,
                Item::ChoiceOfInteger,
            ),
            ("PlanarConfiguration", Shape::One, Item::ChoiceOfInteger),
            ("PrimaryChromaticities", Shape::Seq, Item::Rational),
            ("ReferenceBlackWhite", Shape::Seq, Item::Rational),
            ("ResolutionUnit", Shape::One, Item::ChoiceOfInteger),
            ("SamplesPerPixel", Shape::One, Item::Integer),
            ("Software", Shape::One, Item::AgentName),
            ("TransferFunction", Shape::Seq, Item::Integer),
            ("WhitePoint", Shape::Seq, Item::Rational),
            ("XResolution", Shape::One, Item::Rational),
            ("YCbCrCoefficients", Shape::Seq, Item::Rational),
            ("YCbCrPositioning", Shape::One, Item::ChoiceOfInteger),
            ("YCbCrSubSampling", Shape::Seq, Item::ChoiceOfInteger),
            ("YResolution", Shape::One, Item::Rational),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/",
        prefix: "xmp",
        properties: &[
            ("Advisory", Shape::Bag, Item::XPath),
            ("BaseURL", Shape::One, Item::Url),
            ("CreateDate", Shape::One, Item::Date),
            ("CreatorTool", Shape::One, Item::AgentName),
            ("Identifier", Shape::Bag, Item::Text),
            ("Label", Shape::One, Item::Text),
            ("MetadataDate", Shape::One, Item::Date),
            ("ModifyDate", Shape::One, Item::Date),
            ("Nickname", Shape::One, Item::Text),
            ("Rating", Shape::One, Item::ChoiceOfInteger),
            ("Thumbnails", Shape::Alt, Item::Structure("Thumbnail")),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/bj/",
        prefix: "xmpBJ",
        properties: &[("JobRef", Shape::Bag, Item::Structure("Job"))],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/mm/",
        prefix: "xmpMM",
        properties: &[
            ("DerivedFrom", Shape::One, Item::Structure("ResourceRef")),
            ("DocumentID", Shape::One, Item::Uri),
            ("History", Shape::Seq, Item::Structure("ResourceEvent")),
            ("InstanceID", Shape::One, Item::Uri),
            ("LastURL", Shape::One, Item::Url),
            ("ManageTo", Shape::One, Item::Uri),
            ("ManageUI", Shape::One, Item::Uri),
            ("ManagedFrom", Shape::One, Item::Structure("ResourceRef")),
            ("Manager", Shape::One, Item::AgentName),
            ("ManagerVariant", Shape::One, Item::Text),
            ("RenditionClass", Shape::One, Item::RenditionClass),
            ("RenditionOf", Shape::One, Item::Structure("ResourceRef")),
            ("RenditionParams", Shape::One, Item::Text),
            ("SaveID", Shape::One, Item::Integer),
            ("VersionID", Shape::One, Item::Text),
            ("Versions", Shape::Seq, Item::Structure("Version")),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/rights/",
        prefix: "xmpRights",
        properties: &[
            ("Certificate", Shape::One, Item::Url),
            ("Marked", Shape::One, Item::Boolean),
            ("Owner", Shape::Bag, Item::ProperName),
            ("UsageTerms", Shape::LangAlt, Item::Text),
            ("WebStatement", Shape::One, Item::Url),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/t/pg/",
        prefix: "xmpTPg",
        properties: &[
            ("Colorants", Shape::Seq, Item::Structure("Colorant")),
            ("Fonts", Shape::Bag, Item::Structure("Font")),
            ("MaxPageSize", Shape::One, Item::Structure("Dimensions")),
            ("NPages", Shape::One, Item::Integer),
            ("PlateNames", Shape::Seq, Item::Text),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xmp/1.0/DynamicMedia/",
        prefix: "xmpDM",
        properties: &[
            ("absPeakAudioFilePath", Shape::One, Item::Uri),
            ("album", Shape::One, Item::Text),
            ("altTapeName", Shape::One, Item::Text),
            ("altTimecode", Shape::One, Item::Structure("Timecode")),
            ("artist", Shape::One, Item::Text),
            ("audioChannelType", Shape::One, Item::ChoiceOfText),
            ("audioCompressor", Shape::One, Item::Text),
            ("audioModDate", Shape::One, Item::Date),
            ("audioSampleRate", Shape::One, Item::Integer),
            ("audioSampleType", Shape::One, Item::ChoiceOfText),
            (
                "beatSpliceParams",
                Shape::One,
                Item::Structure("beatSpliceStretch"),
            ),
            ("composer", Shape::One, Item::Text),
            ("contributedMedia", Shape::Bag, Item::Structure("Media")),
            ("copyright", Shape::One, Item::Text),
            ("duration", Shape::One, Item::Structure("Time")),
            ("engineer", Shape::One, Item::Text),
            ("fileDataRate", Shape::One, Item::Rational),
            ("genre", Shape::One, Item::Text),
            ("instrument", Shape::One, Item::Text),
            ("introTime", Shape::One, Item::Structure("Time")),
            ("key", Shape::One, Item::ChoiceOfText),
            ("logComment", Shape::One, Item::Text),
            ("loop", Shape::One, Item::Boolean),
            ("markers", Shape::Seq, Item::Structure("Marker")),
            ("metadataModDate", Shape::One, Item::Date),
            ("numberOfBeats", Shape::One, Item::Real),
            ("outCue", Shape::One, Item::Structure("Time")),
            ("projectRef", Shape::One, Item::Structure("ProjectLink")),
            ("pullDown", Shape::One, Item::ChoiceOfText),
            ("relativePeakAudioFilePath", Shape::One, Item::Uri),
            ("relativeTimestamp", Shape::One, Item::Structure("Time")),
            ("releaseDate", Shape::One, Item::Date),
            (
                "resampleParams",
                Shape::One,
                Item::Structure("resampleStretch"),
            ),
            ("scaleType", Shape::One, Item::ChoiceOfText),
            ("scene", Shape::One, Item::Text),
            ("shotDate", Shape::One, Item::Date),
            ("shotLocation", Shape::One, Item::Text),
            ("shotName", Shape::One, Item::Text),
            ("speakerPlacement", Shape::One, Item::Text),
            ("startTimecode", Shape::One, Item::Structure("Timecode")),
            ("stretchMode", Shape::One, Item::ChoiceOfText),
            ("tapeName", Shape::One, Item::Text),
            ("tempo", Shape::One, Item::Real),
            (
                "timeScaleParams",
                Shape::One,
                Item::Structure("timeScaleStretch"),
            ),
            ("timeSignature", Shape::One, Item::ChoiceOfText),
            ("trackNumber", Shape::One, Item::Integer),
            ("videoAlphaMode", Shape::One, Item::ChoiceOfText),
            (
                "videoAlphaPremultipleColor",
                Shape::One,
                Item::Structure("Colorant"),
            ),
            ("videoAlphaUnityIsTransparent", Shape::One, Item::Boolean),
            ("videoColorSpace", Shape::One, Item::ChoiceOfText),
            ("videoCompressor", Shape::One, Item::Text),
            ("videoFieldOrder", Shape::One, Item::ChoiceOfText),
            ("videoFrameRate", Shape::One, Item::ChoiceOfText),
            ("videoFrameSize", Shape::One, Item::Structure("Dimensions")),
            ("videoModDate", Shape::One, Item::Date),
            ("videoPixelAspectRatio", Shape::One, Item::Rational),
            ("videoPixelDepth", Shape::One, Item::ChoiceOfText),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xmp/Identifier/qual/1.0/",
        prefix: "xmpidq",
        properties: &[("Scheme", Shape::One, Item::Text)],
    },
    Schema {
        uri: "http://purl.org/dc/elements/1.1/",
        prefix: "dc",
        properties: &[
            ("contributor", Shape::Bag, Item::ProperName),
            ("coverage", Shape::One, Item::Text),
            ("creator", Shape::Seq, Item::ProperName),
            ("date", Shape::Seq, Item::Date),
            ("description", Shape::LangAlt, Item::Text),
            ("format", Shape::One, Item::MimeType),
            ("identifier", Shape::One, Item::Text),
            ("language", Shape::Bag, Item::Locale),
            ("publisher", Shape::Bag, Item::ProperName),
            ("relation", Shape::Bag, Item::Text),
            ("rights", Shape::LangAlt, Item::Text),
            ("source", Shape::One, Item::Text),
            ("subject", Shape::Bag, Item::Text),
            ("title", Shape::LangAlt, Item::Text),
            ("type", Shape::Bag, Item::ChoiceOfText),
        ],
    },
];
