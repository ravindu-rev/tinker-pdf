//! FT-16.4: the font measurement API held to each committed face's own
//! tables (`docs/design/font-metrics.md`, *The independent reading the test
//! holds each answer to*).
//!
//! The reader below shares no code with `tinker-pdf-font`: its table
//! directory, `head`, `hhea`, `hmtx`, `maxp`, `loca`, `glyf`, `cmap`, `post`
//! and `OS/2` readers, its CFF INDEX, DICT and charset readers and Type 2
//! charstring interpreter, and its eexec decryption and Type 1 charstring
//! interpreter are written here from the formats' specifications — the
//! OpenType specification's chapters, TN #5176, TN #5177, the Type 1 book
//! and TN #5015's section 3.12 — and its name tables are transcribed from
//! them: TN
//! #5176's Appendix A, the `post` chapter's standard Macintosh order, and
//! ISO 32000-2's Table D.2. What the API answers for each face is held to
//! what that reader reads, answer by answer and glyph by glyph; a number the
//! two compute by the design's arithmetic is held within 10⁻⁹ of its
//! magnitude, the rounding of the same operations taken in another order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use tinker_pdf::{Document, FontFormat, FontMetrics, FontMetricsError, Rect};

// ---- the faces ----------------------------------------------------------

/// The repository's root, from this crate's manifest.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every file of a directory, sorted by name, as `(label, bytes)`.
fn files(dir: &str) -> Vec<(String, Vec<u8>)> {
    let dir = root().join(dir);
    let mut out: Vec<(String, Vec<u8>)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            // Licence and provenance files sit beside the faces.
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            !name.is_some_and(|n| {
                n.ends_with(".txt") || n.ends_with(".md") || n == "LICENSE" || n == "AUTHORS"
            })
        })
        .map(|path| {
            let label = path
                .strip_prefix(root())
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{label}: {e}"));
            (label, bytes)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The committed faces FT-16.4's exit names.
fn committed_faces() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for dir in [
        "crates/tinker-pdf-font/data/liberation",
        "crates/tinker-pdf-shape/data/text-rendering-tests/fonts",
        "crates/tinker-pdf-shape/data/aots/fonts",
        "fuzz/corpus/cff",
        "fuzz/corpus/type1",
    ] {
        out.extend(files(dir));
    }
    out
}

// ---- reading bytes -------------------------------------------------------

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *data.get(at)?,
        *data.get(at.checked_add(1)?)?,
    ]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ---- the names, transcribed ------------------------------------------------

/// TN #5176's Appendix A: the 391 standard strings, by SID.
const STANDARD_STRINGS: [&str; 391] = [
    ".notdef",
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quoteright",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "quoteleft",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "exclamdown",
    "cent",
    "sterling",
    "fraction",
    "yen",
    "florin",
    "section",
    "currency",
    "quotesingle",
    "quotedblleft",
    "guillemotleft",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "endash",
    "dagger",
    "daggerdbl",
    "periodcentered",
    "paragraph",
    "bullet",
    "quotesinglbase",
    "quotedblbase",
    "quotedblright",
    "guillemotright",
    "ellipsis",
    "perthousand",
    "questiondown",
    "grave",
    "acute",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "dieresis",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "emdash",
    "AE",
    "ordfeminine",
    "Lslash",
    "Oslash",
    "OE",
    "ordmasculine",
    "ae",
    "dotlessi",
    "lslash",
    "oslash",
    "oe",
    "germandbls",
    "onesuperior",
    "logicalnot",
    "mu",
    "trademark",
    "Eth",
    "onehalf",
    "plusminus",
    "Thorn",
    "onequarter",
    "divide",
    "brokenbar",
    "degree",
    "thorn",
    "threequarters",
    "twosuperior",
    "registered",
    "minus",
    "eth",
    "multiply",
    "threesuperior",
    "copyright",
    "Aacute",
    "Acircumflex",
    "Adieresis",
    "Agrave",
    "Aring",
    "Atilde",
    "Ccedilla",
    "Eacute",
    "Ecircumflex",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Ntilde",
    "Oacute",
    "Ocircumflex",
    "Odieresis",
    "Ograve",
    "Otilde",
    "Scaron",
    "Uacute",
    "Ucircumflex",
    "Udieresis",
    "Ugrave",
    "Yacute",
    "Ydieresis",
    "Zcaron",
    "aacute",
    "acircumflex",
    "adieresis",
    "agrave",
    "aring",
    "atilde",
    "ccedilla",
    "eacute",
    "ecircumflex",
    "edieresis",
    "egrave",
    "iacute",
    "icircumflex",
    "idieresis",
    "igrave",
    "ntilde",
    "oacute",
    "ocircumflex",
    "odieresis",
    "ograve",
    "otilde",
    "scaron",
    "uacute",
    "ucircumflex",
    "udieresis",
    "ugrave",
    "yacute",
    "ydieresis",
    "zcaron",
    "exclamsmall",
    "Hungarumlautsmall",
    "dollaroldstyle",
    "dollarsuperior",
    "ampersandsmall",
    "Acutesmall",
    "parenleftsuperior",
    "parenrightsuperior",
    "twodotenleader",
    "onedotenleader",
    "zerooldstyle",
    "oneoldstyle",
    "twooldstyle",
    "threeoldstyle",
    "fouroldstyle",
    "fiveoldstyle",
    "sixoldstyle",
    "sevenoldstyle",
    "eightoldstyle",
    "nineoldstyle",
    "commasuperior",
    "threequartersemdash",
    "periodsuperior",
    "questionsmall",
    "asuperior",
    "bsuperior",
    "centsuperior",
    "dsuperior",
    "esuperior",
    "isuperior",
    "lsuperior",
    "msuperior",
    "nsuperior",
    "osuperior",
    "rsuperior",
    "ssuperior",
    "tsuperior",
    "ff",
    "ffi",
    "ffl",
    "parenleftinferior",
    "parenrightinferior",
    "Circumflexsmall",
    "hyphensuperior",
    "Gravesmall",
    "Asmall",
    "Bsmall",
    "Csmall",
    "Dsmall",
    "Esmall",
    "Fsmall",
    "Gsmall",
    "Hsmall",
    "Ismall",
    "Jsmall",
    "Ksmall",
    "Lsmall",
    "Msmall",
    "Nsmall",
    "Osmall",
    "Psmall",
    "Qsmall",
    "Rsmall",
    "Ssmall",
    "Tsmall",
    "Usmall",
    "Vsmall",
    "Wsmall",
    "Xsmall",
    "Ysmall",
    "Zsmall",
    "colonmonetary",
    "onefitted",
    "rupiah",
    "Tildesmall",
    "exclamdownsmall",
    "centoldstyle",
    "Lslashsmall",
    "Scaronsmall",
    "Zcaronsmall",
    "Dieresissmall",
    "Brevesmall",
    "Caronsmall",
    "Dotaccentsmall",
    "Macronsmall",
    "figuredash",
    "hypheninferior",
    "Ogoneksmall",
    "Ringsmall",
    "Cedillasmall",
    "questiondownsmall",
    "oneeighth",
    "threeeighths",
    "fiveeighths",
    "seveneighths",
    "onethird",
    "twothirds",
    "zerosuperior",
    "foursuperior",
    "fivesuperior",
    "sixsuperior",
    "sevensuperior",
    "eightsuperior",
    "ninesuperior",
    "zeroinferior",
    "oneinferior",
    "twoinferior",
    "threeinferior",
    "fourinferior",
    "fiveinferior",
    "sixinferior",
    "seveninferior",
    "eightinferior",
    "nineinferior",
    "centinferior",
    "dollarinferior",
    "periodinferior",
    "commainferior",
    "Agravesmall",
    "Aacutesmall",
    "Acircumflexsmall",
    "Atildesmall",
    "Adieresissmall",
    "Aringsmall",
    "AEsmall",
    "Ccedillasmall",
    "Egravesmall",
    "Eacutesmall",
    "Ecircumflexsmall",
    "Edieresissmall",
    "Igravesmall",
    "Iacutesmall",
    "Icircumflexsmall",
    "Idieresissmall",
    "Ethsmall",
    "Ntildesmall",
    "Ogravesmall",
    "Oacutesmall",
    "Ocircumflexsmall",
    "Otildesmall",
    "Odieresissmall",
    "OEsmall",
    "Oslashsmall",
    "Ugravesmall",
    "Uacutesmall",
    "Ucircumflexsmall",
    "Udieresissmall",
    "Yacutesmall",
    "Thornsmall",
    "Ydieresissmall",
    "001.000",
    "001.001",
    "001.002",
    "001.003",
    "Black",
    "Bold",
    "Book",
    "Light",
    "Medium",
    "Regular",
    "Roman",
    "Semibold",
];

/// The standard Macintosh glyph order of the `post` chapter: version 1.0's
/// names, and version 2.0's below 258.
const MAC_NAMES: [&str; 258] = [
    ".notdef",
    ".null",
    "nonmarkingreturn",
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quotesingle",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "grave",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "Adieresis",
    "Aring",
    "Ccedilla",
    "Eacute",
    "Ntilde",
    "Odieresis",
    "Udieresis",
    "aacute",
    "agrave",
    "acircumflex",
    "adieresis",
    "atilde",
    "aring",
    "ccedilla",
    "eacute",
    "egrave",
    "ecircumflex",
    "edieresis",
    "iacute",
    "igrave",
    "icircumflex",
    "idieresis",
    "ntilde",
    "oacute",
    "ograve",
    "ocircumflex",
    "odieresis",
    "otilde",
    "uacute",
    "ugrave",
    "ucircumflex",
    "udieresis",
    "dagger",
    "degree",
    "cent",
    "sterling",
    "section",
    "bullet",
    "paragraph",
    "germandbls",
    "registered",
    "copyright",
    "trademark",
    "acute",
    "dieresis",
    "notequal",
    "AE",
    "Oslash",
    "infinity",
    "plusminus",
    "lessequal",
    "greaterequal",
    "yen",
    "mu",
    "partialdiff",
    "summation",
    "product",
    "pi",
    "integral",
    "ordfeminine",
    "ordmasculine",
    "Omega",
    "ae",
    "oslash",
    "questiondown",
    "exclamdown",
    "logicalnot",
    "radical",
    "florin",
    "approxequal",
    "Delta",
    "guillemotleft",
    "guillemotright",
    "ellipsis",
    "nonbreakingspace",
    "Agrave",
    "Atilde",
    "Otilde",
    "OE",
    "oe",
    "endash",
    "emdash",
    "quotedblleft",
    "quotedblright",
    "quoteleft",
    "quoteright",
    "divide",
    "lozenge",
    "ydieresis",
    "Ydieresis",
    "fraction",
    "currency",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "daggerdbl",
    "periodcentered",
    "quotesinglbase",
    "quotedblbase",
    "perthousand",
    "Acircumflex",
    "Ecircumflex",
    "Aacute",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Oacute",
    "Ocircumflex",
    "apple",
    "Ograve",
    "Uacute",
    "Ucircumflex",
    "Ugrave",
    "dotlessi",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "Lslash",
    "lslash",
    "Scaron",
    "scaron",
    "Zcaron",
    "zcaron",
    "brokenbar",
    "Eth",
    "eth",
    "Yacute",
    "yacute",
    "Thorn",
    "thorn",
    "minus",
    "multiply",
    "onesuperior",
    "twosuperior",
    "threesuperior",
    "onehalf",
    "onequarter",
    "threequarters",
    "franc",
    "Gbreve",
    "gbreve",
    "Idotaccent",
    "Scedilla",
    "scedilla",
    "Cacute",
    "cacute",
    "Ccaron",
    "ccaron",
    "dcroat",
];

/// ISO 32000-2's Table D.2, the Standard Latin character set: each name and
/// the character its CHAR column shows. The table's own footnote markers are
/// not part of a name, and the notes it closes with give `space` and `hyphen`
/// second codes that name no character of their own.
const STANDARD_LATIN: [(&str, char); 229] = [
    ("A", '\u{0041}'),
    ("AE", '\u{00C6}'),
    ("Aacute", '\u{00C1}'),
    ("Acircumflex", '\u{00C2}'),
    ("Adieresis", '\u{00C4}'),
    ("Agrave", '\u{00C0}'),
    ("Aring", '\u{00C5}'),
    ("Atilde", '\u{00C3}'),
    ("B", '\u{0042}'),
    ("C", '\u{0043}'),
    ("Ccedilla", '\u{00C7}'),
    ("D", '\u{0044}'),
    ("E", '\u{0045}'),
    ("Eacute", '\u{00C9}'),
    ("Ecircumflex", '\u{00CA}'),
    ("Edieresis", '\u{00CB}'),
    ("Egrave", '\u{00C8}'),
    ("Eth", '\u{00D0}'),
    ("Euro", '\u{20AC}'),
    ("F", '\u{0046}'),
    ("G", '\u{0047}'),
    ("H", '\u{0048}'),
    ("I", '\u{0049}'),
    ("Iacute", '\u{00CD}'),
    ("Icircumflex", '\u{00CE}'),
    ("Idieresis", '\u{00CF}'),
    ("Igrave", '\u{00CC}'),
    ("J", '\u{004A}'),
    ("K", '\u{004B}'),
    ("L", '\u{004C}'),
    ("Lslash", '\u{0141}'),
    ("M", '\u{004D}'),
    ("N", '\u{004E}'),
    ("Ntilde", '\u{00D1}'),
    ("O", '\u{004F}'),
    ("OE", '\u{0152}'),
    ("Oacute", '\u{00D3}'),
    ("Ocircumflex", '\u{00D4}'),
    ("Odieresis", '\u{00D6}'),
    ("Ograve", '\u{00D2}'),
    ("Oslash", '\u{00D8}'),
    ("Otilde", '\u{00D5}'),
    ("P", '\u{0050}'),
    ("Q", '\u{0051}'),
    ("R", '\u{0052}'),
    ("S", '\u{0053}'),
    ("Scaron", '\u{0160}'),
    ("T", '\u{0054}'),
    ("Thorn", '\u{00DE}'),
    ("U", '\u{0055}'),
    ("Uacute", '\u{00DA}'),
    ("Ucircumflex", '\u{00DB}'),
    ("Udieresis", '\u{00DC}'),
    ("Ugrave", '\u{00D9}'),
    ("V", '\u{0056}'),
    ("W", '\u{0057}'),
    ("X", '\u{0058}'),
    ("Y", '\u{0059}'),
    ("Yacute", '\u{00DD}'),
    ("Ydieresis", '\u{0178}'),
    ("Z", '\u{005A}'),
    ("Zcaron", '\u{017D}'),
    ("a", '\u{0061}'),
    ("aacute", '\u{00E1}'),
    ("acircumflex", '\u{00E2}'),
    ("acute", '\u{00B4}'),
    ("adieresis", '\u{00E4}'),
    ("ae", '\u{00E6}'),
    ("agrave", '\u{00E0}'),
    ("ampersand", '\u{0026}'),
    ("aring", '\u{00E5}'),
    ("asciicircum", '\u{005E}'),
    ("asciitilde", '\u{007E}'),
    ("asterisk", '\u{002A}'),
    ("at", '\u{0040}'),
    ("atilde", '\u{00E3}'),
    ("b", '\u{0062}'),
    ("backslash", '\u{005C}'),
    ("bar", '\u{007C}'),
    ("braceleft", '\u{007B}'),
    ("braceright", '\u{007D}'),
    ("bracketleft", '\u{005B}'),
    ("bracketright", '\u{005D}'),
    ("breve", '\u{02D8}'),
    ("brokenbar", '\u{00A6}'),
    ("bullet", '\u{2022}'),
    ("c", '\u{0063}'),
    ("caron", '\u{02C7}'),
    ("ccedilla", '\u{00E7}'),
    ("cedilla", '\u{00B8}'),
    ("cent", '\u{00A2}'),
    ("circumflex", '\u{02C6}'),
    ("colon", '\u{003A}'),
    ("comma", '\u{002C}'),
    ("copyright", '\u{00A9}'),
    ("currency", '\u{00A4}'),
    ("d", '\u{0064}'),
    ("dagger", '\u{2020}'),
    ("daggerdbl", '\u{2021}'),
    ("degree", '\u{00B0}'),
    ("dieresis", '\u{00A8}'),
    ("divide", '\u{00F7}'),
    ("dollar", '\u{0024}'),
    ("dotaccent", '\u{02D9}'),
    ("dotlessi", '\u{0131}'),
    ("e", '\u{0065}'),
    ("eacute", '\u{00E9}'),
    ("ecircumflex", '\u{00EA}'),
    ("edieresis", '\u{00EB}'),
    ("egrave", '\u{00E8}'),
    ("eight", '\u{0038}'),
    ("ellipsis", '\u{2026}'),
    ("emdash", '\u{2014}'),
    ("endash", '\u{2013}'),
    ("equal", '\u{003D}'),
    ("eth", '\u{00F0}'),
    ("exclam", '\u{0021}'),
    ("exclamdown", '\u{00A1}'),
    ("f", '\u{0066}'),
    ("fi", '\u{FB01}'),
    ("five", '\u{0035}'),
    ("fl", '\u{FB02}'),
    ("florin", '\u{0192}'),
    ("four", '\u{0034}'),
    ("fraction", '\u{2044}'),
    ("g", '\u{0067}'),
    ("germandbls", '\u{00DF}'),
    ("grave", '\u{0060}'),
    ("greater", '\u{003E}'),
    ("guillemotleft", '\u{00AB}'),
    ("guillemotright", '\u{00BB}'),
    ("guilsinglleft", '\u{2039}'),
    ("guilsinglright", '\u{203A}'),
    ("h", '\u{0068}'),
    ("hungarumlaut", '\u{02DD}'),
    ("hyphen", '\u{002D}'),
    ("i", '\u{0069}'),
    ("iacute", '\u{00ED}'),
    ("icircumflex", '\u{00EE}'),
    ("idieresis", '\u{00EF}'),
    ("igrave", '\u{00EC}'),
    ("j", '\u{006A}'),
    ("k", '\u{006B}'),
    ("l", '\u{006C}'),
    ("less", '\u{003C}'),
    ("logicalnot", '\u{00AC}'),
    ("lslash", '\u{0142}'),
    ("m", '\u{006D}'),
    ("macron", '\u{00AF}'),
    ("minus", '\u{2212}'),
    ("mu", '\u{00B5}'),
    ("multiply", '\u{00D7}'),
    ("n", '\u{006E}'),
    ("nine", '\u{0039}'),
    ("ntilde", '\u{00F1}'),
    ("numbersign", '\u{0023}'),
    ("o", '\u{006F}'),
    ("oacute", '\u{00F3}'),
    ("ocircumflex", '\u{00F4}'),
    ("odieresis", '\u{00F6}'),
    ("oe", '\u{0153}'),
    ("ogonek", '\u{02DB}'),
    ("ograve", '\u{00F2}'),
    ("one", '\u{0031}'),
    ("onehalf", '\u{00BD}'),
    ("onequarter", '\u{00BC}'),
    ("onesuperior", '\u{00B9}'),
    ("ordfeminine", '\u{00AA}'),
    ("ordmasculine", '\u{00BA}'),
    ("oslash", '\u{00F8}'),
    ("otilde", '\u{00F5}'),
    ("p", '\u{0070}'),
    ("paragraph", '\u{00B6}'),
    ("parenleft", '\u{0028}'),
    ("parenright", '\u{0029}'),
    ("percent", '\u{0025}'),
    ("period", '\u{002E}'),
    ("periodcentered", '\u{00B7}'),
    ("perthousand", '\u{2030}'),
    ("plus", '\u{002B}'),
    ("plusminus", '\u{00B1}'),
    ("q", '\u{0071}'),
    ("question", '\u{003F}'),
    ("questiondown", '\u{00BF}'),
    ("quotedbl", '\u{0022}'),
    ("quotedblbase", '\u{201E}'),
    ("quotedblleft", '\u{201C}'),
    ("quotedblright", '\u{201D}'),
    ("quoteleft", '\u{2018}'),
    ("quoteright", '\u{2019}'),
    ("quotesinglbase", '\u{201A}'),
    ("quotesingle", '\u{0027}'),
    ("r", '\u{0072}'),
    ("registered", '\u{00AE}'),
    ("ring", '\u{02DA}'),
    ("s", '\u{0073}'),
    ("scaron", '\u{0161}'),
    ("section", '\u{00A7}'),
    ("semicolon", '\u{003B}'),
    ("seven", '\u{0037}'),
    ("six", '\u{0036}'),
    ("slash", '\u{002F}'),
    ("space", '\u{0020}'),
    ("sterling", '\u{00A3}'),
    ("t", '\u{0074}'),
    ("thorn", '\u{00FE}'),
    ("three", '\u{0033}'),
    ("threequarters", '\u{00BE}'),
    ("threesuperior", '\u{00B3}'),
    ("tilde", '\u{02DC}'),
    ("trademark", '\u{2122}'),
    ("two", '\u{0032}'),
    ("twosuperior", '\u{00B2}'),
    ("u", '\u{0075}'),
    ("uacute", '\u{00FA}'),
    ("ucircumflex", '\u{00FB}'),
    ("udieresis", '\u{00FC}'),
    ("ugrave", '\u{00F9}'),
    ("underscore", '\u{005F}'),
    ("v", '\u{0076}'),
    ("w", '\u{0077}'),
    ("x", '\u{0078}'),
    ("y", '\u{0079}'),
    ("yacute", '\u{00FD}'),
    ("ydieresis", '\u{00FF}'),
    ("yen", '\u{00A5}'),
    ("z", '\u{007A}'),
    ("zcaron", '\u{017E}'),
    ("zero", '\u{0030}'),
];

/// Names the committed name-keyed programs carry that no table of Annex D
/// lists — read against Tables D.2, D.5 and D.6 — and that no algorithmic
/// form reads, so they stand for no character.
const OUTSIDE_ANNEX_D: [&str; 1] = ["square"];

/// The character a name-keyed program's glyph name stands for, as the
/// design reads it, for the names the committed programs carry: the part
/// before the first period, holding no underscore, as a D.2 name or the
/// Glyph List's `uniXXXX` or `uXXXX` to `uXXXXXX` with upper-case digits.
fn name_char(label: &str, name: &str) -> Option<char> {
    let base = name.split('.').next()?;
    if base.is_empty() || base.contains('_') {
        return None;
    }
    if let Some((_, c)) = STANDARD_LATIN.iter().find(|(n, _)| *n == base) {
        return Some(*c);
    }
    let hex = |digits: &str| {
        digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
            .then(|| u32::from_str_radix(digits, 16).ok())
            .flatten()
            .and_then(char::from_u32)
    };
    if let Some(digits) = base.strip_prefix("uni") {
        if digits.len() == 4 {
            return hex(digits);
        }
        return None;
    }
    if let Some(digits) = base.strip_prefix('u') {
        if (4..=6).contains(&digits.len()) {
            if let Some(c) = hex(digits) {
                return Some(c);
            }
        }
    }
    assert!(
        OUTSIDE_ANNEX_D.contains(&base),
        "{label}: the test's reader does not know whether Annex D lists `{base}`"
    );
    None
}

/// Whether a name is one of Table D.2's.
fn is_standard_latin_name(name: &str) -> bool {
    STANDARD_LATIN.iter().any(|(n, _)| *n == name)
}

// ---- what the reader reads -------------------------------------------------

/// One glyph as the reader reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Glyph {
    advance: Option<f64>,
    bounds: Option<[f64; 4]>,
    scale: f64,
}

/// Every answer for one face, as the reader reads it.
#[derive(Debug)]
struct Expected {
    format: FontFormat,
    glyph_count: u16,
    units_per_em: f64,
    font_scale: f64,
    font_box: Option<[f64; 4]>,
    glyphs: Vec<Glyph>,
    chars: Vec<(char, u16)>,
    names: Vec<Option<String>>,
    /// Every name given to a glyph: each glyph's own, and a Type 42
    /// program's further keys.
    given: Vec<(String, u16)>,
    flags: u32,
    notdef: Option<u16>,
}

const FIXED_PITCH: u32 = 1;
const SERIF: u32 = 1 << 1;
const SYMBOLIC: u32 = 1 << 2;
const SCRIPT: u32 = 1 << 3;
const NONSYMBOLIC: u32 = 1 << 5;
const ITALIC: u32 = 1 << 6;
const FORCE_BOLD: u32 = 1 << 18;

fn ordered([x0, y0, x1, y1]: [f64; 4]) -> [f64; 4] {
    [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
}

/// A font matrix's `a` as a scale, glyph space's 1/1000 where it is zero or
/// not finite.
fn scale_of(a: f64) -> f64 {
    if a.is_finite() && a != 0.0 {
        a
    } else {
        0.001
    }
}

// ---- the box of a drawn shape ------------------------------------------------

/// 9.2.4's box of what an outline draws, accumulated as it is drawn: each
/// contour that draws something — its start, every segment's end, each
/// curve's extremes — and the origin where nothing is drawn.
#[derive(Default)]
struct Shape {
    start: (f64, f64),
    started: bool,
    x: Option<(f64, f64)>,
    y: Option<(f64, f64)>,
}

impl Shape {
    fn widen(axis: &mut Option<(f64, f64)>, value: f64) {
        if !value.is_finite() {
            return;
        }
        *axis = Some(match *axis {
            None => (value, value),
            Some((low, high)) => (low.min(value), high.max(value)),
        });
    }

    fn point(&mut self, (x, y): (f64, f64)) {
        Shape::widen(&mut self.x, x);
        Shape::widen(&mut self.y, y);
    }

    fn begin(&mut self) {
        if !self.started {
            let start = self.start;
            self.point(start);
            self.started = true;
        }
    }

    fn move_to(&mut self, at: (f64, f64)) {
        self.start = at;
        self.started = false;
    }

    fn line_to(&mut self, to: (f64, f64)) {
        self.begin();
        self.point(to);
    }

    fn curve_to(&mut self, from: (f64, f64), c1: (f64, f64), c2: (f64, f64), to: (f64, f64)) {
        self.begin();
        self.point(to);
        for value in turns(from.0, c1.0, c2.0, to.0) {
            Shape::widen(&mut self.x, value);
        }
        for value in turns(from.1, c1.1, c2.1, to.1) {
            Shape::widen(&mut self.y, value);
        }
    }

    fn bounds(&self) -> [f64; 4] {
        match (self.x, self.y) {
            (Some((x0, x1)), Some((y0, y1))) => [x0, y0, x1, y1],
            _ => [0.0; 4],
        }
    }
}

/// A cubic coordinate's values where it turns inside `(0, 1)`: the design's
/// roots of `A t² + B t + C`, each evaluated by de Casteljau's construction.
fn turns(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let d0 = p1 - p0;
    let d1 = p2 - p1;
    let d2 = p3 - p2;
    let a = d0 - 2.0 * d1 + d2;
    let b = 2.0 * (d1 - d0);
    let c = d0;
    let mut roots = Vec::new();
    if a == 0.0 {
        if b != 0.0 {
            roots.push(-c / b);
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            roots.push((-b + root) / (2.0 * a));
            roots.push((-b - root) / (2.0 * a));
        }
    }
    roots
        .into_iter()
        .filter(|t| *t > 0.0 && *t < 1.0)
        .map(|t| {
            let q0 = p0 + (p1 - p0) * t;
            let q1 = p1 + (p2 - p1) * t;
            let q2 = p2 + (p3 - p2) * t;
            let r0 = q0 + (q1 - q0) * t;
            let r1 = q1 + (q2 - q1) * t;
            r0 + (r1 - r0) * t
        })
        .collect()
}

// ---- an sfnt ---------------------------------------------------------------

/// A table directory: the first face of a collection.
struct Directory<'a> {
    data: &'a [u8],
    tables: Vec<([u8; 4], usize, usize)>,
}

impl<'a> Directory<'a> {
    fn read(data: &'a [u8]) -> Option<Directory<'a>> {
        let base = if data.get(..4) == Some(b"ttcf") {
            be32(data, 12)? as usize
        } else {
            0
        };
        if !matches!(be32(data, base)?, 0x0001_0000 | 0x7472_7565 | 0x4F54_544F) {
            return None;
        }
        let count = usize::from(be16(data, base + 4)?);
        let mut tables = Vec::new();
        for i in 0..count.min(512) {
            let at = base + 12 + 16 * i;
            let (Some(tag), Some(offset), Some(length)) = (
                data.get(at..at + 4),
                be32(data, at + 8),
                be32(data, at + 12),
            ) else {
                break;
            };
            tables.push((tag.try_into().ok()?, offset as usize, length as usize));
        }
        Some(Directory { data, tables })
    }

    fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        let (_, offset, length) = self.tables.iter().find(|(t, _, _)| t == tag)?;
        self.data.get(*offset..offset.checked_add(*length)?)
    }
}

/// The `cmap` subtable a face's characters are looked up in, and every
/// scalar it maps to a glyph below `glyph_count` other than glyph 0.
fn read_cmap(label: &str, cmap: &[u8], glyph_count: u16) -> (Vec<(char, u16)>, Option<(u16, u16)>) {
    let records = usize::from(be16(cmap, 2).unwrap_or(0));
    let mut best: Option<(u32, u16, u16, usize)> = None;
    for i in 0..records.min(64) {
        let at = 4 + 8 * i;
        let (Some(platform), Some(encoding), Some(offset)) =
            (be16(cmap, at), be16(cmap, at + 2), be32(cmap, at + 4))
        else {
            break;
        };
        let score = match (platform, encoding) {
            (3, 10) => 5,
            (3, 1) => 4,
            (0, _) => 3,
            (3, 0) => 2,
            (1, 0) => 1,
            _ => 0,
        };
        if score > 0 && best.is_none_or(|(b, _, _, _)| score > b) {
            best = Some((score, platform, encoding, offset as usize));
        }
    }
    let Some((_, platform, encoding, offset)) = best else {
        return (Vec::new(), None);
    };
    let sub = cmap.get(offset..).unwrap_or(&[]);
    let codes: BTreeMap<u32, u16> = match be16(sub, 0) {
        Some(0) => (0..256u32)
            .filter_map(|code| Some((code, u16::from(*sub.get(6 + code as usize)?))))
            .collect(),
        Some(4) => format_4_codes(sub),
        Some(6) => {
            let first = u32::from(be16(sub, 6).unwrap_or(0));
            let count = u32::from(be16(sub, 8).unwrap_or(0));
            (first..first + count)
                .filter_map(|code| Some((code, be16(sub, 10 + ((code - first) as usize) * 2)?)))
                .collect()
        }
        Some(format @ (12 | 13)) => {
            let groups = be32(sub, 12).unwrap_or(0) as usize;
            let mut decided: BTreeMap<u32, Option<u16>> = BTreeMap::new();
            for i in 0..groups.min(1 << 20) {
                let at = 16 + 12 * i;
                let (Some(start), Some(end), Some(glyph)) =
                    (be32(sub, at), be32(sub, at + 4), be32(sub, at + 8))
                else {
                    break;
                };
                if start > end || start > 0x10_FFFF {
                    continue;
                }
                for code in start..=end.min(0x10_FFFF) {
                    let id = if format == 12 {
                        glyph
                            .checked_add(code - start)
                            .and_then(|id| u16::try_from(id).ok())
                    } else {
                        u16::try_from(glyph).ok()
                    };
                    decided.entry(code).or_insert(id);
                }
            }
            decided
                .into_iter()
                .filter_map(|(code, id)| Some((code, id?)))
                .collect()
        }
        other => panic!("{label}: the test's reader reads no format {other:?} subtable"),
    };
    let mut chars: BTreeMap<char, u16> = BTreeMap::new();
    for (code, glyph) in codes {
        if glyph == 0 || glyph >= glyph_count {
            continue;
        }
        if (platform, encoding) == (3, 0) {
            if (0xF000..=0xF0FF).contains(&code) {
                if let Some(c) = char::from_u32(code - 0xF000) {
                    chars.insert(c, glyph);
                }
            }
            if code < 0x100 {
                continue;
            }
        }
        if platform == 1 && code >= 0x80 {
            continue;
        }
        if let Some(c) = char::from_u32(code) {
            chars.insert(c, glyph);
        }
    }
    (chars.into_iter().collect(), Some((platform, encoding)))
}

/// A format 4 subtable's codes: the first segment whose end is at or past a
/// code decides it, glyph 0 where the segment starts past it.
fn format_4_codes(sub: &[u8]) -> BTreeMap<u32, u16> {
    let mut out = BTreeMap::new();
    let Some(seg_x2) = be16(sub, 6) else {
        return out;
    };
    let segments = usize::from(seg_x2) / 2;
    let ends = 14;
    let starts = ends + 2 * segments + 2;
    let deltas = starts + 2 * segments;
    let ranges = deltas + 2 * segments;
    let mut undecided = 0u32;
    for i in 0..segments {
        let Some(end) = be16(sub, ends + 2 * i).map(u32::from) else {
            break;
        };
        if end < undecided {
            continue;
        }
        let from = undecided;
        undecided = end + 1;
        let (Some(start), Some(delta), Some(range)) = (
            be16(sub, starts + 2 * i),
            be16(sub, deltas + 2 * i),
            be16(sub, ranges + 2 * i),
        ) else {
            continue;
        };
        for code in from.max(u32::from(start))..=end {
            let glyph = if range == 0 {
                (code as u16).wrapping_add(delta)
            } else {
                let at =
                    ranges + 2 * i + usize::from(range) + 2 * (code - u32::from(start)) as usize;
                match be16(sub, at) {
                    None | Some(0) => 0,
                    Some(glyph) => glyph.wrapping_add(delta),
                }
            };
            if glyph != 0 {
                out.insert(code, glyph);
            }
        }
    }
    out
}

/// `post`'s names, glyph by glyph, as far as `glyph_count`.
fn post_names(post: Option<&[u8]>, glyph_count: u16, maxp: Option<u16>) -> Vec<Option<String>> {
    let count = usize::from(glyph_count);
    let mut names: Vec<Option<String>> = vec![None; count];
    let Some(post) = post else {
        return names;
    };
    match be32(post, 0) {
        Some(0x0001_0000) => {
            let limit = maxp.map_or(MAC_NAMES.len(), usize::from);
            for (glyph, slot) in names.iter_mut().enumerate() {
                if glyph < limit {
                    *slot = MAC_NAMES.get(glyph).map(|name| (*name).to_string());
                }
            }
        }
        Some(0x0002_0000) => {
            let numbered = usize::from(be16(post, 32).unwrap_or(0));
            let mut strings: Vec<&[u8]> = Vec::new();
            let mut at = 34 + 2 * numbered;
            while let Some(&len) = post.get(at) {
                let Some(string) = post.get(at + 1..at + 1 + usize::from(len)) else {
                    break;
                };
                strings.push(string);
                at += 1 + usize::from(len);
            }
            for (glyph, slot) in names.iter_mut().enumerate().take(numbered) {
                let Some(index) = be16(post, 34 + 2 * glyph).map(usize::from) else {
                    break;
                };
                *slot = if index < 258 {
                    Some(MAC_NAMES[index].to_string())
                } else {
                    strings
                        .get(index - 258)
                        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
                };
            }
        }
        Some(0x0002_5000) => {
            let numbered = usize::from(be16(post, 32).unwrap_or(0));
            for (glyph, slot) in names.iter_mut().enumerate().take(numbered) {
                let Some(&offset) = post.get(34 + glyph) else {
                    break;
                };
                let index = glyph as i64 + i64::from(offset as i8);
                *slot = usize::try_from(index)
                    .ok()
                    .and_then(|index| MAC_NAMES.get(index))
                    .map(|name| (*name).to_string());
            }
        }
        _ => {}
    }
    names
}

/// An sfnt, as the reader reads it; `keys`, a Type 42 program's
/// `/CharStrings`, names its glyphs first.
fn read_sfnt(label: &str, data: &[u8], keys: Option<&[(String, u16)]>) -> Expected {
    let dir = Directory::read(data).unwrap_or_else(|| panic!("{label}: no table directory"));
    let head = dir.table(b"head");
    let units = head
        .and_then(|head| be16(head, 18))
        .filter(|units| *units >= 16)
        .unwrap_or(1000);
    let (format, cff) = if dir.table(b"glyf").is_some() {
        (FontFormat::TrueType, None)
    } else if let Some(table) = dir.table(b"CFF ") {
        (FontFormat::OpenTypeCff, CffFont::read(table))
    } else if dir.table(b"CFF2").is_some() {
        panic!("{label}: no committed face has CFF2 outlines, and the reader reads none")
    } else {
        (FontFormat::TrueType, None)
    };
    let maxp = dir.table(b"maxp").and_then(|maxp| be16(maxp, 4));
    let long_loca = head.and_then(|head| be16(head, 50)).is_some_and(|v| v != 0);
    let loca = dir.table(b"loca");
    let glyph_count = maxp
        .or_else(|| {
            if format == FontFormat::TrueType {
                let entries = loca?.len() / if long_loca { 4 } else { 2 };
                Some(u16::try_from(entries.saturating_sub(1)).unwrap_or(u16::MAX))
            } else {
                cff.as_ref()
                    .map(|cff| u16::try_from(cff.charstrings.len()).unwrap_or(u16::MAX))
            }
        })
        .unwrap_or(0);
    let scale = 1.0 / f64::from(units);

    // hhea's count of long metrics, hmtx the advances.
    let long_metrics = dir
        .table(b"hhea")
        .and_then(|hhea| be16(hhea, 34))
        .unwrap_or(0);
    let hmtx = dir.table(b"hmtx");
    let glyf = dir.table(b"glyf");
    let glyphs: Vec<Glyph> = (0..glyph_count)
        .map(|glyph| {
            let advance = (long_metrics > 0)
                .then(|| {
                    let at = usize::from(glyph.min(long_metrics - 1)) * 4;
                    be16(hmtx?, at).map(f64::from)
                })
                .flatten();
            let bounds = match &cff {
                Some(cff) => cff.glyph(glyph).map(|(_, bounds)| bounds),
                None => (|| {
                    let loca = loca?;
                    let index = usize::from(glyph);
                    let (start, end) = if long_loca {
                        (
                            be32(loca, index * 4)? as usize,
                            be32(loca, index * 4 + 4)? as usize,
                        )
                    } else {
                        (
                            usize::from(be16(loca, index * 2)?) * 2,
                            usize::from(be16(loca, index * 2 + 2)?) * 2,
                        )
                    };
                    let glyf = glyf?;
                    if end <= start {
                        return Some([0.0; 4]);
                    }
                    let header = glyf.get(start..end)?;
                    let edge = |at: usize| be16(header, at).map(|v| f64::from(v as i16));
                    Some(ordered([edge(2)?, edge(4)?, edge(6)?, edge(8)?]))
                })(),
            };
            Glyph {
                advance,
                bounds,
                scale,
            }
        })
        .collect();

    let font_box = head.and_then(|head| {
        let edge = |at: usize| be16(head, at).map(|v| f64::from(v as i16));
        Some(ordered([edge(36)?, edge(38)?, edge(40)?, edge(42)?]))
    });
    let (chars, subtable) = match dir.table(b"cmap") {
        Some(cmap) => read_cmap(label, cmap, glyph_count),
        None => (Vec::new(), None),
    };

    // Names: a Type 42 program's keys, an OpenType CFF face's charset, else
    // post.
    let post = dir.table(b"post");
    let mut names = match (&cff, keys) {
        (Some(cff), None) => (0..glyph_count).map(|glyph| cff.name(glyph)).collect(),
        _ => post_names(post, glyph_count, maxp),
    };
    let mut given: Vec<(String, u16)> = Vec::new();
    if let Some(keys) = keys {
        let posted = post_names(post, glyph_count, maxp);
        names = vec![None; usize::from(glyph_count)];
        for (name, glyph) in keys {
            if let Some(slot) = names.get_mut(usize::from(*glyph)) {
                given.push((name.clone(), *glyph));
                if slot.is_none() {
                    *slot = Some(name.clone());
                }
            }
        }
        for (slot, posted) in names.iter_mut().zip(posted) {
            if slot.is_none() {
                *slot = posted;
            }
        }
    }
    given.extend(
        names
            .iter()
            .enumerate()
            .filter_map(|(glyph, name)| Some((name.clone()?, glyph as u16))),
    );

    // 9.8.2's flags.
    let os2 = dir.table(b"OS/2");
    let class = os2.and_then(|os2| be16(os2, 30)).map(|v| (v >> 8) as u8);
    let panose = os2.and_then(|os2| os2.get(32..42));
    let serif = match class {
        Some(1..=5 | 7) => true,
        Some(0) => panose.is_some_and(|p| p[0] == 2 && (2..=10).contains(&p[1])),
        _ => false,
    };
    let script = match class {
        Some(10) => true,
        Some(0) => panose.is_some_and(|p| p[0] == 3),
        _ => false,
    };
    let fixed = post.and_then(|post| be32(post, 12)).is_some_and(|v| v != 0);
    let slanted = post.and_then(|post| be32(post, 4)).is_some_and(|v| v != 0)
        || os2
            .and_then(|os2| be16(os2, 62))
            .is_some_and(|selection| selection & (1 | (1 << 9)) != 0)
        || head
            .and_then(|head| be16(head, 44))
            .is_some_and(|mac| mac & 2 != 0);
    let nonsymbolic = subtable != Some((3, 0))
        && !chars.is_empty()
        && chars
            .iter()
            .all(|(c, _)| STANDARD_LATIN.iter().any(|(_, latin)| latin == c));
    let force_bold = cff.as_ref().is_some_and(|cff| cff.force_bold());
    let mut flags = 0;
    for (set, flag) in [
        (fixed, FIXED_PITCH),
        (serif, SERIF),
        (script, SCRIPT),
        (slanted, ITALIC),
        (force_bold, FORCE_BOLD),
        (nonsymbolic, NONSYMBOLIC),
        (!nonsymbolic, SYMBOLIC),
    ] {
        if set {
            flags |= flag;
        }
    }

    Expected {
        format,
        glyph_count,
        units_per_em: f64::from(units),
        font_scale: scale,
        font_box,
        glyphs,
        chars,
        names,
        given,
        flags,
        notdef: (glyph_count > 0).then_some(0),
    }
}

// ---- a CFF -----------------------------------------------------------------

/// An INDEX at `at`: its items and where it ends.
fn cff_index(data: &[u8], at: usize) -> Option<(Vec<&[u8]>, usize)> {
    let count = usize::from(be16(data, at)?);
    if count == 0 {
        return Some((Vec::new(), at + 2));
    }
    let size = usize::from(*data.get(at + 2)?);
    if !(1..=4).contains(&size) {
        return None;
    }
    let offset = |i: usize| -> Option<usize> {
        let mut value = 0usize;
        for byte in data.get(at + 3 + i * size..at + 3 + (i + 1) * size)? {
            value = (value << 8) | usize::from(*byte);
        }
        Some(value)
    };
    let base = at + 3 + (count + 1) * size - 1;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        items.push(data.get(base + offset(i)?..base + offset(i + 1)?)?);
    }
    Some((items, base + offset(count)?))
}

/// A DICT's operators and their operands, the first of a repeated operator
/// kept.
fn cff_dict(bytes: &[u8]) -> BTreeMap<u16, Vec<f64>> {
    let mut out = BTreeMap::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            0..=21 => {
                let op = if b == 12 {
                    i += 1;
                    0x0C00 | u16::from(bytes.get(i).copied().unwrap_or(0))
                } else {
                    u16::from(b)
                };
                i += 1;
                out.entry(op).or_insert_with(|| operands.clone());
                operands.clear();
            }
            28 => {
                let Some(v) = be16(bytes, i + 1) else { break };
                operands.push(f64::from(v as i16));
                i += 3;
            }
            29 => {
                let Some(v) = be32(bytes, i + 1) else { break };
                operands.push(f64::from(v as i32));
                i += 5;
            }
            30 => {
                let mut text = String::new();
                i += 1;
                'real: while let Some(&byte) = bytes.get(i) {
                    i += 1;
                    for nibble in [byte >> 4, byte & 15] {
                        match nibble {
                            0..=9 => text.push(char::from(b'0' + nibble)),
                            10 => text.push('.'),
                            11 => text.push('E'),
                            12 => text.push_str("E-"),
                            14 => text.push('-'),
                            15 => break 'real,
                            _ => {}
                        }
                    }
                }
                operands.push(text.parse().unwrap_or(0.0));
            }
            32..=246 => {
                operands.push(f64::from(b) - 139.0);
                i += 1;
            }
            247..=250 => {
                let next = f64::from(bytes.get(i + 1).copied().unwrap_or(0));
                operands.push((f64::from(b) - 247.0) * 256.0 + next + 108.0);
                i += 2;
            }
            251..=254 => {
                let next = f64::from(bytes.get(i + 1).copied().unwrap_or(0));
                operands.push(-(f64::from(b) - 251.0) * 256.0 - next - 108.0);
                i += 2;
            }
            _ => i += 1,
        }
    }
    out
}

/// A Private DICT's subroutines, widths and `ForceBold`.
#[derive(Default)]
struct CffPrivate<'a> {
    subrs: Vec<&'a [u8]>,
    default_width: f64,
    nominal_width: f64,
    force_bold: bool,
}

fn cff_private<'a>(data: &'a [u8], operands: &[f64]) -> CffPrivate<'a> {
    let (Some(&size), Some(&offset)) = (operands.first(), operands.get(1)) else {
        return CffPrivate::default();
    };
    let (size, offset) = (size as usize, offset as usize);
    let Some(bytes) = data.get(offset..offset + size) else {
        return CffPrivate::default();
    };
    let dict = cff_dict(bytes);
    let first = |op: u16| dict.get(&op).and_then(|v| v.first().copied());
    CffPrivate {
        subrs: first(19)
            .and_then(|at| cff_index(data, offset + at as usize))
            .map(|(subrs, _)| subrs)
            .unwrap_or_default(),
        default_width: first(20).unwrap_or(0.0),
        nominal_width: first(21).unwrap_or(0.0),
        force_bold: first(0x0C0E).is_some_and(|v| v != 0.0),
    }
}

/// The bias a subroutine number is read with, by the INDEX's count (TN #5177
/// section 4.7).
fn bias(count: usize) -> f64 {
    if count < 1240 {
        107.0
    } else if count < 33900 {
        1131.0
    } else {
        32768.0
    }
}

/// A CFF font program's first font.
struct CffFont<'a> {
    charstrings: Vec<&'a [u8]>,
    global: Vec<&'a [u8]>,
    strings: Vec<&'a [u8]>,
    private: CffPrivate<'a>,
    fds: Vec<(CffPrivate<'a>, Option<[f64; 6]>)>,
    fd_select: Vec<u8>,
    charset: Vec<u16>,
    cid: bool,
    matrix: [f64; 6],
    top_matrix: bool,
    bbox: [f64; 4],
    fixed_pitch: bool,
    italic_angle: f64,
}

impl<'a> CffFont<'a> {
    fn read(data: &'a [u8]) -> Option<CffFont<'a>> {
        let header = usize::from(*data.get(2)?);
        let (_, at) = cff_index(data, header)?;
        let (tops, at) = cff_index(data, at)?;
        let (strings, at) = cff_index(data, at)?;
        let (global, _) = cff_index(data, at)?;
        let top = cff_dict(tops.first()?);
        let first = |op: u16| top.get(&op).and_then(|v| v.first().copied());
        let (charstrings, _) = cff_index(data, first(17)? as usize)?;
        let count = charstrings.len();
        let cid = top.contains_key(&0x0C1E);
        let private = top
            .get(&18)
            .map(|operands| cff_private(data, operands))
            .unwrap_or_default();

        let charset_at = first(15).unwrap_or(0.0) as usize;
        let mut charset: Vec<u16> = vec![0];
        match charset_at {
            0 => {
                assert!(count <= 229, "ISOAdobe names 229 glyphs");
                charset = (0..count as u16).collect();
            }
            1 | 2 => panic!("no committed CFF uses a predefined Expert charset"),
            at => match data.get(at)? {
                0 => {
                    for i in 0..count.saturating_sub(1) {
                        charset.push(be16(data, at + 1 + 2 * i)?);
                    }
                }
                format @ (1 | 2) => {
                    let mut cursor = at + 1;
                    while charset.len() < count {
                        let first = be16(data, cursor)?;
                        let left = if *format == 1 {
                            u16::from(*data.get(cursor + 2)?)
                        } else {
                            be16(data, cursor + 2)?
                        };
                        cursor += if *format == 1 { 3 } else { 4 };
                        for k in 0..=left {
                            if charset.len() < count {
                                charset.push(first + k);
                            }
                        }
                    }
                }
                _ => return None,
            },
        }

        let mut fds = Vec::new();
        if let Some(at) = first(0x0C24) {
            let (dicts, _) = cff_index(data, at as usize)?;
            for dict in dicts {
                let dict = cff_dict(dict);
                let private = dict
                    .get(&18)
                    .map(|operands| cff_private(data, operands))
                    .unwrap_or_default();
                let matrix = dict
                    .get(&0x0C07)
                    .filter(|m| m.len() >= 6)
                    .map(|m| [m[0], m[1], m[2], m[3], m[4], m[5]]);
                fds.push((private, matrix));
            }
        }
        let mut fd_select = Vec::new();
        if let (Some(at), false) = (first(0x0C25), fds.is_empty()) {
            let at = at as usize;
            match data.get(at)? {
                0 => fd_select = data.get(at + 1..at + 1 + count)?.to_vec(),
                3 => {
                    let ranges = usize::from(be16(data, at + 1)?);
                    fd_select = vec![0; count];
                    for r in 0..ranges {
                        let first = usize::from(be16(data, at + 3 + 3 * r)?);
                        let fd = *data.get(at + 5 + 3 * r)?;
                        let next = usize::from(be16(data, at + 6 + 3 * r)?);
                        for slot in fd_select.iter_mut().take(next.min(count)).skip(first) {
                            *slot = fd;
                        }
                    }
                }
                _ => return None,
            }
        }
        let (matrix, top_matrix) = match top.get(&0x0C07) {
            Some(m) if m.len() >= 6 => ([m[0], m[1], m[2], m[3], m[4], m[5]], true),
            _ => ([0.001, 0.0, 0.0, 0.001, 0.0, 0.0], false),
        };
        let bbox = match top.get(&5) {
            Some(b) if b.len() == 4 && b.iter().all(|v| v.is_finite()) => [b[0], b[1], b[2], b[3]],
            _ => [0.0; 4],
        };
        Some(CffFont {
            charstrings,
            global,
            strings,
            private,
            fds,
            fd_select,
            charset,
            cid,
            matrix,
            top_matrix,
            bbox,
            fixed_pitch: first(0x0C01).is_some_and(|v| v != 0.0),
            italic_angle: first(0x0C02).filter(|v| v.is_finite()).unwrap_or(0.0),
        })
    }

    /// The Private DICT and font matrix a glyph draws with.
    fn private_for(&self, glyph: u16) -> (&CffPrivate<'a>, Option<&[f64; 6]>) {
        if self.cid {
            if let Some(fd) = self.fd_select.get(usize::from(glyph)) {
                if let Some((private, matrix)) = self.fds.get(usize::from(*fd)) {
                    return (private, matrix.as_ref());
                }
            }
        }
        (&self.private, None)
    }

    /// A glyph's scale to the em: its Font DICT's matrix composed with the
    /// Top DICT's where both state one.
    fn scale(&self, glyph: u16) -> f64 {
        let a = match self.private_for(glyph).1 {
            Some(fd) if self.top_matrix => fd[0] * self.matrix[0] + fd[1] * self.matrix[2],
            Some(fd) => fd[0],
            None => self.matrix[0],
        };
        scale_of(a)
    }

    fn force_bold(&self) -> bool {
        if self.cid {
            self.fds
                .first()
                .is_some_and(|(private, _)| private.force_bold)
        } else {
            self.private.force_bold
        }
    }

    fn sid_name(&self, sid: u16) -> Option<String> {
        let sid = usize::from(sid);
        if let Some(name) = STANDARD_STRINGS.get(sid) {
            return Some((*name).to_string());
        }
        String::from_utf8(self.strings.get(sid - 391)?.to_vec()).ok()
    }

    fn name(&self, glyph: u16) -> Option<String> {
        if self.cid {
            return None;
        }
        self.sid_name(*self.charset.get(usize::from(glyph))?)
    }

    /// A glyph's width and box, by the Type 2 interpreter below.
    fn glyph(&self, glyph: u16) -> Option<(f64, [f64; 4])> {
        let code = self.charstrings.get(usize::from(glyph))?;
        let (private, _) = self.private_for(glyph);
        let mut run = Type2 {
            font: self,
            private,
            stack: Vec::new(),
            stems: 0,
            width: None,
            x: 0.0,
            y: 0.0,
            shape: Shape::default(),
            ended: false,
            steps: 0,
        };
        run.run(code, 0);
        let width = run.width.unwrap_or(private.default_width);
        Some((width, run.shape.bounds()))
    }
}

/// TN #5177's Type 2 charstring machine, as far as a box and a width need.
struct Type2<'f, 'a> {
    font: &'f CffFont<'a>,
    private: &'f CffPrivate<'a>,
    stack: Vec<f64>,
    stems: usize,
    width: Option<f64>,
    x: f64,
    y: f64,
    shape: Shape,
    ended: bool,
    steps: u32,
}

impl Type2<'_, '_> {
    /// The width an operator that clears the stack carries where it is the
    /// first: an operand more than `operands` takes.
    fn width_past(&mut self, operands: usize) {
        if self.width.is_some() {
            return;
        }
        self.width = Some(if self.stack.len() > operands {
            self.private.nominal_width + self.stack.remove(0)
        } else {
            self.private.default_width
        });
    }

    /// The width a stem operator carries where it is the first: an odd
    /// operand count's first.
    fn width_odd(&mut self) {
        if self.width.is_some() {
            return;
        }
        self.width = Some(if self.stack.len() % 2 == 1 {
            self.private.nominal_width + self.stack.remove(0)
        } else {
            self.private.default_width
        });
    }

    fn move_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.shape.move_to((self.x, self.y));
    }

    fn line_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.shape.line_to((self.x, self.y));
    }

    #[allow(clippy::too_many_arguments)]
    fn curve_by(&mut self, dxa: f64, dya: f64, dxb: f64, dyb: f64, dxc: f64, dyc: f64) {
        let from = (self.x, self.y);
        let c1 = (from.0 + dxa, from.1 + dya);
        let c2 = (c1.0 + dxb, c1.1 + dyb);
        let to = (c2.0 + dxc, c2.1 + dyc);
        self.shape.curve_to(from, c1, c2, to);
        (self.x, self.y) = to;
    }

    fn run(&mut self, code: &[u8], depth: u32) {
        assert!(depth <= 10, "a committed charstring nests past ten");
        let mut i = 0;
        while i < code.len() && !self.ended {
            self.steps += 1;
            assert!(self.steps < 1_000_000, "a committed charstring runs away");
            let b = code[i];
            i += 1;
            let args = |run: &Self| run.stack.clone();
            match b {
                32..=246 => self.stack.push(f64::from(b) - 139.0),
                247..=250 => {
                    let w = f64::from(code[i]);
                    i += 1;
                    self.stack.push((f64::from(b) - 247.0) * 256.0 + w + 108.0);
                }
                251..=254 => {
                    let w = f64::from(code[i]);
                    i += 1;
                    self.stack.push(-(f64::from(b) - 251.0) * 256.0 - w - 108.0);
                }
                255 => {
                    let v = i32::from_be_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
                    i += 4;
                    self.stack.push(f64::from(v) / 65536.0);
                }
                28 => {
                    let v = i16::from_be_bytes([code[i], code[i + 1]]);
                    i += 2;
                    self.stack.push(f64::from(v));
                }
                1 | 3 | 18 | 23 => {
                    self.width_odd();
                    self.stems += self.stack.len() / 2;
                    self.stack.clear();
                }
                19 | 20 => {
                    self.width_odd();
                    self.stems += self.stack.len() / 2;
                    self.stack.clear();
                    i += self.stems.div_ceil(8);
                }
                21 => {
                    self.width_past(2);
                    let a = args(self);
                    self.move_by(a[0], a[1]);
                    self.stack.clear();
                }
                22 => {
                    self.width_past(1);
                    let a = args(self);
                    self.move_by(a[0], 0.0);
                    self.stack.clear();
                }
                4 => {
                    self.width_past(1);
                    let a = args(self);
                    self.move_by(0.0, a[0]);
                    self.stack.clear();
                }
                5 => {
                    for pair in args(self).chunks_exact(2) {
                        self.line_by(pair[0], pair[1]);
                    }
                    self.stack.clear();
                }
                6 | 7 => {
                    let mut horizontal = b == 6;
                    for v in args(self) {
                        if horizontal {
                            self.line_by(v, 0.0);
                        } else {
                            self.line_by(0.0, v);
                        }
                        horizontal = !horizontal;
                    }
                    self.stack.clear();
                }
                8 => {
                    for c in args(self).chunks_exact(6) {
                        self.curve_by(c[0], c[1], c[2], c[3], c[4], c[5]);
                    }
                    self.stack.clear();
                }
                24 => {
                    let a = args(self);
                    let curves = (a.len() - 2) / 6;
                    for c in a[..curves * 6].chunks_exact(6) {
                        self.curve_by(c[0], c[1], c[2], c[3], c[4], c[5]);
                    }
                    self.line_by(a[curves * 6], a[curves * 6 + 1]);
                    self.stack.clear();
                }
                25 => {
                    let a = args(self);
                    let lines = (a.len() - 6) / 2;
                    for pair in a[..lines * 2].chunks_exact(2) {
                        self.line_by(pair[0], pair[1]);
                    }
                    let c = &a[lines * 2..];
                    self.curve_by(c[0], c[1], c[2], c[3], c[4], c[5]);
                    self.stack.clear();
                }
                26 => {
                    let a = args(self);
                    let (mut dx1, rest) = if a.len() % 2 == 1 {
                        (a[0], &a[1..])
                    } else {
                        (0.0, &a[..])
                    };
                    for c in rest.chunks_exact(4) {
                        self.curve_by(dx1, c[0], c[1], c[2], 0.0, c[3]);
                        dx1 = 0.0;
                    }
                    self.stack.clear();
                }
                27 => {
                    let a = args(self);
                    let (mut dy1, rest) = if a.len() % 2 == 1 {
                        (a[0], &a[1..])
                    } else {
                        (0.0, &a[..])
                    };
                    for c in rest.chunks_exact(4) {
                        self.curve_by(c[0], dy1, c[1], c[2], c[3], 0.0);
                        dy1 = 0.0;
                    }
                    self.stack.clear();
                }
                30 | 31 => {
                    let a = args(self);
                    let mut horizontal = b == 31;
                    let mut at = 0;
                    while at + 4 <= a.len() {
                        let last = a.len() - at == 5;
                        let extra = if last { a[at + 4] } else { 0.0 };
                        if horizontal {
                            self.curve_by(a[at], 0.0, a[at + 1], a[at + 2], extra, a[at + 3]);
                        } else {
                            self.curve_by(0.0, a[at], a[at + 1], a[at + 2], a[at + 3], extra);
                        }
                        at += if last { 5 } else { 4 };
                        horizontal = !horizontal;
                    }
                    self.stack.clear();
                }
                10 | 29 => {
                    let subrs = if b == 10 {
                        &self.private.subrs
                    } else {
                        &self.font.global
                    };
                    let number = self.stack.pop().expect("a subroutine number") + bias(subrs.len());
                    let subr = *subrs
                        .get(number as usize)
                        .expect("a committed charstring calls a subroutine it has");
                    self.run(subr, depth + 1);
                }
                11 => return,
                14 => {
                    // seac's four operands are no width, and a fifth is one.
                    if self.width.is_none() {
                        self.width = Some(if self.stack.len() % 2 == 1 {
                            self.private.nominal_width + self.stack.remove(0)
                        } else {
                            self.private.default_width
                        });
                    }
                    assert!(self.stack.len() < 4, "no committed CFF uses seac");
                    self.ended = true;
                }
                12 => {
                    let op = code[i];
                    i += 1;
                    let a = args(self);
                    match op {
                        35 => {
                            self.curve_by(a[0], a[1], a[2], a[3], a[4], a[5]);
                            self.curve_by(a[6], a[7], a[8], a[9], a[10], a[11]);
                        }
                        34 => {
                            self.curve_by(a[0], 0.0, a[1], a[2], a[3], 0.0);
                            self.curve_by(a[4], 0.0, a[5], -a[2], a[6], 0.0);
                        }
                        36 => {
                            self.curve_by(a[0], a[1], a[2], a[3], a[4], 0.0);
                            self.curve_by(a[5], 0.0, a[6], a[7], a[8], -(a[1] + a[3] + a[7]));
                        }
                        37 => {
                            let dx: f64 = [a[0], a[2], a[4], a[6], a[8]].iter().sum();
                            let dy: f64 = [a[1], a[3], a[5], a[7], a[9]].iter().sum();
                            let (dx6, dy6) = if dx.abs() > dy.abs() {
                                (a[10], -dy)
                            } else {
                                (-dx, a[10])
                            };
                            self.curve_by(a[0], a[1], a[2], a[3], a[4], a[5]);
                            self.curve_by(a[6], a[7], a[8], a[9], dx6, dy6);
                        }
                        other => panic!("no committed CFF uses the Type 2 operator 12 {other}"),
                    }
                    self.stack.clear();
                }
                other => panic!("no committed CFF uses the Type 2 operator {other}"),
            }
        }
    }
}

/// A bare CFF, as the reader reads it.
fn read_bare_cff(label: &str, data: &[u8]) -> Expected {
    let cff = CffFont::read(data).unwrap_or_else(|| panic!("{label}: no CFF"));
    let glyph_count = u16::try_from(cff.charstrings.len()).unwrap_or(u16::MAX);
    let glyphs: Vec<Glyph> = (0..glyph_count)
        .map(|glyph| {
            let read = cff.glyph(glyph);
            Glyph {
                advance: read.map(|(width, _)| width),
                bounds: read.map(|(_, bounds)| bounds),
                scale: cff.scale(glyph),
            }
        })
        .collect();
    let names: Vec<Option<String>> = (0..glyph_count).map(|glyph| cff.name(glyph)).collect();
    let mut chars: BTreeMap<char, u16> = BTreeMap::new();
    if !cff.cid {
        for (glyph, name) in names.iter().enumerate() {
            if let Some(c) = name.as_deref().and_then(|name| name_char(label, name)) {
                chars.entry(c).or_insert(glyph as u16);
            }
        }
    }
    let named = names.iter().any(|name| name.as_deref() != Some(".notdef"));
    let nonsymbolic = !cff.cid
        && named
        && names.iter().all(|name| match name.as_deref() {
            Some(".notdef") => true,
            Some(name) => is_standard_latin_name(name),
            None => false,
        });
    let mut flags = if nonsymbolic { NONSYMBOLIC } else { SYMBOLIC };
    if cff.fixed_pitch {
        flags |= FIXED_PITCH;
    }
    if cff.italic_angle != 0.0 {
        flags |= ITALIC;
    }
    if cff.force_bold() {
        flags |= FORCE_BOLD;
    }
    let top_scale = scale_of(cff.matrix[0]);
    let given = names
        .iter()
        .enumerate()
        .filter_map(|(glyph, name)| Some((name.clone()?, glyph as u16)))
        .collect();
    Expected {
        format: if cff.cid {
            FontFormat::CidKeyedCff
        } else {
            FontFormat::Cff
        },
        glyph_count,
        units_per_em: 1.0 / top_scale,
        font_scale: top_scale,
        font_box: Some(ordered(cff.bbox)),
        glyphs,
        chars: chars.into_iter().collect(),
        names,
        given,
        flags,
        notdef: (glyph_count > 0).then_some(0),
    }
}

// ---- a Type 1 program -------------------------------------------------------

/// The Type 1 book's chapter 7 cipher, decrypting, `skip` leading bytes
/// discarded.
fn decrypt(cipher: &[u8], key: u16, skip: usize) -> Vec<u8> {
    let mut r = key;
    let mut out = Vec::new();
    for (i, &c) in cipher.iter().enumerate() {
        let plain = c ^ (r >> 8) as u8;
        r = (u16::from(c).wrapping_add(r))
            .wrapping_mul(52_845)
            .wrapping_add(22_719);
        if i >= skip {
            out.push(plain);
        }
    }
    out
}

/// The number at the start of `text` after white space, and where it ends.
fn number_after(text: &[u8]) -> Option<(f64, usize)> {
    let start = text.iter().position(|b| !b.is_ascii_whitespace())?;
    let len = text[start..]
        .iter()
        .position(|b| !(b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')))
        .unwrap_or(text.len() - start);
    let value = std::str::from_utf8(&text[start..start + len])
        .ok()?
        .parse()
        .ok()?;
    Some((value, start + len))
}

/// The numbers of the array or procedure a key is defined as, its first
/// definition that opens with `[` or `{`.
fn array_after(text: &[u8], key: &[u8]) -> Option<Vec<f64>> {
    let mut from = 0;
    while let Some(found) = find(&text[from..], key) {
        let after = from + found + key.len();
        from = after;
        let rest = &text[after..];
        let start = rest.iter().position(|b| !b.is_ascii_whitespace())?;
        let close = match rest[start] {
            b'[' => b']',
            b'{' => b'}',
            _ => continue,
        };
        let end = rest[start..].iter().position(|b| *b == close)? + start;
        return std::str::from_utf8(&rest[start + 1..end])
            .ok()?
            .split_whitespace()
            .map(|token| token.parse().ok())
            .collect();
    }
    None
}

/// The token after a key's first definition, as text.
fn token_after<'t>(text: &'t [u8], key: &[u8]) -> Option<&'t [u8]> {
    let mut from = 0;
    while let Some(found) = find(&text[from..], key) {
        let after = from + found + key.len();
        from = after;
        // `/Weight` is not `/WeightVector`'s first seven bytes.
        if text.get(after).is_some_and(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        let rest = &text[after..];
        let start = rest.iter().position(|b| !b.is_ascii_whitespace())?;
        let len = rest[start..]
            .iter()
            .position(|b| b.is_ascii_whitespace() || b"/[]{}()<>".contains(b))
            .unwrap_or(rest.len() - start);
        if len > 0 {
            return Some(&rest[start..start + len]);
        }
    }
    None
}

/// A Type 1 program, decrypted.
struct Type1Font {
    glyphs: Vec<(String, Vec<u8>)>,
    subrs: Vec<Vec<u8>>,
    scale: f64,
    weights: Option<Vec<f64>>,
    bbox: [f64; 4],
    fixed_pitch: bool,
    italic_angle: f64,
    force_bold: bool,
}

impl Type1Font {
    fn read(data: &[u8]) -> Option<Type1Font> {
        let at = find(data, b"eexec")?;
        let clear = &data[..at];
        let rest = &data[at + 5..];
        let start = rest
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        let rest = &rest[start..];
        let cipher: Vec<u8> = if rest.len() >= 4 && rest[..4].iter().all(u8::is_ascii_hexdigit) {
            let digits: Vec<u8> = rest
                .iter()
                .filter_map(|b| (*b as char).to_digit(16).map(|d| d as u8))
                .collect();
            digits.chunks_exact(2).map(|p| (p[0] << 4) | p[1]).collect()
        } else {
            rest.to_vec()
        };
        let private = decrypt(&cipher, 55_665, 4);
        let len_iv = token_after(&private, b"/lenIV")
            .and_then(|t| std::str::from_utf8(t).ok()?.parse::<usize>().ok())
            .unwrap_or(4);

        // `RD` or `-|`, one space, then exactly `len` bytes.
        let binary = |text: &[u8], at: usize| -> Option<(usize, usize)> {
            let (len, used) = number_after(&text[at..])?;
            let after = at + used;
            let token = text[after..]
                .iter()
                .position(|b| !b.is_ascii_whitespace())?
                + after;
            let token_end = text[token..].iter().position(u8::is_ascii_whitespace)? + token;
            Some((token_end + 1, len as usize))
        };

        // `/CharStrings n dict dup begin`, then `/name len RD <len bytes> ND`
        // for each glyph, in order, to the dictionary's `end`: what follows,
        // past `closefile`, is no part of the program.
        let mut glyphs = Vec::new();
        if let Some(at) = find(&private, b"/CharStrings") {
            let mut cursor = at + find(&private[at..], b"begin")? + b"begin".len();
            loop {
                let rest = &private[cursor..];
                let Some(slash) = rest.iter().position(|b| *b == b'/') else {
                    break;
                };
                if find(&rest[..slash], b"end").is_some() {
                    break;
                }
                let name_start = cursor + slash + 1;
                let name_end = private[name_start..]
                    .iter()
                    .position(u8::is_ascii_whitespace)
                    .map_or(private.len(), |n| name_start + n);
                let name = String::from_utf8(private[name_start..name_end].to_vec()).ok()?;
                let (data_start, len) = binary(&private, name_end)?;
                let bytes = private.get(data_start..data_start + len)?;
                glyphs.push((name, decrypt(bytes, 4330, len_iv)));
                cursor = data_start + len;
            }
        }
        let mut subrs: Vec<Vec<u8>> = Vec::new();
        if let Some(at) = find(&private, b"/Subrs") {
            let mut cursor = at;
            while let Some(found) = find(&private[cursor..], b"dup ") {
                let at = cursor + found + 4;
                let Some((index, used)) = number_after(&private[at..]) else {
                    cursor = at;
                    continue;
                };
                let Some((data_start, len)) = binary(&private, at + used) else {
                    cursor = at;
                    continue;
                };
                let Some(bytes) = private.get(data_start..data_start + len) else {
                    break;
                };
                let index = index as usize;
                if subrs.len() <= index {
                    subrs.resize(index + 1, Vec::new());
                }
                subrs[index] = decrypt(bytes, 4330, len_iv);
                cursor = data_start + len;
            }
        }
        let entries_end = [find(&private, b"/Subrs"), find(&private, b"/CharStrings")]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(private.len());
        let bbox = array_after(clear, b"/FontBBox")
            .filter(|b| b.len() == 4)
            .map_or([0.0; 4], |b| [b[0], b[1], b[2], b[3]]);
        Some(Type1Font {
            glyphs,
            subrs,
            scale: scale_of(
                array_after(clear, b"/FontMatrix")
                    .and_then(|m| m.first().copied())
                    .unwrap_or(0.001),
            ),
            weights: array_after(clear, b"/WeightVector").filter(|w| (2..=16).contains(&w.len())),
            bbox,
            fixed_pitch: token_after(clear, b"/isFixedPitch") == Some(b"true"),
            italic_angle: token_after(clear, b"/ItalicAngle")
                .and_then(|t| std::str::from_utf8(t).ok()?.parse().ok())
                .unwrap_or(0.0),
            force_bold: token_after(&private[..entries_end], b"/ForceBold") == Some(b"true"),
        })
    }

    /// A glyph's width and box, by the Type 1 interpreter below.
    fn glyph(&self, glyph: usize) -> Option<(f64, [f64; 4])> {
        let (_, code) = self.glyphs.get(glyph)?;
        let mut run = Type1Run {
            font: self,
            stack: Vec::new(),
            postscript: Vec::new(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            shape: Shape::default(),
            flex: None,
            ended: false,
            steps: 0,
        };
        run.run(code, 0);
        Some((run.width, run.shape.bounds()))
    }
}

/// The Type 1 book's chapter 6 charstring machine, with chapter 8's
/// othersubrs and TN #5015's blends, as far as a box and a width need.
struct Type1Run<'f> {
    font: &'f Type1Font,
    stack: Vec<f64>,
    /// What `callothersubr` leaves for `pop`, the next answer last.
    postscript: Vec<f64>,
    x: f64,
    y: f64,
    width: f64,
    shape: Shape,
    /// Where a flex began, and the points its `rmoveto`s place, between
    /// othersubrs 1 and 0.
    flex: Option<((f64, f64), Vec<(f64, f64)>)>,
    ended: bool,
    steps: u32,
}

impl Type1Run<'_> {
    fn line_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.shape.line_to((self.x, self.y));
    }

    #[allow(clippy::too_many_arguments)]
    fn curve_by(&mut self, dxa: f64, dya: f64, dxb: f64, dyb: f64, dxc: f64, dyc: f64) {
        let from = (self.x, self.y);
        let c1 = (from.0 + dxa, from.1 + dya);
        let c2 = (c1.0 + dxb, c1.1 + dyb);
        let to = (c2.0 + dxc, c2.1 + dyc);
        self.shape.curve_to(from, c1, c2, to);
        (self.x, self.y) = to;
    }

    fn move_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        match &mut self.flex {
            Some((_, points)) => points.push((self.x, self.y)),
            None => self.shape.move_to((self.x, self.y)),
        }
    }

    fn run(&mut self, code: &[u8], depth: u32) {
        assert!(depth <= 10, "a committed charstring nests past ten");
        let mut i = 0;
        while i < code.len() && !self.ended {
            self.steps += 1;
            assert!(self.steps < 1_000_000, "a committed charstring runs away");
            let b = code[i];
            i += 1;
            let a = self.stack.clone();
            match b {
                32..=246 => self.stack.push(f64::from(b) - 139.0),
                247..=250 => {
                    let w = f64::from(code[i]);
                    i += 1;
                    self.stack.push((f64::from(b) - 247.0) * 256.0 + w + 108.0);
                }
                251..=254 => {
                    let w = f64::from(code[i]);
                    i += 1;
                    self.stack.push(-(f64::from(b) - 251.0) * 256.0 - w - 108.0);
                }
                255 => {
                    let v = i32::from_be_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
                    i += 4;
                    self.stack.push(f64::from(v));
                }
                13 => {
                    self.x = a[0];
                    self.y = 0.0;
                    self.width = a[1];
                    self.stack.clear();
                }
                21 => {
                    self.move_by(a[0], a[1]);
                    self.stack.clear();
                }
                22 => {
                    self.move_by(a[0], 0.0);
                    self.stack.clear();
                }
                4 => {
                    self.move_by(0.0, a[0]);
                    self.stack.clear();
                }
                5 => {
                    self.line_by(a[0], a[1]);
                    self.stack.clear();
                }
                6 => {
                    self.line_by(a[0], 0.0);
                    self.stack.clear();
                }
                7 => {
                    self.line_by(0.0, a[0]);
                    self.stack.clear();
                }
                8 => {
                    self.curve_by(a[0], a[1], a[2], a[3], a[4], a[5]);
                    self.stack.clear();
                }
                30 => {
                    self.curve_by(0.0, a[0], a[1], a[2], a[3], 0.0);
                    self.stack.clear();
                }
                31 => {
                    self.curve_by(a[0], 0.0, a[1], a[2], 0.0, a[3]);
                    self.stack.clear();
                }
                9 | 1 | 3 => self.stack.clear(),
                14 => self.ended = true,
                10 => {
                    let number = self.stack.pop().expect("a subroutine number") as usize;
                    let subr = self
                        .font
                        .subrs
                        .get(number)
                        .expect("a committed charstring calls a subroutine it has")
                        .clone();
                    self.run(&subr, depth + 1);
                }
                11 => return,
                12 => {
                    let op = code[i];
                    i += 1;
                    match op {
                        0 | 1 | 2 => self.stack.clear(),
                        7 => {
                            self.x = a[0];
                            self.y = a[1];
                            self.width = a[2];
                            self.stack.clear();
                        }
                        12 => {
                            let divisor = self.stack.pop().expect("a divisor");
                            let dividend = self.stack.pop().expect("a dividend");
                            self.stack.push(dividend / divisor);
                        }
                        16 => self.othersubr(),
                        17 => {
                            let value = self.postscript.pop().expect("a value to pop");
                            self.stack.push(value);
                        }
                        33 => {
                            self.x = a[a.len() - 2];
                            self.y = a[a.len() - 1];
                            self.stack.clear();
                        }
                        other => panic!("no committed Type 1 program uses 12 {other}"),
                    }
                }
                other => panic!("no committed Type 1 program uses the operator {other}"),
            }
        }
    }

    /// `callothersubr`: flex (0 to 2), hint replacement (3) and TN #5015's
    /// blends (14 to 18); an othersubr it does not know hands its arguments
    /// back to `pop`.
    fn othersubr(&mut self) {
        let number = self.stack.pop().expect("an othersubr number") as i64;
        let count = self.stack.pop().expect("an argument count") as usize;
        let args = self.stack.split_off(self.stack.len() - count);
        let answers: Vec<f64> = match number {
            1 => {
                self.flex = Some(((self.x, self.y), Vec::new()));
                Vec::new()
            }
            2 => Vec::new(),
            0 => {
                let (start, points) = self.flex.take().expect("a flex begun");
                assert_eq!(points.len(), 7, "a flex places seven points");
                let p = |n: usize| points[n];
                // The reference point, then the two curves' points, the
                // first curve from where the flex began.
                let mut from = start;
                for curve in [[p(1), p(2), p(3)], [p(4), p(5), p(6)]] {
                    self.shape.curve_to(from, curve[0], curve[1], curve[2]);
                    from = curve[2];
                }
                vec![args[1], args[2]]
            }
            14..=18 => {
                let quantities = [1, 2, 3, 4, 6][(number - 14) as usize];
                let weights = self.font.weights.as_ref().expect("a weight vector");
                let k = weights.len();
                (0..quantities)
                    .map(|q| {
                        let deltas =
                            &args[quantities + q * (k - 1)..quantities + (q + 1) * (k - 1)];
                        let mut value = args[q];
                        for (weight, delta) in weights[1..].iter().zip(deltas) {
                            value += weight * delta;
                        }
                        value
                    })
                    .collect()
            }
            _ => args,
        };
        // `pop` takes them in order: the first answer is pushed last.
        self.postscript.extend(answers.into_iter().rev());
    }
}

/// A Type 1 program, as the reader reads it.
fn read_type1(label: &str, font: &Type1Font) -> Expected {
    let glyph_count = u16::try_from(font.glyphs.len()).unwrap_or(u16::MAX);
    let glyphs = (0..usize::from(glyph_count))
        .map(|glyph| {
            let read = font.glyph(glyph);
            Glyph {
                advance: read.map(|(width, _)| width),
                bounds: read.map(|(_, bounds)| bounds),
                scale: font.scale,
            }
        })
        .collect();
    let names: Vec<Option<String>> = font
        .glyphs
        .iter()
        .map(|(name, _)| Some(name.clone()))
        .collect();
    let mut chars: BTreeMap<char, u16> = BTreeMap::new();
    for (glyph, (name, _)) in font.glyphs.iter().enumerate() {
        if let Some(c) = name_char(label, name) {
            chars.entry(c).or_insert(glyph as u16);
        }
    }
    let named = font.glyphs.iter().any(|(name, _)| name != ".notdef");
    let nonsymbolic = named
        && font
            .glyphs
            .iter()
            .all(|(name, _)| name == ".notdef" || is_standard_latin_name(name));
    let mut flags = if nonsymbolic { NONSYMBOLIC } else { SYMBOLIC };
    if font.fixed_pitch {
        flags |= FIXED_PITCH;
    }
    if font.italic_angle != 0.0 {
        flags |= ITALIC;
    }
    if font.force_bold {
        flags |= FORCE_BOLD;
    }
    let given = names
        .iter()
        .enumerate()
        .filter_map(|(glyph, name)| Some((name.clone()?, glyph as u16)))
        .collect();
    Expected {
        format: FontFormat::Type1,
        glyph_count,
        units_per_em: 1.0 / font.scale,
        font_scale: font.scale,
        font_box: Some(ordered(font.bbox)),
        glyphs,
        chars: chars.into_iter().collect(),
        notdef: font
            .glyphs
            .iter()
            .position(|(name, _)| name == ".notdef")
            .map(|glyph| glyph as u16),
        names,
        given,
        flags,
    }
}

/// What the reader reads a committed file as: `None` for one it reads as no
/// font program at all.
fn read_expected(label: &str, data: &[u8]) -> Option<Expected> {
    if Directory::read(data).is_some() {
        return Some(read_sfnt(label, data, None));
    }
    if data.first() == Some(&1) && CffFont::read(data).is_some() {
        return Some(read_bare_cff(label, data));
    }
    let font = Type1Font::read(data)?;
    Some(read_type1(label, &font))
}

// ---- holding the API to the reading ------------------------------------------

/// Two numbers one stated arithmetic computes, held within 10⁻⁹ of their
/// magnitude.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn close_rect(label: &str, what: &str, got: Option<Rect>, expected: Option<[f64; 4]>) {
    match (got, expected) {
        (None, None) => {}
        (Some(got), Some([x0, y0, x1, y1])) => {
            assert!(
                close(got.x0, x0) && close(got.y0, y0) && close(got.x1, x1) && close(got.y1, y1),
                "{label}: {what}: {got:?} against {:?}",
                [x0, y0, x1, y1]
            );
        }
        (got, expected) => panic!("{label}: {what}: {got:?} against {expected:?}"),
    }
}

fn scaled([x0, y0, x1, y1]: [f64; 4], scale: f64, size: f64) -> [f64; 4] {
    ordered([
        x0 * scale * size,
        y0 * scale * size,
        x1 * scale * size,
        y1 * scale * size,
    ])
}

/// Every answer `metrics` gives, held to `expected`.
fn assert_answers(label: &str, metrics: &FontMetrics, expected: &Expected) {
    assert_eq!(metrics.format(), expected.format, "{label}: format");
    assert_eq!(
        metrics.glyph_count(),
        expected.glyph_count,
        "{label}: glyphs"
    );
    assert!(
        close(metrics.units_per_em(), expected.units_per_em),
        "{label}: units per em {} against {}",
        metrics.units_per_em(),
        expected.units_per_em
    );
    close_rect(label, "font box", metrics.font_box(), expected.font_box);
    close_rect(
        label,
        "font box at 7.25",
        metrics.font_box_at(7.25),
        expected
            .font_box
            .map(|b| scaled(b, expected.font_scale, 7.25)),
    );
    assert_eq!(
        metrics.flags().bits(),
        expected.flags,
        "{label}: flags {:#b} against {:#b}",
        metrics.flags().bits(),
        expected.flags
    );

    // Characters.
    let points: Vec<char> = expected.chars.iter().map(|(c, _)| *c).collect();
    assert_eq!(metrics.code_points(), points, "{label}: code points");
    for (c, glyph) in &expected.chars {
        assert_eq!(metrics.glyph_for_char(*c), Some(*glyph), "{label}: {c:?}");
        assert!(metrics.has_glyph(*c), "{label}: {c:?}");
    }
    for c in ['\u{10FFFF}', '\u{E8FF}', '\u{1D}', '\u{FFFE}'] {
        if !points.contains(&c) {
            assert_eq!(metrics.glyph_for_char(c), None, "{label}: {c:?}");
            assert!(!metrics.has_glyph(c), "{label}: {c:?}");
        }
    }

    // Glyphs.
    for (glyph, read) in expected.glyphs.iter().enumerate() {
        let id = glyph as u16;
        match (metrics.advance(id), read.advance) {
            (None, None) => {}
            (Some(got), Some(want)) => {
                assert!(
                    close(got, want),
                    "{label}: glyph {glyph} advance {got} against {want}"
                );
                let at = metrics.advance_at(id, 9.5).expect("an advance at a size");
                assert!(
                    close(at, want * read.scale * 9.5),
                    "{label}: glyph {glyph} advance at 9.5"
                );
            }
            (got, want) => panic!("{label}: glyph {glyph} advance {got:?} against {want:?}"),
        }
        close_rect(
            label,
            &format!("glyph {glyph} box"),
            metrics.glyph_box(id),
            read.bounds,
        );
        close_rect(
            label,
            &format!("glyph {glyph} box at 9.5"),
            metrics.glyph_box_at(id, 9.5),
            read.bounds.map(|b| scaled(b, read.scale, 9.5)),
        );
        assert_eq!(
            metrics.glyph_name(id),
            expected.names[glyph].as_deref(),
            "{label}: glyph {glyph} name"
        );
    }
    let past = expected.glyph_count;
    if past < u16::MAX {
        assert_eq!(metrics.advance(past), None, "{label}: past the count");
        assert_eq!(metrics.glyph_box(past), None, "{label}: past the count");
        assert_eq!(metrics.glyph_name(past), None, "{label}: past the count");
    }

    // Names both ways: the lowest glyph each is given to.
    let mut lowest: BTreeMap<&str, u16> = BTreeMap::new();
    for (name, glyph) in &expected.given {
        lowest
            .entry(name.as_str())
            .and_modify(|g| *g = (*g).min(*glyph))
            .or_insert(*glyph);
    }
    for (name, glyph) in &lowest {
        assert_eq!(
            metrics.glyph_for_name(name),
            Some(*glyph),
            "{label}: /{name}"
        );
    }
    assert_eq!(
        metrics.glyph_for_name("no such glyph name"),
        None,
        "{label}"
    );

    // A text's length: every character with a glyph, and one without.
    let text: String = points
        .iter()
        .take(300)
        .copied()
        .chain(['\u{10FFFF}'])
        .collect();
    let mut total = 0.0;
    for c in text.chars() {
        let glyph = expected
            .chars
            .iter()
            .find(|(mapped, _)| *mapped == c)
            .map(|(_, glyph)| *glyph)
            .or(expected.notdef);
        if let Some(read) = glyph.and_then(|g| expected.glyphs.get(usize::from(g))) {
            if let Some(advance) = read.advance {
                total += advance * read.scale;
            }
        }
    }
    for size in [1.0, 10.25] {
        let got = metrics.text_length(&text, size);
        assert!(
            close(got, total * size),
            "{label}: text length at {size}: {got} against {}",
            total * size
        );
    }
    assert_eq!(metrics.text_length("", 12.0), 0.0, "{label}: no text");
}

// ---- the exits ---------------------------------------------------------------

/// FT-16.4: every answer, for every committed face, equals what the reader
/// written here reads from the face's own tables.
#[test]
fn each_answer_is_the_faces_own_tables() {
    let faces = committed_faces();
    let mut measured = 0usize;
    let mut glyphs = 0usize;
    let mut characters = 0usize;
    let mut formats: BTreeMap<String, usize> = BTreeMap::new();
    for (label, bytes) in &faces {
        let expected = read_expected(label, bytes);
        let metrics = FontMetrics::from_bytes(bytes.clone());
        let (Some(expected), Ok(metrics)) = (&expected, &metrics) else {
            // A file the reader reads as no font — the truncated eexec seed —
            // is one the API refuses.
            assert!(
                expected.is_none(),
                "{label}: the API refused {:?}",
                metrics.as_ref().err()
            );
            assert_eq!(
                metrics.as_ref().err(),
                Some(&FontMetricsError::NotAFont),
                "{label}"
            );
            continue;
        };
        assert_answers(label, metrics, expected);
        measured += 1;
        glyphs += usize::from(expected.glyph_count);
        characters += expected.chars.len();
        *formats.entry(format!("{:?}", expected.format)).or_default() += 1;
    }
    println!(
        "RAN font_metrics: {measured} of {} committed faces measured, {glyphs} glyphs, \
         {characters} characters, by format {formats:?}",
        faces.len()
    );
    assert!(
        measured >= 12 + 53 + 185 + 4 + 2,
        "every committed face is read: {measured}"
    );
}

/// The WOFF and WOFF 2.0 containers of a committed face answer as the face
/// does, and the face as the reader reads it.
#[test]
fn a_container_answers_as_the_face_it_wraps() {
    let dir = "crates/tinker-pdf-font/tests/woff";
    let face = std::fs::read(root().join(dir).join("synthetic-2.ttf")).expect("the face");
    let expected = read_sfnt("synthetic-2.ttf", &face, None);
    let plain = FontMetrics::from_bytes(face.clone()).expect("the face measures");
    assert_answers("synthetic-2.ttf", &plain, &expected);
    for container in [
        "synthetic-2.woff",
        "synthetic-2-ttf2woff.woff",
        "synthetic-2.woff2",
        "synthetic-2-wawoff2.woff2",
    ] {
        let bytes = std::fs::read(root().join(dir).join(container)).expect("the container");
        let metrics = FontMetrics::from_bytes(bytes).expect("the container measures");
        assert_answers(container, &metrics, &expected);
    }
    // `hmtx` transform 1, against the face it was made from.
    let aligned =
        std::fs::read(root().join(dir).join("synthetic-2-aligned.ttf")).expect("the face");
    let expected = read_sfnt("synthetic-2-aligned.ttf", &aligned, None);
    let bytes = std::fs::read(root().join(dir).join("synthetic-2-aligned-hmtx.woff2"))
        .expect("the container");
    let metrics = FontMetrics::from_bytes(bytes).expect("the container measures");
    assert_answers("synthetic-2-aligned-hmtx.woff2", &metrics, &expected);
    // A container cut short decodes to nothing and is refused by name.
    let bytes = std::fs::read(root().join(dir).join("synthetic-2.woff")).expect("the container");
    assert_eq!(
        FontMetrics::from_bytes(bytes[..40].to_vec()).err(),
        Some(FontMetricsError::Container)
    );
}

/// A Type 42 program's hexadecimal `/sfnts` strings, each without the
/// padding byte TN #5012 section 4.2 ends an odd one with, and its
/// `/CharStrings` keys.
fn type42_parts(program: &[u8]) -> (Vec<u8>, Vec<(String, u16)>) {
    let at = find(program, b"/sfnts").expect("an /sfnts array");
    let end = find(&program[at..], b"]").expect("its end") + at;
    let mut face = Vec::new();
    let mut cursor = at;
    while let Some(open) = program[cursor..end].iter().position(|b| *b == b'<') {
        let start = cursor + open + 1;
        let close = program[start..end]
            .iter()
            .position(|b| *b == b'>')
            .expect("a close")
            + start;
        let digits: Vec<u8> = program[start..close]
            .iter()
            .filter_map(|b| (*b as char).to_digit(16).map(|d| d as u8))
            .collect();
        let mut string: Vec<u8> = digits
            .chunks(2)
            .map(|p| (p[0] << 4) | p.get(1).copied().unwrap_or(0))
            .collect();
        if string.len() % 2 == 1 {
            string.pop();
        }
        face.extend(string);
        cursor = close + 1;
    }
    let keys_at = find(program, b"/CharStrings").expect("a /CharStrings") + 12;
    let keys_end = find(&program[keys_at..], b"end").expect("its end") + keys_at;
    let text = std::str::from_utf8(&program[keys_at..keys_end]).expect("ASCII");
    let mut keys = Vec::new();
    for entry in text.split('/').skip(1) {
        let mut tokens = entry.split_whitespace();
        let (Some(name), Some(index)) = (tokens.next(), tokens.next()) else {
            continue;
        };
        if let Ok(index) = index.parse() {
            keys.push((name.to_string(), index));
        }
    }
    (face, keys)
}

/// A Type 42 program answers as the face its `/sfnts` reassemble, named by
/// its `/CharStrings`, a glyph two keys give answering to both; its binary
/// and split forms answer alike; one cut inside its strings is refused.
#[test]
fn a_type42_program_answers_as_its_face_named_by_its_charstrings() {
    let dir = root().join("fuzz/corpus/type42");
    let hex = std::fs::read(dir.join("hex-one-string.t42")).expect("the seed");
    let (face, keys) = type42_parts(&hex);
    assert!(
        keys.iter()
            .any(|(name, glyph)| name == "space" && *glyph == 2),
        "the seed gives glyph 2 a second key"
    );
    let expected = read_sfnt("hex-one-string.t42", &face, Some(&keys));
    for seed in [
        "hex-one-string.t42",
        "hex-split.t42",
        "binary-split.t42",
        "binary-dash-bar.t42",
    ] {
        let bytes = std::fs::read(dir.join(seed)).expect("the seed");
        let metrics = FontMetrics::from_bytes(bytes).expect("the program measures");
        assert_answers(seed, &metrics, &expected);
    }
    let cut = std::fs::read(dir.join("truncated-in-sfnts.t42")).expect("the seed");
    assert_eq!(
        FontMetrics::from_bytes(cut).err(),
        Some(FontMetricsError::NotAFont)
    );
}

/// Two committed faces put in one collection: each face answers by its
/// index as it answers alone, and an index past them is refused by name.
#[test]
fn a_collections_faces_answer_by_index() {
    let dir = root().join("crates/tinker-pdf-font/data/liberation");
    let faces = [
        std::fs::read(dir.join("LiberationSans-Regular.ttf")).expect("a face"),
        std::fs::read(dir.join("LiberationMono-Bold.ttf")).expect("a face"),
    ];
    // A `ttcf` header of two offsets, then each face's directory with its
    // table offsets moved by where the face now begins.
    let header = 12 + 4 * faces.len();
    let mut starts = Vec::new();
    let mut body = Vec::new();
    for face in &faces {
        let start = header + body.len();
        starts.push(start as u32);
        let mut moved = face.clone();
        let tables = usize::from(be16(face, 4).expect("a count"));
        for i in 0..tables {
            let at = 12 + 16 * i + 8;
            let offset = be32(face, at).expect("an offset");
            moved[at..at + 4].copy_from_slice(&(offset + start as u32).to_be_bytes());
        }
        body.extend_from_slice(&moved);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    let mut collection = b"ttcf".to_vec();
    collection.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    collection.extend_from_slice(&(faces.len() as u32).to_be_bytes());
    for start in &starts {
        collection.extend_from_slice(&start.to_be_bytes());
    }
    collection.extend_from_slice(&body);

    for (index, face) in faces.iter().enumerate() {
        let alone = read_sfnt("alone", face, None);
        let metrics =
            FontMetrics::from_collection(collection.clone(), index as u32).expect("a face");
        assert_answers(&format!("collection face {index}"), &metrics, &alone);
    }
    let first = FontMetrics::from_bytes(collection.clone()).expect("face 0");
    assert_eq!(
        first.glyph_count(),
        read_sfnt("first", &faces[0], None).glyph_count
    );
    assert_eq!(
        FontMetrics::from_collection(collection, 2).err(),
        Some(FontMetricsError::NoSuchFace { faces: 2 })
    );
    assert_eq!(
        FontMetrics::from_collection(faces[0].clone(), 1).err(),
        Some(FontMetricsError::NoSuchFace { faces: 1 })
    );
}

// ---- a document's font ---------------------------------------------------------

/// A one-page document embedding `face` as a TrueType font's `/FontFile2`,
/// its stream's `/Length` stated as `length`, with a Helvetica beside it that
/// embeds nothing and a third font whose program will not decode.
fn document_embedding(face: &[u8], length: usize) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    out.extend_from_slice(
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
/Resources << /Font << /F1 4 0 R /F2 8 0 R /F3 9 0 R >> >> /Contents 7 0 R >>\nendobj\n\
4 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif-Italic \
/FirstChar 65 /LastChar 65 /Widths [667] /FontDescriptor 5 0 R >>\nendobj\n\
5 0 obj\n<< /Type /FontDescriptor /FontName /LiberationSerif-Italic /Flags 32 \
/FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 /CapHeight 700 \
/StemV 80 /FontFile2 6 0 R >>\nendobj\n",
    );
    out.extend_from_slice(format!("6 0 obj\n<< /Length {length} >>\nstream\n").as_bytes());
    out.extend_from_slice(face);
    out.extend_from_slice(
        b"\nendstream\nendobj\n\
7 0 obj\n<< /Length 31 >>\nstream\nBT /F1 12 Tf 10 10 Td (A) Tj ET\nendstream\nendobj\n\
8 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
9 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Broken /FontDescriptor 10 0 R >>\nendobj\n\
10 0 obj\n<< /Type /FontDescriptor /FontName /Broken /Flags 32 /FontFile2 11 0 R >>\nendobj\n\
11 0 obj\n<< /Length 4 /Filter /FlateDecode /DecodeParms << /Predictor 99 >> >>\n\
stream\nabcd\nendstream\nendobj\n\
trailer\n<< /Size 12 /Root 1 0 R >>\n%%EOF\n",
    );
    out
}

fn kinds(document: &Document) -> Vec<String> {
    document
        .warnings()
        .iter()
        .map(|warning| format!("{:?}", warning.kind))
        .collect()
}

/// A `DocumentFont` embedding a Liberation face answers as the face's bytes
/// do, whether its stream states its length or a recovery finds it;
/// measuring adds no warning to the document and takes none from a read
/// after it; nothing embedded and a program that will not decode are refused
/// by name.
#[test]
fn a_document_font_answers_as_its_bytes_do_and_adds_no_warning() {
    let face = std::fs::read(
        root().join("crates/tinker-pdf-font/data/liberation/LiberationSerif-Italic.ttf"),
    )
    .expect("the face");
    let expected = read_sfnt("LiberationSerif-Italic.ttf", &face, None);
    for length in [face.len(), face.len() + 100] {
        let label = format!("embedded, /Length {length}");
        let bytes = document_embedding(&face, length);
        let measured = Document::open(bytes.clone()).expect("it opens");
        let fonts = measured.fonts();
        let embedded = fonts
            .iter()
            .find(|font| font.name == "LiberationSerif-Italic")
            .expect("the embedded font");
        let before = kinds(&measured);
        let metrics = embedded.metrics().expect("the program measures");
        assert_eq!(
            kinds(&measured),
            before,
            "{label}: measuring adds no warning"
        );
        assert_answers(&label, &metrics, &expected);

        let helvetica = fonts
            .iter()
            .find(|font| font.name == "Helvetica")
            .expect("Helvetica");
        assert_eq!(
            helvetica.metrics().err(),
            Some(FontMetricsError::NotEmbedded)
        );
        let broken = fonts
            .iter()
            .find(|font| font.name == "Broken")
            .expect("Broken");
        assert_eq!(broken.metrics().err(), Some(FontMetricsError::Undecodable));
        assert_eq!(
            kinds(&measured),
            before,
            "{label}: refusing adds none either"
        );

        // A read after the measurement reports what it reports in a document
        // nobody measured.
        let _ = embedded.program_bytes();
        let unmeasured = Document::open(bytes).expect("it opens");
        let twin = unmeasured.fonts();
        let _ = twin
            .iter()
            .find(|font| font.name == "LiberationSerif-Italic")
            .expect("the embedded font")
            .program_bytes();
        assert_eq!(
            kinds(&measured),
            kinds(&unmeasured),
            "{label}: nothing taken"
        );
    }
}

// ---- ruling 1 ----------------------------------------------------------------

/// Every answer, asked of whatever a constructor took.
fn ask_everything(metrics: &FontMetrics) {
    let count = metrics.glyph_count();
    for glyph in (0..count.min(48)).chain([count, u16::MAX]) {
        let _ = metrics.advance(glyph);
        let _ = metrics.advance_at(glyph, 11.0);
        let _ = metrics.glyph_box(glyph);
        let _ = metrics.glyph_box_at(glyph, -3.0);
        if let Some(name) = metrics.glyph_name(glyph) {
            let _ = metrics.glyph_for_name(name);
        }
    }
    let points = metrics.code_points();
    for c in points.iter().take(48) {
        let _ = metrics.glyph_for_char(*c);
    }
    let _ = metrics.has_glyph('A');
    let _ = metrics.font_box();
    let _ = metrics.font_box_at(0.0);
    let _ = metrics.flags();
    let _ = metrics.units_per_em();
    let _ = metrics.format();
    let _ = metrics.text_length("Hello, wörld \u{1F600}", 12.5);
}

/// Small committed faces, one of each format, for mutation.
fn small_faces() -> Vec<Vec<u8>> {
    [
        "crates/tinker-pdf-shape/data/text-rendering-tests/fonts/TestGSUBOne.otf",
        "crates/tinker-pdf-shape/data/text-rendering-tests/fonts/TestCMAP13.ttf",
        "crates/tinker-pdf-font/tests/woff/synthetic-2.ttf",
        "crates/tinker-pdf-font/tests/woff/synthetic-2.woff2",
        "fuzz/corpus/cff/cid_keyed.cff",
        "fuzz/corpus/cff/charset_and_encoding.cff",
        "fuzz/corpus/type1/multiple-master.t1",
        "fuzz/corpus/type42/hex-split.t42",
    ]
    .iter()
    .map(|path| std::fs::read(root().join(path)).expect("a committed face"))
    .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Ruling 1: the constructors take arbitrary bytes without panicking,
    /// and every answer of whatever they take does too.
    #[test]
    fn arbitrary_bytes_never_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..2048),
        face in 0u32..4,
    ) {
        if let Ok(metrics) = FontMetrics::from_bytes(bytes.clone()) {
            ask_everything(&metrics);
        }
        if let Ok(metrics) = FontMetrics::from_collection(bytes, face) {
            ask_everything(&metrics);
        }
    }

    /// Ruling 1, over committed faces of every format cut short and with
    /// bytes changed: the readers meet each count and offset a damaged face
    /// can claim.
    #[test]
    fn damaged_faces_never_panic(
        which in 0usize..8,
        cut in any::<proptest::sample::Index>(),
        flips in proptest::collection::vec((any::<proptest::sample::Index>(), any::<u8>()), 0..8),
    ) {
        let mut bytes = small_faces().swap_remove(which);
        let len = cut.index(bytes.len() + 1);
        bytes.truncate(len.max(1));
        for (at, value) in flips {
            let at = at.index(bytes.len());
            bytes[at] ^= value;
        }
        if let Ok(metrics) = FontMetrics::from_bytes(bytes) {
            ask_everything(&metrics);
        }
    }
}
