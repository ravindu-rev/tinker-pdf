//! Font programs a test can state the coverage of (gap 31, milestone 9).
//!
//! `tests/substitute_fonts.rs` records why a fixture face is **synthesised**
//! rather than read from the system: the test is then identical on every
//! platform and the repository carries no font anybody has to licence. That
//! argument is unchanged here; what changed is what a face has to be able to
//! say.
//!
//! # `boxy_font` cannot declare coverage, and milestone 9 is about coverage
//!
//! The face `tests/substitute_fonts.rs` and `tests/epub_package.rs` share emits
//! `head`, `loca` and `glyf` and nothing else. That is exactly enough to answer
//! *"did a glyph get drawn"* for a [`tinker_pdf::FontProvider`], which reaches
//! a glyph by code and never asks a `cmap` anything. It cannot answer *"which
//! characters does this face have"*, because it has no `cmap` to answer with —
//! and `css-fonts-4` §5.3's per-character matching is that question and nothing
//! else. So [`covering`] builds a real one.
//!
//! # What "real" has to mean here, and the three tables that had to be right
//!
//! - **`cmap`, format 4**, in the (3, 1) Windows BMP encoding this build's
//!   `Sfnt::glyph_for_char` prefers, with one segment per contiguous run of
//!   covered characters and the mandatory `0xFFFF` terminator. A character
//!   outside every segment reads back as glyph 0, which is `.notdef`, which is
//!   a `cmap` saying **no** — so a fixture face refuses characters rather than
//!   claiming the whole plane.
//! - **`hmtx` and `hhea`**, because an embedded face's advances and line height
//!   come from the file rather than from a table of guesses. `hhea`'s
//!   `numberOfHMetrics` is at offset 34 and its `ascender` at offset 4, and a
//!   fixture that got either wrong would agree with a reader that got the same
//!   one wrong.
//! - **`loca` and `glyf`**, so the program survives subsetting on the way into
//!   a PDF: `DocumentBuilder` subsets a composite font to the glyphs a document
//!   drew, and a face whose outlines are absent is one the subsetter drops.
//!
//! `name` and `maxp` are written because a font without them is not a font a
//! third party will read, and gap 31's oracle is qpdf.

/// One glyph's outline: a filled square, as one contour of four on-curve
/// points.
///
/// The shape is not the claim anywhere in this file — coverage is — but an
/// **empty** glyph is indistinguishable from a missing one to everything
/// downstream, so every covered character gets ink.
fn box_glyph(side: i16) -> Vec<u8> {
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes()); // one contour
    glyph.extend_from_slice(&0i16.to_be_bytes()); // xMin
    glyph.extend_from_slice(&0i16.to_be_bytes()); // yMin
    glyph.extend_from_slice(&side.to_be_bytes()); // xMax
    glyph.extend_from_slice(&side.to_be_bytes()); // yMax
    glyph.extend_from_slice(&3u16.to_be_bytes()); // last point of contour 0
    glyph.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on-curve, word deltas
    for dx in [0i16, side, 0, -side] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, side, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }
    glyph
}

/// How a synthesised face is described: what it covers and how wide it is.
///
/// A struct rather than four positional arguments, because two of the four are
/// `u16`s in the same units and a fixture that swapped them would still build
/// a font.
#[derive(Clone, Debug)]
pub struct Face {
    /// The family name written into the `name` table, `nameID` 1.
    pub family: String,
    /// Exactly the characters this face has a glyph for. Order does not
    /// matter; duplicates are ignored.
    pub covers: Vec<char>,
    /// Every covered glyph's advance, in font units.
    pub advance: u16,
    /// The em square, in font units.
    pub units_per_em: u16,
    /// `hhea`'s `ascender`, in font units.
    pub ascender: i16,
    /// `hhea`'s `descender`, in font units and **negative**, which is the
    /// sfnt's own sign convention.
    pub descender: i16,
    /// A `GSUB` ligature: two characters this face joins into one glyph, under
    /// one feature of one script.
    ///
    /// `None` — the default — is a face with no `GSUB` at all, which is what
    /// every fixture wanted before shaping existed and is still what most
    /// want. `Some` is the smallest face that makes shaping observable: with
    /// it a run of two characters is one glyph and one advance, and without it
    /// two of each.
    pub ligature: Option<Ligature>,
    /// A `GSUB` giving every covered character an initial, medial and final
    /// form under `init`, `medi` and `fina`.
    ///
    /// `None` — the default — is a face whose letters look the same wherever
    /// they stand, which is every script but the cursive ones. `Some` is the
    /// smallest face that makes **joining** observable: shaping a word of
    /// three letters through it produces three *different* glyphs from the
    /// three the `cmap` alone would give, so a test can tell a shaped run from
    /// an unshaped one by glyph index and not by eye.
    pub joining: Option<Joining>,
    /// The advance of every glyph a `GSUB` substitution produces, where it
    /// differs from [`Face::advance`].
    ///
    /// `None` is a face whose every glyph is the same width, which is what
    /// every fixture wanted before shaping existed. `Some` is what makes the
    /// **shaped** measurement of a run distinguishable from the
    /// character-at-a-time one: with one advance throughout, a build that
    /// measured a joined word by summing its unjoined letters gets the right
    /// answer by arithmetic and nothing can see the mistake.
    pub joined_advance: Option<u16>,
    /// A `GPOS` `SinglePos` that displaces one character's glyph from where
    /// its advance would put it.
    ///
    /// `None` — the default — is a face with no `GPOS` at all, which is every
    /// fixture that came before this one and is why nothing in this repository
    /// could tell a build that carried positioning offsets onto the page from
    /// one that dropped them. `Some` is the smallest face that can tell them
    /// apart.
    pub placement: Option<Placement>,
}

/// One glyph displaced from its advance, by a `GPOS` `SinglePos`.
///
/// A whole struct for four values because three of them are numbers in
/// different spaces — a character, two font-unit displacements and a feature
/// tag — and positional arguments would let a caller swap the two
/// displacements and still build a font.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// The character whose glyph the lookup covers. **One and not all**: the
    /// glyphs either side of it are what say the pen came back.
    pub ch: char,
    /// The script tag the lookup is declared under. `DFLT` reaches every run,
    /// because a face that declares no script for a run's own tag falls back
    /// to it.
    pub script: [u8; 4],
    /// The feature tag. It has to be one the default shaper turns on —
    /// `tinker_pdf_shape::shape::DEFAULT_GPOS_FEATURES` — or the lookup is in
    /// the file and never runs.
    pub feature: [u8; 4],
    /// `XPlacement`, in font units: how far along the baseline the glyph moves
    /// **without** moving the pen.
    pub x: i16,
    /// `YPlacement`, in font units, positive away from the descenders.
    pub y: i16,
}

/// A face whose letters take a different form by position.
///
/// One tag and no more: which features are involved is the Unicode Standard's
/// rule and not a fixture's choice, and a builder that let a caller name them
/// would be a fixture that could disagree with the specification.
#[derive(Clone, Copy, Debug)]
pub struct Joining {
    /// The script tag the lookups are declared under, such as `arab`.
    pub script: [u8; 4],
}

/// Which of the three positional forms a joining face carries.
///
/// `isol` is deliberately absent: the isolated form *is* the glyph the `cmap`
/// gives, so a face that substituted one would be saying nothing, and a test
/// that saw the substitution could not tell it from a shaper that had left the
/// glyph alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// `init`: the first letter of a joined word.
    Initial,
    /// `medi`: a letter joined on both sides.
    Medial,
    /// `fina`: the last letter of a joined word.
    Final,
}

impl Form {
    /// The OpenType feature tag this form is selected by.
    const fn tag(self) -> [u8; 4] {
        match self {
            Form::Initial => *b"init",
            Form::Medial => *b"medi",
            Form::Final => *b"fina",
        }
    }

    /// Which block of substituted glyphs this form occupies, counting from the
    /// first glyph after the plain ones.
    const fn block(self) -> usize {
        match self {
            Form::Initial => 0,
            Form::Medial => 1,
            Form::Final => 2,
        }
    }

    /// The three, in the order their glyph blocks are laid out.
    const ALL: [Form; 3] = [Form::Initial, Form::Medial, Form::Final];
}

/// Two characters that become one glyph.
///
/// The glyph the pair becomes is appended **after** every covered character,
/// so `Face::glyph_of` keeps answering for the characters and the ligature's
/// own index is `glyph_order().len() + 1`.
#[derive(Clone, Copy, Debug)]
pub struct Ligature {
    /// The first character of the pair.
    pub first: char,
    /// The second.
    pub second: char,
    /// The script tag the lookup is declared under, such as `arab`.
    pub script: [u8; 4],
    /// The feature tag, such as `rlig`.
    pub feature: [u8; 4],
}

impl Face {
    /// A face covering exactly `covers`, at 1000 units per em.
    #[must_use]
    pub fn new(family: &str, covers: &str) -> Face {
        Face {
            family: family.to_owned(),
            covers: covers.chars().collect(),
            advance: 500,
            units_per_em: 1000,
            ascender: 800,
            descender: -200,
            ligature: None,
            joining: None,
            joined_advance: None,
            placement: None,
        }
    }

    /// The same face, displacing one character's glyph through `GPOS`.
    #[must_use]
    pub fn with_placement(mut self, placement: Placement) -> Face {
        self.placement = Some(placement);
        self
    }

    /// The same face, joining `first` and `second` into one glyph.
    #[must_use]
    pub fn with_ligature(mut self, ligature: Ligature) -> Face {
        self.ligature = Some(ligature);
        self
    }

    /// The same face, whose substituted glyphs are `advance` units wide.
    #[must_use]
    pub fn with_joined_advance(mut self, advance: u16) -> Face {
        self.joined_advance = Some(advance);
        self
    }

    /// The same face, with an initial, medial and final form for every
    /// character it covers.
    #[must_use]
    pub fn with_joining(mut self, joining: Joining) -> Face {
        self.joining = Some(joining);
        self
    }

    /// The glyph a character takes in one joining form.
    ///
    /// `None` for a face with no joining, or a character it does not cover —
    /// the two answers a test wants to be able to tell apart from a glyph
    /// index that happens to exist.
    ///
    /// The blocks are contiguous and in [`Form::ALL`]'s order, after the plain
    /// glyphs and after the ligature's, so a caller can predict every index
    /// without reading the font back.
    #[must_use]
    pub fn form_glyph(&self, ch: char, form: Form) -> Option<u16> {
        self.joining?;
        let plain = self.glyph_of(ch)?;
        let covered = u16::try_from(self.glyph_order().len()).ok()?;
        let first = covered + 1 + u16::from(self.ligature.is_some());
        let block = u16::try_from(form.block()).ok()?;
        first
            .checked_add(block.checked_mul(covered)?)?
            .checked_add(plain - 1)
    }

    /// The glyph index the ligature's own glyph has, if this face has one.
    #[must_use]
    pub fn ligature_glyph(&self) -> Option<u16> {
        self.ligature?;
        u16::try_from(self.glyph_order().len() + 1).ok()
    }

    /// The same face at another advance, which is what makes two faces
    /// measurably different rather than merely differently named.
    #[must_use]
    pub fn with_advance(mut self, advance: u16) -> Face {
        self.advance = advance;
        self
    }

    /// The same face at another ascent and descent.
    #[must_use]
    pub fn with_vertical(mut self, ascender: i16, descender: i16) -> Face {
        self.ascender = ascender;
        self.descender = descender;
        self
    }

    /// The characters this face covers, sorted and deduplicated — which is the
    /// order glyph identifiers are assigned in, so a caller can predict them.
    #[must_use]
    pub fn glyph_order(&self) -> Vec<char> {
        let mut sorted: Vec<char> = self.covers.clone();
        sorted.sort_unstable();
        sorted.dedup();
        sorted
    }

    /// The glyph identifier this face's `cmap` will give a character.
    ///
    /// Glyph 0 is `.notdef` and the covered characters follow in sorted order,
    /// so a test can name the glyph it expects rather than reading it back out
    /// of the font it is testing.
    #[must_use]
    pub fn glyph_of(&self, ch: char) -> Option<u16> {
        let at = self.glyph_order().iter().position(|c| *c == ch)?;
        u16::try_from(at + 1).ok()
    }

    /// The program itself.
    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        build(self)
    }
}

/// A face covering exactly the characters of `covers`, and nothing else.
#[must_use]
pub fn covering(family: &str, covers: &str) -> Vec<u8> {
    Face::new(family, covers).build()
}

fn build(face: &Face) -> Vec<u8> {
    let order = face.glyph_order();
    // A ligature needs a glyph of its own, after every covered character, so
    // that `Face::glyph_of` keeps answering for the characters. A joining face
    // needs three more blocks of the same size after that, one per form, which
    // is what `Face::form_glyph` predicts.
    let glyph_count = order.len()
        + 1
        + usize::from(face.ligature.is_some())
        + if face.joining.is_some() {
            order.len() * Form::ALL.len()
        } else {
            0
        };

    // ---- glyf and loca ------------------------------------------------------
    //
    // Glyph 0 is `.notdef` and is **empty**: a glyph is empty precisely when
    // its `loca` entry equals the next one, which is what a reader draws as
    // nothing. Every covered character gets its own copy of the outline,
    // because `loca` gives each glyph its own slice and offsets that merely
    // repeat make every glyph empty.
    let outline = box_glyph(i16::try_from(face.units_per_em * 7 / 10).unwrap_or(700));
    let mut glyf = Vec::with_capacity(outline.len() * glyph_count);
    let mut loca = vec![0u32];
    for _ in 1..glyph_count {
        glyf.extend_from_slice(&outline);
        loca.push(u32::try_from(glyf.len()).unwrap_or(0));
    }
    let loca: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();

    // ---- head ---------------------------------------------------------------
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes()); // magicNumber
    head[18..20].copy_from_slice(&face.units_per_em.to_be_bytes());
    head[36..38].copy_from_slice(&0i16.to_be_bytes()); // xMin
    head[38..40].copy_from_slice(&0i16.to_be_bytes()); // yMin
    head[40..42].copy_from_slice(&(face.units_per_em as i16).to_be_bytes()); // xMax
    head[42..44].copy_from_slice(&(face.units_per_em as i16).to_be_bytes()); // yMax
    head[46..48].copy_from_slice(&8u16.to_be_bytes()); // lowestRecPPEM
    head[48..50].copy_from_slice(&2i16.to_be_bytes()); // fontDirectionHint
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    // ---- hhea ---------------------------------------------------------------
    //
    // The offsets are the specification's and are worth naming, because two of
    // them are read by code this repository owns: `ascender` at 4 and
    // `descender` at 6, **not** at 0 and 2, which is where the version is.
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    hhea[4..6].copy_from_slice(&face.ascender.to_be_bytes());
    hhea[6..8].copy_from_slice(&face.descender.to_be_bytes());
    hhea[8..10].copy_from_slice(&0i16.to_be_bytes()); // lineGap
    hhea[10..12].copy_from_slice(&face.advance.to_be_bytes()); // advanceWidthMax
    hhea[18..20].copy_from_slice(&1i16.to_be_bytes()); // caretSlopeRise
    hhea[34..36].copy_from_slice(&(glyph_count as u16).to_be_bytes()); // numberOfHMetrics

    // ---- hmtx ---------------------------------------------------------------
    //
    // A full entry per glyph, `.notdef` included: `numberOfHMetrics` equals the
    // glyph count, so nothing here depends on the trailing side-bearing form.
    //
    // A joining face may give its substituted forms a **different** advance,
    // and that is the only way a test can tell the shaped measurement from the
    // per-character one: with every glyph the same width the two agree by
    // arithmetic and a build that measured the wrong one would pass.
    let joined_from = order.len() + 1 + usize::from(face.ligature.is_some());
    let mut hmtx = Vec::with_capacity(glyph_count * 4);
    for glyph in 0..glyph_count {
        let advance = match face.joined_advance {
            Some(joined) if glyph >= joined_from => joined,
            _ => face.advance,
        };
        hmtx.extend_from_slice(&advance.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // leftSideBearing
    }

    // ---- maxp ---------------------------------------------------------------
    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&(glyph_count as u16).to_be_bytes());
    maxp[6..8].copy_from_slice(&4u16.to_be_bytes()); // maxPoints
    maxp[8..10].copy_from_slice(&1u16.to_be_bytes()); // maxContours

    let cmap = cmap_format_4(&order);
    let name = name_table(&face.family);

    let gsub = match (face.ligature, face.joining) {
        (Some(ligature), _) => Some(gsub_ligature(face, ligature)),
        (None, Some(joining)) => Some(gsub_joining(face, joining)),
        (None, None) => None,
    };
    let gpos = face
        .placement
        .map(|placement| gpos_placement(face, placement));

    // Built as a list rather than as two hard-coded arrays, because `GPOS` and
    // `GSUB` are independent: a face may have either, both or neither, and the
    // four cases written out would be four places to forget a table.
    // Alphabetical by tag, which is where the two upper-case ones sort.
    let mut tables: Vec<(&[u8; 4], &Vec<u8>)> = Vec::with_capacity(10);
    if let Some(gpos) = gpos.as_ref() {
        tables.push((b"GPOS", gpos));
    }
    if let Some(gsub) = gsub.as_ref() {
        tables.push((b"GSUB", gsub));
    }
    tables.extend([
        (b"cmap", &cmap),
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"maxp", &maxp),
        (b"name", &name),
    ]);
    assemble(&tables)
}

/// A `GPOS` with one `SinglePos` lookup that displaces one glyph.
///
/// # Why this is the smallest fixture that proves the offsets are carried
///
/// A mark-to-base fixture is what a *reader* pictures when it hears `GPOS`:
/// an `Anchor` on the base, an `Anchor` on the mark, a `MarkArray`, two
/// coverage tables and a class definition — around a hundred and fifty bytes
/// of hand-written table, and every one of those bytes is arithmetic this
/// repository already adjudicates. The aots corpus covers `GPOS` lookup types
/// 1 through 9 case by case, and `text-rendering-tests`' GPOS-3 and GPOS-4
/// sections settle mark-to-base and mark-to-mark against real faces with
/// their expected positions inside them.
///
/// What **no** fixture in this repository said, until this one, is that the
/// numbers those suites adjudicate survive the trip onto a page. So this
/// fixture is deliberately not about anchor arithmetic: it is a
/// `SinglePosFormat1` with an `XPlacement` and a `YPlacement`, about twenty
/// bytes of table, whose only job is to put a non-zero offset on a glyph the
/// EPUB writer will draw. If it reaches the content stream, every offset
/// does.
///
/// The layout, with each offset written from the start of the table or of its
/// own subtable as ISO/IEC 14496-22 requires:
///
/// | At | What |
/// | --- | --- |
/// | 0 | header: version 1.0, three offsets |
/// | 10 | `ScriptList`: one record, one `Script`, one default `LangSys` |
/// | 30 | `FeatureList`: one record, one `Feature`, one lookup index |
/// | 44 | `LookupList`: one `Lookup` of type 1, one subtable |
fn gpos_placement(face: &Face, placement: Placement) -> Vec<u8> {
    let glyph = face.glyph_of(placement.ch).unwrap_or(0);

    // ---- the subtable: `SinglePosFormat1` ----------------------------------
    //
    // `valueFormat` 0x0003 is `X_PLACEMENT | Y_PLACEMENT`, so the value record
    // is two `int16`s and the header is six bytes — which is why the coverage
    // begins at ten and not at eight.
    let mut subtable = Vec::new();
    subtable.extend_from_slice(&1u16.to_be_bytes()); // posFormat 1
    subtable.extend_from_slice(&10u16.to_be_bytes()); // coverage, from here
    subtable.extend_from_slice(&0x0003u16.to_be_bytes()); // valueFormat
    subtable.extend_from_slice(&placement.x.to_be_bytes()); // XPlacement
    subtable.extend_from_slice(&placement.y.to_be_bytes()); // YPlacement
    subtable.extend_from_slice(&1u16.to_be_bytes()); // coverage format 1
    subtable.extend_from_slice(&1u16.to_be_bytes()); // glyphCount
    subtable.extend_from_slice(&glyph.to_be_bytes());

    let mut lookup_list = Vec::new();
    lookup_list.extend_from_slice(&1u16.to_be_bytes()); // lookupCount
    lookup_list.extend_from_slice(&4u16.to_be_bytes()); // lookups[0]
    lookup_list.extend_from_slice(&1u16.to_be_bytes()); // lookupType: single
    lookup_list.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
    lookup_list.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
    lookup_list.extend_from_slice(&8u16.to_be_bytes()); // subtables[0]
    lookup_list.extend_from_slice(&subtable);

    // ---- one feature, holding that one lookup ------------------------------
    let mut feature_list = Vec::new();
    feature_list.extend_from_slice(&1u16.to_be_bytes()); // featureCount
    feature_list.extend_from_slice(&placement.feature);
    feature_list.extend_from_slice(&8u16.to_be_bytes()); // Feature, from here
    feature_list.extend_from_slice(&0u16.to_be_bytes()); // featureParams
    feature_list.extend_from_slice(&1u16.to_be_bytes()); // lookupIndexCount
    feature_list.extend_from_slice(&0u16.to_be_bytes()); // lookupListIndices[0]

    // ---- one script, one default language system, that one feature ---------
    let mut script_list = Vec::new();
    script_list.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    script_list.extend_from_slice(&placement.script);
    script_list.extend_from_slice(&8u16.to_be_bytes()); // Script, from the list
    script_list.extend_from_slice(&4u16.to_be_bytes()); // defaultLangSys
    script_list.extend_from_slice(&0u16.to_be_bytes()); // langSysCount
    script_list.extend_from_slice(&0u16.to_be_bytes()); // lookupOrderOffset
    script_list.extend_from_slice(&0xFFFFu16.to_be_bytes()); // requiredFeature
    script_list.extend_from_slice(&1u16.to_be_bytes()); // featureIndexCount
    script_list.extend_from_slice(&0u16.to_be_bytes()); // featureIndices[0]

    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    let script_at = 10usize;
    let feature_at = script_at + script_list.len();
    let lookup_at = feature_at + feature_list.len();
    for at in [script_at, feature_at, lookup_at] {
        out.extend_from_slice(&u16::try_from(at).expect("a small table").to_be_bytes());
    }
    out.extend_from_slice(&script_list);
    out.extend_from_slice(&feature_list);
    out.extend_from_slice(&lookup_list);
    out
}

/// A `GSUB` with one single-substitution lookup per joining form.
///
/// # Why the offsets are computed here and hand-written above
///
/// [`gsub_ligature`] states every offset as a literal because its table has
/// one of everything and the numbers can be read against the layout in its own
/// doc comment. This one has three features, three lookups and three coverage
/// tables whose sizes follow the face's glyph count, so a literal would be a
/// number nobody could check. What is fixed instead is the *shape*, and each
/// section is measured as it is built.
///
/// Every lookup is `SingleSubstFormat1`: one coverage listing the covered
/// glyphs, and one `deltaGlyphID` that lands on the right block. That works
/// only because [`Face::form_glyph`] lays the blocks out contiguously and in
/// the same order as the plain glyphs — which is the whole reason it does.
fn gsub_joining(face: &Face, joining: Joining) -> Vec<u8> {
    let order = face.glyph_order();
    let covered: Vec<u16> = (1..=order.len())
        .map(|g| u16::try_from(g).expect("a fixture face has few glyphs"))
        .collect();

    // ---- the three lookups, and the list that holds them --------------------
    //
    // A `Lookup` is six bytes of header plus its own array of subtable
    // offsets, so its one subtable can only begin at eight.
    let mut lookups: Vec<Vec<u8>> = Vec::new();
    for form in Form::ALL {
        let delta = face
            .form_glyph(order[0], form)
            .expect("a joining face gives every covered character a form")
            .wrapping_sub(covered[0]);
        let mut subtable = Vec::new();
        subtable.extend_from_slice(&1u16.to_be_bytes()); // substFormat 1
        subtable.extend_from_slice(&6u16.to_be_bytes()); // coverage, from here
        subtable.extend_from_slice(&delta.to_be_bytes()); // deltaGlyphID
        subtable.extend_from_slice(&1u16.to_be_bytes()); // coverage format 1
        subtable.extend_from_slice(
            &u16::try_from(covered.len())
                .expect("a fixture face has few glyphs")
                .to_be_bytes(),
        );
        for glyph in &covered {
            subtable.extend_from_slice(&glyph.to_be_bytes());
        }

        let mut lookup = Vec::new();
        lookup.extend_from_slice(&1u16.to_be_bytes()); // lookupType: single
        lookup.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
        lookup.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
        lookup.extend_from_slice(&8u16.to_be_bytes()); // subtables[0]
        lookup.extend_from_slice(&subtable);
        lookups.push(lookup);
    }

    let mut lookup_list = Vec::new();
    lookup_list.extend_from_slice(
        &u16::try_from(lookups.len())
            .expect("three lookups")
            .to_be_bytes(),
    );
    let mut at = 2 + lookups.len() * 2;
    for lookup in &lookups {
        lookup_list.extend_from_slice(&u16::try_from(at).expect("a small table").to_be_bytes());
        at += lookup.len();
    }
    for lookup in &lookups {
        lookup_list.extend_from_slice(lookup);
    }

    // ---- the three features -------------------------------------------------
    let mut feature_list = Vec::new();
    feature_list.extend_from_slice(&3u16.to_be_bytes()); // featureCount
    let mut at = 2 + 3 * 6;
    for (index, form) in Form::ALL.iter().enumerate() {
        feature_list.extend_from_slice(&form.tag());
        feature_list.extend_from_slice(&u16::try_from(at).expect("a small table").to_be_bytes());
        at += 6;
        let _ = index;
    }
    for (index, _) in Form::ALL.iter().enumerate() {
        feature_list.extend_from_slice(&0u16.to_be_bytes()); // featureParams
        feature_list.extend_from_slice(&1u16.to_be_bytes()); // lookupIndexCount
        feature_list
            .extend_from_slice(&u16::try_from(index).expect("three features").to_be_bytes());
    }

    // ---- one script, one default language system, all three features -------
    let mut script_list = Vec::new();
    script_list.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    script_list.extend_from_slice(&joining.script);
    script_list.extend_from_slice(&8u16.to_be_bytes()); // Script, from the list
    script_list.extend_from_slice(&4u16.to_be_bytes()); // defaultLangSys
    script_list.extend_from_slice(&0u16.to_be_bytes()); // langSysCount
    script_list.extend_from_slice(&0u16.to_be_bytes()); // lookupOrderOffset
    script_list.extend_from_slice(&0xFFFFu16.to_be_bytes()); // requiredFeature
    script_list.extend_from_slice(&3u16.to_be_bytes()); // featureIndexCount
    for index in 0..3u16 {
        script_list.extend_from_slice(&index.to_be_bytes());
    }

    let mut out = Vec::new();
    out.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
    out.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
    let script_at = 10usize;
    let feature_at = script_at + script_list.len();
    let lookup_at = feature_at + feature_list.len();
    out.extend_from_slice(
        &u16::try_from(script_at)
            .expect("a small table")
            .to_be_bytes(),
    );
    out.extend_from_slice(
        &u16::try_from(feature_at)
            .expect("a small table")
            .to_be_bytes(),
    );
    out.extend_from_slice(
        &u16::try_from(lookup_at)
            .expect("a small table")
            .to_be_bytes(),
    );
    out.extend_from_slice(&script_list);
    out.extend_from_slice(&feature_list);
    out.extend_from_slice(&lookup_list);
    out
}

/// A `GSUB` with exactly one ligature substitution, and nothing else.
///
/// The smallest table that makes shaping observable: one script, one language
/// system, one feature, one lookup, one `LigatureSet` with one `Ligature` in
/// it. Written out by hand rather than compiled, because a fixture that needed
/// a font compiler would not be a fixture — and because every offset here is
/// one this repository's own reader has to get right.
///
/// The layout, in order, with each offset written from the start of the table
/// or of its own subtable as ISO/IEC 14496-22 requires:
///
/// | At | What |
/// | --- | --- |
/// | 0 | header: version 1.0, three offsets |
/// | 10 | `ScriptList`: one record, one `Script`, one default `LangSys` |
/// | 30 | `FeatureList`: one record, one `Feature` naming lookup 0 |
/// | 44 | `LookupList`: one offset, one `Lookup` of type 4 |
/// | 56 | the `LigatureSubst` subtable, its coverage and its one ligature |
fn gsub_ligature(face: &Face, ligature: Ligature) -> Vec<u8> {
    let first = face.glyph_of(ligature.first).unwrap_or(0);
    let second = face.glyph_of(ligature.second).unwrap_or(0);
    let joined = face.ligature_glyph().unwrap_or(0);

    let mut out: Vec<u8> = Vec::new();
    // Header: major 1, minor 0, then the three list offsets.
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&10u16.to_be_bytes()); // scriptList
    out.extend_from_slice(&30u16.to_be_bytes()); // featureList
    out.extend_from_slice(&44u16.to_be_bytes()); // lookupList

    // ScriptList at 10: one record of six bytes, then the Script table.
    out.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    out.extend_from_slice(&ligature.script);
    out.extend_from_slice(&8u16.to_be_bytes()); // Script, from the list's start
                                                // Script at 18: a default LangSys and no named ones.
    out.extend_from_slice(&4u16.to_be_bytes()); // defaultLangSys, from Script
    out.extend_from_slice(&0u16.to_be_bytes()); // langSysCount
                                                // LangSys at 22: no required feature, one feature index.
    out.extend_from_slice(&0u16.to_be_bytes()); // lookupOrderOffset, always 0
    out.extend_from_slice(&0xFFFFu16.to_be_bytes()); // requiredFeatureIndex: none
    out.extend_from_slice(&1u16.to_be_bytes()); // featureIndexCount
    out.extend_from_slice(&0u16.to_be_bytes()); // featureIndices[0]

    // FeatureList at 30: one record of six bytes, then the Feature.
    out.extend_from_slice(&1u16.to_be_bytes()); // featureCount
    out.extend_from_slice(&ligature.feature);
    out.extend_from_slice(&8u16.to_be_bytes()); // Feature, from the list's start
                                                // Feature at 38.
    out.extend_from_slice(&0u16.to_be_bytes()); // featureParams
    out.extend_from_slice(&1u16.to_be_bytes()); // lookupIndexCount
    out.extend_from_slice(&0u16.to_be_bytes()); // lookupListIndices[0]

    // LookupList at 44.
    out.extend_from_slice(&1u16.to_be_bytes()); // lookupCount
    out.extend_from_slice(&4u16.to_be_bytes()); // Lookup, from the list's start
                                                // Lookup at 48: type 4, no flags, one subtable.
    out.extend_from_slice(&4u16.to_be_bytes()); // lookupType: ligature
    out.extend_from_slice(&0u16.to_be_bytes()); // lookupFlag
    out.extend_from_slice(&1u16.to_be_bytes()); // subTableCount
                                                // Eight and not six: the `Lookup` is six bytes of header plus its own
                                                // array of subtable offsets, so the first subtable can only begin after
                                                // that array — a six would point into the offset it was read from.
    out.extend_from_slice(&8u16.to_be_bytes()); // subtable, from the Lookup

    // LigatureSubst at 56.
    out.extend_from_slice(&1u16.to_be_bytes()); // substFormat
    out.extend_from_slice(&8u16.to_be_bytes()); // coverage, from the subtable
    out.extend_from_slice(&1u16.to_be_bytes()); // ligatureSetCount
    out.extend_from_slice(&14u16.to_be_bytes()); // ligatureSets[0]
                                                 // Coverage at 64: format 1, one glyph — the ligature first component.
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&first.to_be_bytes());
    // LigatureSet at 70.
    out.extend_from_slice(&1u16.to_be_bytes()); // ligatureCount
    out.extend_from_slice(&4u16.to_be_bytes()); // ligatures[0], from the set
                                                // Ligature at 74: the joined glyph, two components, the second named.
    out.extend_from_slice(&joined.to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes()); // componentCount
    out.extend_from_slice(&second.to_be_bytes()); // componentGlyphIDs[0]
    out
}

/// A format 4 subtable mapping each character of `order` to its index plus one.
///
/// One segment per **contiguous run** of characters, which is what lets
/// `idDelta` alone carry the mapping: within a run the codes rise by one and so
/// do the glyphs, so `glyph = code + delta` holds for the whole segment and no
/// `idRangeOffset` array is needed. A face covering three scattered characters
/// gets three segments and a face covering a range gets one, and both read
/// back the same.
fn cmap_format_4(order: &[char]) -> Vec<u8> {
    // Only the BMP: a format 4 subtable has nowhere to put anything else, and
    // a fixture that silently dropped an astral character would look like a
    // face that does not cover it.
    let mut codes: Vec<(u16, u16)> = Vec::new();
    for (at, ch) in order.iter().enumerate() {
        let code = u32::from(*ch);
        assert!(
            code < 0xFFFF,
            "a fixture face may only cover the BMP; {ch:?} is not in it"
        );
        let glyph = u16::try_from(at + 1).expect("a fixture face has few glyphs");
        codes.push((code as u16, glyph));
    }

    let mut segments: Vec<(u16, u16, u16)> = Vec::new(); // start, end, first glyph
    for (code, glyph) in codes {
        match segments.last_mut() {
            Some(last) if last.1 + 1 == code && last.2 + (last.1 - last.0) + 1 == glyph => {
                last.1 = code;
            }
            _ => segments.push((code, code, glyph)),
        }
    }
    // 9.6.6.4's terminator: the last segment must end at 0xFFFF, and this one
    // maps it to glyph 0.
    segments.push((0xFFFF, 0xFFFF, 1));

    let count = segments.len();
    let seg2 = u16::try_from(count * 2).expect("a fixture face has few segments");
    let mut entry_selector = 0u16;
    while 1u32 << (entry_selector + 1) <= count as u32 {
        entry_selector += 1;
    }
    let search_range = 2u16 * (1 << entry_selector);

    let mut sub = Vec::new();
    sub.extend_from_slice(&4u16.to_be_bytes()); // format
    sub.extend_from_slice(&0u16.to_be_bytes()); // length, filled in below
    sub.extend_from_slice(&0u16.to_be_bytes()); // language
    sub.extend_from_slice(&seg2.to_be_bytes());
    sub.extend_from_slice(&search_range.to_be_bytes());
    sub.extend_from_slice(&entry_selector.to_be_bytes());
    sub.extend_from_slice(&(seg2 - search_range).to_be_bytes()); // rangeShift
    for (_, end, _) in &segments {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for (start, _, _) in &segments {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for (start, _, glyph) in &segments {
        sub.extend_from_slice(&glyph.wrapping_sub(*start).to_be_bytes()); // idDelta
    }
    for _ in &segments {
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }
    let length = u16::try_from(sub.len()).expect("a fixture subtable is small");
    sub[2..4].copy_from_slice(&length.to_be_bytes());

    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // version
    table.extend_from_slice(&1u16.to_be_bytes()); // numTables
    table.extend_from_slice(&3u16.to_be_bytes()); // platformID: Windows
    table.extend_from_slice(&1u16.to_be_bytes()); // encodingID: Unicode BMP
    table.extend_from_slice(&12u32.to_be_bytes()); // offset
    table.extend_from_slice(&sub);
    table
}

/// A `name` table carrying one record: `nameID` 1, the family, in UTF-16BE.
fn name_table(family: &str) -> Vec<u8> {
    let text: Vec<u8> = family
        .encode_utf16()
        .flat_map(|unit| unit.to_be_bytes())
        .collect();
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    table.extend_from_slice(&1u16.to_be_bytes()); // count
    table.extend_from_slice(&18u16.to_be_bytes()); // stringOffset: 6 + 1 * 12
    table.extend_from_slice(&3u16.to_be_bytes()); // platformID: Windows
    table.extend_from_slice(&1u16.to_be_bytes()); // encodingID: UCS-2
    table.extend_from_slice(&0x0409u16.to_be_bytes()); // languageID
    table.extend_from_slice(&1u16.to_be_bytes()); // nameID: family
    table.extend_from_slice(&(text.len() as u16).to_be_bytes());
    table.extend_from_slice(&0u16.to_be_bytes()); // offset into the storage
    table.extend_from_slice(&text);
    table
}

/// The table directory and the tables, each padded to a four-byte boundary.
///
/// The tags are written in the order the caller gave them, which every caller
/// here gives alphabetically — the order a real sfnt uses and the one a reader
/// that binary-searched the directory would need.
fn assemble(tables: &[(&[u8; 4], &Vec<u8>)]) -> Vec<u8> {
    let count = tables.len();
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfntVersion
    out.extend_from_slice(&(count as u16).to_be_bytes());
    let mut entry_selector = 0u16;
    while 1u32 << (entry_selector + 1) <= count as u32 {
        entry_selector += 1;
    }
    let search_range = 16u16 * (1 << entry_selector);
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&((count as u16 * 16).wrapping_sub(search_range)).to_be_bytes());

    let mut offset = 12 + count * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + count * 16 + body.len();
    }
    out.extend_from_slice(&body);
    out
}

/// A table's checksum: the sum of its big-endian 32-bit words, wrapping.
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let mut chunk = [0u8; 4];
    for (at, byte) in data.iter().enumerate() {
        chunk[at % 4] = *byte;
        if at % 4 == 3 {
            sum = sum.wrapping_add(u32::from_be_bytes(chunk));
            chunk = [0; 4];
        }
    }
    if data.len() % 4 != 0 {
        for slot in chunk.iter_mut().skip(data.len() % 4) {
            *slot = 0;
        }
        sum = sum.wrapping_add(u32::from_be_bytes(chunk));
    }
    sum
}

/// A TrueType face whose every glyph from 32 upward is one filled box, reached
/// **by glyph identifier** and not through a `cmap`.
///
/// `tests/substitute_fonts.rs` records why it is synthesised rather than read
/// from the system. It is here rather than copied into each test binary that
/// wants it, and it is deliberately **not** what [`covering`] builds: this face
/// answers *"did a glyph get drawn"* for a [`tinker_pdf::FontProvider`], which
/// reaches a glyph by the code a document wrote and asks no `cmap` anything, so
/// giving it one would test nothing and change what four existing tests mean.
#[must_use]
pub fn boxy_font() -> Vec<u8> {
    let glyph = box_glyph(700);

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    const FIRST: usize = 32;
    const LAST: usize = 255;
    let size = glyph.len() as u32;

    let mut glyf = Vec::with_capacity(glyph.len() * (LAST + 1 - FIRST));
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }
    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        let offset = (index.saturating_sub(FIRST)) as u32 * size;
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

/// Where one text object's `ET` operator starts, or `None` for an object that
/// was never closed.
///
/// # Why this is a scan and not `find(" ET")`
///
/// It was `find(" ET")` and that was a **silent-pass hazard**, not a
/// simplification. `DocumentBuilder::glyph_run` ends a run `] TJ\nET\n`, with
/// a newline before the `ET` and none after — so every shaped text object was
/// invisible to this helper, and a test that asserted something about the
/// objects it found passed by finding none. The two callers that assert a
/// *count* fail loudly; a `contains` over a resource name would not have.
///
/// The scan also steps over strings, which the substring search did not: a
/// book with the word `GET` in it writes `(GET) Tj`, and a search for `" ET"`
/// would have ended the object in the middle of its own text.
fn end_of_text_object(body: &str) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut at = 0usize;
    // 7.3.4.2's literal strings nest, and a `\` escapes the next byte
    // whatever it is.
    let mut depth = 0u32;
    let mut escaped = false;
    while at < bytes.len() {
        let byte = bytes[at];
        if depth > 0 {
            if escaped {
                escaped = false;
            } else {
                match byte {
                    b'\\' => escaped = true,
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
            }
            at += 1;
            continue;
        }
        match byte {
            b'(' => depth = 1,
            // A hex string holds only hex digits and white space, so `ET`
            // cannot appear in one — stepping over it is belt and braces
            // rather than a fix.
            b'<' if bytes.get(at + 1) != Some(&b'<') => {
                while at < bytes.len() && bytes[at] != b'>' {
                    at += 1;
                }
            }
            b'E' if bytes.get(at + 1) == Some(&b'T') => {
                let before = at == 0 || bytes[at - 1].is_ascii_whitespace();
                let after = bytes.get(at + 2).is_none_or(u8::is_ascii_whitespace);
                if before && after {
                    return Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// Every `BT … ET` text object of a content stream, in the order they were
/// written, as `(resource, operators)`.
///
/// A parse rather than a substring search, because the count is the assertion:
/// `a_run_needing_three_faces_becomes_three_text_objects` fails if a build
/// draws one object or five, and a `contains` over three resource names passes
/// on both.
#[must_use]
pub fn text_objects(content: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(at) = rest.find("BT /") {
        let body = &rest[at + 4..];
        let Some(end) = end_of_text_object(body) else {
            break;
        };
        let object = body[..end].trim_end();
        let resource = object
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        out.push((resource, object.to_owned()));
        rest = &body[end + 2..];
    }
    out
}

/// The `x` and `y` one text object starts at.
///
/// `Td` and `Tm` are the two positioning operators this build writes into a
/// text object — the coded path writes `Td` and the shaped path writes a whole
/// `Tm`, because `DocumentBuilder::glyph_run` states the run's matrix — and in
/// both the two operands before the operator are the translation. So the pair
/// before whichever appears is the origin, and a helper that only knew `Td`
/// would panic on every shaped object rather than answer about it.
#[must_use]
pub fn origin_of(object: &str) -> (f64, f64) {
    let words: Vec<&str> = object.split_whitespace().collect();
    let at = words
        .iter()
        .position(|word| *word == "Td" || *word == "Tm")
        .unwrap_or_else(|| panic!("no Td or Tm in {object:?}"));
    let x = words[at - 2].parse().expect("an x before the operator");
    let y = words[at - 1].parse().expect("a y before the operator");
    (x, y)
}

/// Every hex string one text object shows, concatenated — the glyph indices it
/// draws, in the order it draws them.
///
/// **Every** one, and that is the point. A `PageBuilder::glyphs` object shows
/// one hex string and a `glyph_run` object shows a `TJ` array of them broken
/// wherever a glyph asked for a different position, so a reader that took the
/// first would see the run up to its first `GPOS` offset and call it the run.
/// A helper that is right for one writer and quietly short for the other is
/// how a suite stops noticing.
#[must_use]
pub fn shown_glyphs(object: &str) -> String {
    let mut out = String::new();
    let mut rest = object;
    while let Some(at) = rest.find('<') {
        let body = &rest[at + 1..];
        let Some(end) = body.find('>') else { break };
        out.push_str(&body[..end]);
        rest = &body[end + 1..];
    }
    out
}

/// Wraps an sfnt in a WOFF 1.0 container, storing every table uncompressed.
///
/// §5 lets a producer store a table rather than deflate it, and says how a
/// decoder tells: `compLength == origLength` means the bytes are the table.
/// Real producers do it for tables compression would not help; this does it
/// for all of them, because the alternative is a zlib **encoder** and there is
/// none in this tree (`CONTRIBUTING.md` rule 1, and nothing else here would
/// want one).
///
/// So this is a container a decoder has to read correctly without ever
/// inflating anything, which makes it exactly the right shape for the EPUB
/// tests: they are about a face reaching a page, and the compression is
/// `crates/tinker-pdf-font/tests/woff_fixtures.rs`'s subject, held against
/// files two real encoders wrote.
///
/// The directory is written in ascending tag order, which §5 requires of a
/// producer as well as of a decoder, and the tables are laid down in the order
/// the input had them so that the round trip is byte identity.
#[must_use]
pub fn as_woff(sfnt: &[u8]) -> Vec<u8> {
    let count = usize::from(u16::from_be_bytes([sfnt[4], sfnt[5]]));
    let mut entries: Vec<(u32, usize, usize)> = Vec::with_capacity(count);
    for i in 0..count {
        let at = 12 + i * 16;
        let tag = u32::from_be_bytes([sfnt[at], sfnt[at + 1], sfnt[at + 2], sfnt[at + 3]]);
        let offset =
            u32::from_be_bytes([sfnt[at + 8], sfnt[at + 9], sfnt[at + 10], sfnt[at + 11]]) as usize;
        let length =
            u32::from_be_bytes([sfnt[at + 12], sfnt[at + 13], sfnt[at + 14], sfnt[at + 15]])
                as usize;
        entries.push((tag, offset, length));
    }
    // The physical order is what the container records; the directory is
    // sorted by tag. Writing the entries out of one and the blocks out of the
    // other is the whole of what makes the round trip reproduce the input.
    let mut physical = entries.clone();
    physical.sort_by_key(|&(_, offset, _)| offset);
    entries.sort_by_key(|&(tag, _, _)| tag);

    let header = 44 + count * 20;
    let mut blocks: Vec<(u32, usize, usize)> = Vec::with_capacity(count);
    let mut body = Vec::new();
    for &(tag, offset, length) in &physical {
        let at = header + body.len();
        body.extend_from_slice(&sfnt[offset..offset + length]);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        blocks.push((tag, at, length));
    }

    let mut sfnt_size = 12 + count * 16;
    let mut out = vec![0u8; header];
    out[0..4].copy_from_slice(b"wOFF");
    out[4..8].copy_from_slice(&sfnt[0..4]);
    out[12..14].copy_from_slice(&u16::try_from(count).unwrap_or(0).to_be_bytes());
    for (i, &(tag, _, length)) in entries.iter().enumerate() {
        let (_, at, _) = *blocks
            .iter()
            .find(|&&(other, _, _)| other == tag)
            .expect("every table is placed");
        let entry = 44 + i * 20;
        out[entry..entry + 4].copy_from_slice(&tag.to_be_bytes());
        out[entry + 4..entry + 8].copy_from_slice(&u32::try_from(at).unwrap_or(0).to_be_bytes());
        // Stored, not deflated: §5's own signal for it is these two being equal.
        let length32 = u32::try_from(length).unwrap_or(0);
        out[entry + 8..entry + 12].copy_from_slice(&length32.to_be_bytes());
        out[entry + 12..entry + 16].copy_from_slice(&length32.to_be_bytes());
        let (_, offset, _) = *physical
            .iter()
            .find(|&&(other, _, _)| other == tag)
            .expect("every table is in the input");
        out[entry + 16..entry + 20]
            .copy_from_slice(&woff_checksum(tag, &sfnt[offset..offset + length]).to_be_bytes());
        sfnt_size += (length + 3) & !3;
    }
    out[16..20].copy_from_slice(&u32::try_from(sfnt_size).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&body);
    let total = u32::try_from(out.len()).unwrap_or(0);
    out[8..12].copy_from_slice(&total.to_be_bytes());
    out
}

/// A table's checksum as an sfnt directory states it: the sum of its
/// big-endian words, with `head`'s `checkSumAdjustment` taken as zero.
fn woff_checksum(tag: u32, data: &[u8]) -> u32 {
    let mut owned;
    let data = if tag == 0x6865_6164 && data.len() >= 12 {
        owned = data.to_vec();
        owned[8..12].fill(0);
        &owned[..]
    } else {
        data
    };
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}
