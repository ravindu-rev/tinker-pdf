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
//! property tables" in `THIRDPARTY.md`.
//!
//! | table | document | chapter 4 "XMP Schemas" | schemas | properties |
//! | --- | --- | --- | ---: | ---: |
//! | [`PREDEFINED_2004`] | *XMP Specification*, January 2004, 94 pp | pp. 37–58 | 11 | 169 |
//! | [`PREDEFINED_2005`] | *XMP Specification*, September 2005, 112 pp | pp. 39–70 | 14 | 274 |
//!
//! Every row was read off the page it is printed on, with the page recorded
//! beside it, and read a second time against that page before it was written
//! here; the per-property page numbers stay out of the table because nothing
//! in the engine reads them. The four value forms below are what the value
//! type column collapses to: `Lang Alt` is a language alternative, anything
//! containing `bag`, `seq` or `alt` is an array, the named structure types
//! (`ResourceRef`, `Dimensions`, `Flash`, `CFAPattern`, `OECF/SFR`,
//! `DeviceSettings`, `Colorant`, `ProjectLink`, `Time`, `Timecode` and the
//! three `…Stretch` parameter structures) are structures, and everything else
//! — `Text`, `Integer`, `Rational`, `Date`, `URI`, the closed and open choices
//! — is a simple value.
//!
//! # What these tables are not — yet
//!
//! **Neither is read as a membership list, and no rule here treats one as
//! one.** ISO 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 require every property to
//! belong to a predefined schema *or* be described by an extension schema, and
//! these tables are now the right half of that answer: the revision each part
//! cites, as that revision printed it. What is still missing is the other
//! half. 6.7.8's extension schemas are a packet's own way of declaring a
//! property these tables cannot know, and a membership rule that ran before
//! that exception was read would report every conforming file that uses one.
//! So membership stays in [`super::STAGED`], which now names what is left of
//! it rather than the table that could not support it, and only the value
//! *type* is read.
//!
//! That restraint is also what makes the value-type half sound. A property a
//! table does not name is skipped, so a name one revision had and the other
//! dropped costs nothing; a property it does name is one the cited revision
//! printed a value type for, and the type is then a claim about that
//! revision's own row.
//!
//! **One name is worth flagging for whoever writes the membership rule.** The
//! suite's fixtures are themselves a list of membership claims — 422 distinct
//! ones across the two parts, each of the form *the property X, which is (not)
//! permitted in \<schema\> in XMP 2004/2005* — and **421 of the 422 agree with
//! these tables exactly**, schema by schema and name by name. The one that
//! does not is `xmpMM:InstanceID`, which `PDF_A-1b` `6-7-2-t09-fail-q` says is
//! permitted in XMP 2004: the string does not occur anywhere in the 94 pages
//! of the January 2004 document, and September 2005 introduces it with an
//! editorial marker its own author left in the file (`<< new InstanceID
//! stuff>>`, p45). So a membership rule reading [`PREDEFINED_2004`] strictly
//! will report that one fixture where the suite would not. That is a
//! disagreement between two published sources, to be recorded in the ledger
//! when the rule lands rather than patched out of the table now.
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
//! It is also the one property the corpus forced, and it is the only place the
//! two tables disagree about a value *form* at all: reading both revisions
//! against the single table these replaced turns up exactly one shared
//! property whose form moves, and this is it.

use super::Part;

/// The shape a property's value takes in the packet.
///
/// Four rather than the specification's several dozen value *types*, because
/// this is what an RDF/XML serialisation can be distinguished into without
/// reading the value: `Rational`, `Date` and `Text` are all one element with
/// text in it. Narrowing further would mean parsing every value, which is a
/// different rule from the one this file supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ValueForm {
    /// A single value: an attribute, or an element whose content is text.
    Simple,
    /// `rdf:Seq`, `rdf:Bag`, or an `rdf:Alt` whose items carry no `xml:lang`.
    Array,
    /// An `rdf:Alt` whose items carry `xml:lang` — XMP's language alternative.
    ///
    /// Separated from [`ValueForm::Array`] because the corpus says to: reading
    /// a bare `rdf:Alt` as satisfying a Lang Alt agrees with **ten fewer**
    /// fixtures across seven suites and gains nothing, so an alternative array
    /// with no language on its items is a finding rather than a leniency.
    LangAlt,
    /// `rdf:parseType="Resource"`, or a nested `rdf:Description`, or the
    /// attribute shorthand — XMP's structure.
    Structure,
}

impl ValueForm {
    /// How a finding names this form, in the words the XMP specification uses
    /// for its own value types.
    pub(super) fn describe(self) -> &'static str {
        match self {
            ValueForm::Simple => "a simple value",
            ValueForm::Array => "an array",
            ValueForm::LangAlt => "a language alternative",
            ValueForm::Structure => "a structure",
        }
    }
}

/// One predefined schema, and the value form each of its properties declares.
pub(super) struct Schema {
    /// The namespace URI, which is what a packet is matched on.
    pub(super) uri: &'static str,
    /// The prefix the specification prints beside the schema, carried so a
    /// finding can name a property the way a reader would write it. Both
    /// revisions call it the *preferred* prefix; it is never matched.
    pub(super) prefix: &'static str,
    /// The properties, **sorted by name**, so the lookup below is a binary
    /// search and the iteration order is one order on every target (ruling 4).
    pub(super) properties: &'static [(&'static str, ValueForm)],
}

/// The value form `local` declares in `uri`, under the revision `part` cites.
///
/// `None` if that revision's table does not name the property — which is not a
/// statement that the property is unknown, only that nothing here can judge
/// it.
pub(super) fn value_form(uri: &str, local: &str, part: Part) -> Option<ValueForm> {
    let schema = table_of(part)?.iter().find(|schema| schema.uri == uri)?;
    let index = schema
        .properties
        .binary_search_by(|(name, _)| (*name).cmp(local))
        .ok()?;
    Some(schema.properties[index].1)
}

/// The table the revision `part` cites, or `None` for a part that cites none.
fn table_of(part: Part) -> Option<&'static [Schema]> {
    match part {
        Part::One => Some(PREDEFINED_2004),
        Part::Two | Part::Three => Some(PREDEFINED_2005),
        // Part 4 asks this file nothing, and `None` is what it should get if
        // it ever does. `xmp::part_carries_the_predefined_schema_rule` is
        // false for it, so the walk that calls `value_form` does not run there
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
            ("ApertureValue", ValueForm::Simple),
            ("BrightnessValue", ValueForm::Simple),
            ("CFAPattern", ValueForm::Structure),
            ("ColorSpace", ValueForm::Simple),
            ("ComponentsConfiguration", ValueForm::Array),
            ("CompressedBitsPerPixel", ValueForm::Simple),
            ("Contrast", ValueForm::Simple),
            ("CustomRendered", ValueForm::Simple),
            ("DateTimeDigitized", ValueForm::Simple),
            ("DateTimeOriginal", ValueForm::Simple),
            ("DeviceSettingDescription", ValueForm::Structure),
            ("DigitalZoomRatio", ValueForm::Simple),
            ("ExifVersion", ValueForm::Simple),
            ("ExposureBiasValue", ValueForm::Simple),
            ("ExposureIndex", ValueForm::Simple),
            ("ExposureMode", ValueForm::Simple),
            ("ExposureProgram", ValueForm::Simple),
            ("ExposureTime", ValueForm::Simple),
            ("FNumber", ValueForm::Simple),
            ("FileSource", ValueForm::Simple),
            ("Flash", ValueForm::Structure),
            ("FlashEnergy", ValueForm::Simple),
            ("FlashpixVersion", ValueForm::Simple),
            ("FocalLength", ValueForm::Simple),
            ("FocalLengthIn35mmFilm", ValueForm::Simple),
            ("FocalPlaneResolutionUnit", ValueForm::Simple),
            ("FocalPlaneXResolution", ValueForm::Simple),
            ("FocalPlaneYResolution", ValueForm::Simple),
            ("GPSAltitude", ValueForm::Simple),
            ("GPSAltitudeRef", ValueForm::Simple),
            ("GPSAreaInformation", ValueForm::Simple),
            ("GPSDOP", ValueForm::Simple),
            ("GPSDestBearing", ValueForm::Simple),
            ("GPSDestBearingRef", ValueForm::Simple),
            ("GPSDestDistance", ValueForm::Simple),
            ("GPSDestDistanceRef", ValueForm::Simple),
            ("GPSDestLatitude", ValueForm::Simple),
            ("GPSDestLongitude", ValueForm::Simple),
            ("GPSDifferential", ValueForm::Simple),
            ("GPSImgDirection", ValueForm::Simple),
            ("GPSImgDirectionRef", ValueForm::Simple),
            ("GPSLatitude", ValueForm::Simple),
            ("GPSLongitude", ValueForm::Simple),
            ("GPSMapDatum", ValueForm::Simple),
            ("GPSMeasureMode", ValueForm::Simple),
            ("GPSProcessingMethod", ValueForm::Simple),
            ("GPSSatellites", ValueForm::Simple),
            ("GPSSpeed", ValueForm::Simple),
            ("GPSSpeedRef", ValueForm::Simple),
            ("GPSStatus", ValueForm::Simple),
            ("GPSTimeStamp", ValueForm::Simple),
            ("GPSTrack", ValueForm::Simple),
            ("GPSTrackRef", ValueForm::Simple),
            ("GPSVersionID", ValueForm::Simple),
            ("GainControl", ValueForm::Simple),
            ("ISOSpeedRatings", ValueForm::Array),
            ("ImageUniqueID", ValueForm::Simple),
            ("LightSource", ValueForm::Simple),
            ("MakerNote", ValueForm::Simple),
            ("MaxApertureValue", ValueForm::Simple),
            ("MeteringMode", ValueForm::Simple),
            ("OECF", ValueForm::Structure),
            ("PixelXDimension", ValueForm::Simple),
            ("PixelYDimension", ValueForm::Simple),
            ("RelatedSoundFile", ValueForm::Simple),
            ("Saturation", ValueForm::Simple),
            ("SceneCaptureType", ValueForm::Simple),
            ("SceneType", ValueForm::Simple),
            ("SensingMethod", ValueForm::Simple),
            ("Sharpness", ValueForm::Simple),
            ("ShutterSpeedValue", ValueForm::Simple),
            ("SpatialFrequencyResponse", ValueForm::Structure),
            ("SpectralSensitivity", ValueForm::Simple),
            ("SubjectArea", ValueForm::Array),
            ("SubjectDistance", ValueForm::Simple),
            ("SubjectDistanceRange", ValueForm::Simple),
            ("SubjectLocation", ValueForm::Array),
            ("UserComment", ValueForm::LangAlt),
            ("WhiteBalance", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/pdf/1.3/",
        prefix: "pdf",
        properties: &[
            ("Keywords", ValueForm::Simple),
            ("PDFVersion", ValueForm::Simple),
            ("Producer", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/photoshop/1.0/",
        prefix: "photoshop",
        properties: &[
            ("AuthorsPosition", ValueForm::Simple),
            ("CaptionWriter", ValueForm::Simple),
            ("Category", ValueForm::Simple),
            ("City", ValueForm::Simple),
            ("Country", ValueForm::Simple),
            ("Credit", ValueForm::Simple),
            ("DateCreated", ValueForm::Simple),
            ("Headline", ValueForm::Simple),
            ("Instructions", ValueForm::Simple),
            ("Source", ValueForm::Simple),
            ("State", ValueForm::Simple),
            ("SupplementalCategories", ValueForm::Simple),
            ("TransmissionReference", ValueForm::Simple),
            ("Urgency", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/tiff/1.0/",
        prefix: "tiff",
        properties: &[
            ("Artist", ValueForm::Simple),
            ("BitsPerSample", ValueForm::Array),
            ("Compression", ValueForm::Simple),
            ("Copyright", ValueForm::LangAlt),
            ("DateTime", ValueForm::Simple),
            ("ImageDescription", ValueForm::LangAlt),
            ("ImageLength", ValueForm::Simple),
            ("ImageWidth", ValueForm::Simple),
            ("Make", ValueForm::Simple),
            ("Model", ValueForm::Simple),
            ("Orientation", ValueForm::Simple),
            ("PhotometricInterpretation", ValueForm::Simple),
            ("PlanarConfiguration", ValueForm::Simple),
            ("PrimaryChromaticities", ValueForm::Array),
            ("ReferenceBlackWhite", ValueForm::Array),
            ("ResolutionUnit", ValueForm::Simple),
            ("SamplesPerPixel", ValueForm::Simple),
            ("Software", ValueForm::Simple),
            ("TransferFunction", ValueForm::Array),
            ("WhitePoint", ValueForm::Array),
            ("XResolution", ValueForm::Simple),
            ("YCbCrCoefficients", ValueForm::Array),
            ("YCbCrPositioning", ValueForm::Simple),
            ("YCbCrSubSampling", ValueForm::Array),
            ("YResolution", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/",
        prefix: "xmp",
        properties: &[
            ("Advisory", ValueForm::Array),
            ("BaseURL", ValueForm::Simple),
            ("CreateDate", ValueForm::Simple),
            ("CreatorTool", ValueForm::Simple),
            ("Identifier", ValueForm::Array),
            ("MetadataDate", ValueForm::Simple),
            ("ModifyDate", ValueForm::Simple),
            ("Nickname", ValueForm::Simple),
            ("Thumbnails", ValueForm::Array),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/bj/",
        prefix: "xmpBJ",
        properties: &[("JobRef", ValueForm::Array)],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/mm/",
        prefix: "xmpMM",
        properties: &[
            ("DerivedFrom", ValueForm::Structure),
            ("DocumentID", ValueForm::Simple),
            ("History", ValueForm::Array),
            ("LastURL", ValueForm::Simple),
            ("ManageTo", ValueForm::Simple),
            ("ManageUI", ValueForm::Simple),
            ("ManagedFrom", ValueForm::Structure),
            ("Manager", ValueForm::Simple),
            ("ManagerVariant", ValueForm::Simple),
            ("RenditionClass", ValueForm::Simple),
            ("RenditionOf", ValueForm::Structure),
            ("RenditionParams", ValueForm::Simple),
            ("SaveID", ValueForm::Simple),
            ("VersionID", ValueForm::Simple),
            ("Versions", ValueForm::Array),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/rights/",
        prefix: "xmpRights",
        properties: &[
            ("Certificate", ValueForm::Simple),
            ("Marked", ValueForm::Simple),
            ("Owner", ValueForm::Array),
            ("UsageTerms", ValueForm::LangAlt),
            ("WebStatement", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/t/pg/",
        prefix: "xmpTPg",
        properties: &[
            ("MaxPageSize", ValueForm::Structure),
            ("NPages", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xmp/Identifier/qual/1.0/",
        prefix: "xmpidq",
        properties: &[("Scheme", ValueForm::Simple)],
    },
    Schema {
        uri: "http://purl.org/dc/elements/1.1/",
        prefix: "dc",
        properties: &[
            ("contributor", ValueForm::Array),
            ("coverage", ValueForm::Simple),
            ("creator", ValueForm::Array),
            ("date", ValueForm::Array),
            ("description", ValueForm::LangAlt),
            ("format", ValueForm::Simple),
            ("identifier", ValueForm::Simple),
            ("language", ValueForm::Array),
            ("publisher", ValueForm::Array),
            ("relation", ValueForm::Array),
            ("rights", ValueForm::LangAlt),
            ("source", ValueForm::Simple),
            ("subject", ValueForm::Array),
            ("title", ValueForm::LangAlt),
            ("type", ValueForm::Array),
        ],
    },
];

/// The fourteen schemas of the **September 2005** revision, sorted by URI: the
/// revision ISO 19005-2 6.6.2.3 and ISO 19005-3 are governed by.
///
/// 274 properties, chapter 4 "XMP Schemas", pages 39-70. Against
/// [`PREDEFINED_2004`]: three whole schemas more — `xmpDM` (57 properties),
/// `crs` (41) and the Exif `aux` namespace (2), which the specification's own
/// changelog lists as added in June 2005 — plus `xmp:Label`, `xmp:Rating`,
/// `xmpMM:InstanceID` and three `xmpTPg` properties; `exif:MakerNote` gone;
/// and two value types changed, of which only
/// `photoshop:SupplementalCategories` changes the **form** a packet must
/// write (`exif:GPSMeasureMode` moves from a closed choice of Integer to
/// Text, and both are simple values).
pub(super) const PREDEFINED_2005: &[Schema] = &[
    Schema {
        uri: "http://ns.adobe.com/camera-raw-settings/1.0/",
        prefix: "crs",
        properties: &[
            ("AutoBrightness", ValueForm::Simple),
            ("AutoContrast", ValueForm::Simple),
            ("AutoExposure", ValueForm::Simple),
            ("AutoShadows", ValueForm::Simple),
            ("BlueHue", ValueForm::Simple),
            ("BlueSaturation", ValueForm::Simple),
            ("Brightness", ValueForm::Simple),
            ("CameraProfile", ValueForm::Simple),
            ("ChromaticAberrationB", ValueForm::Simple),
            ("ChromaticAberrationR", ValueForm::Simple),
            ("ColorNoiseReduction", ValueForm::Simple),
            ("Contrast", ValueForm::Simple),
            ("CropAngle", ValueForm::Simple),
            ("CropBottom", ValueForm::Simple),
            ("CropHeight", ValueForm::Simple),
            ("CropLeft", ValueForm::Simple),
            ("CropRight", ValueForm::Simple),
            ("CropTop", ValueForm::Simple),
            ("CropUnits", ValueForm::Simple),
            ("CropWidth", ValueForm::Simple),
            ("Exposure", ValueForm::Simple),
            ("GreenHue", ValueForm::Simple),
            ("GreenSaturation", ValueForm::Simple),
            ("HasCrop", ValueForm::Simple),
            ("HasSettings", ValueForm::Simple),
            ("LuminanceSmoothing", ValueForm::Simple),
            ("RawFileName", ValueForm::Simple),
            ("RedHue", ValueForm::Simple),
            ("RedSaturation", ValueForm::Simple),
            ("Saturation", ValueForm::Simple),
            ("ShadowTint", ValueForm::Simple),
            ("Shadows", ValueForm::Simple),
            ("Sharpness", ValueForm::Simple),
            ("Temperature", ValueForm::Simple),
            ("Tint", ValueForm::Simple),
            ("ToneCurve", ValueForm::Array),
            ("ToneCurveName", ValueForm::Simple),
            ("Version", ValueForm::Simple),
            ("VignetteAmount", ValueForm::Simple),
            ("VignetteMidpoint", ValueForm::Simple),
            ("WhiteBalance", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/exif/1.0/",
        prefix: "exif",
        properties: &[
            ("ApertureValue", ValueForm::Simple),
            ("BrightnessValue", ValueForm::Simple),
            ("CFAPattern", ValueForm::Structure),
            ("ColorSpace", ValueForm::Simple),
            ("ComponentsConfiguration", ValueForm::Array),
            ("CompressedBitsPerPixel", ValueForm::Simple),
            ("Contrast", ValueForm::Simple),
            ("CustomRendered", ValueForm::Simple),
            ("DateTimeDigitized", ValueForm::Simple),
            ("DateTimeOriginal", ValueForm::Simple),
            ("DeviceSettingDescription", ValueForm::Structure),
            ("DigitalZoomRatio", ValueForm::Simple),
            ("ExifVersion", ValueForm::Simple),
            ("ExposureBiasValue", ValueForm::Simple),
            ("ExposureIndex", ValueForm::Simple),
            ("ExposureMode", ValueForm::Simple),
            ("ExposureProgram", ValueForm::Simple),
            ("ExposureTime", ValueForm::Simple),
            ("FNumber", ValueForm::Simple),
            ("FileSource", ValueForm::Simple),
            ("Flash", ValueForm::Structure),
            ("FlashEnergy", ValueForm::Simple),
            ("FlashpixVersion", ValueForm::Simple),
            ("FocalLength", ValueForm::Simple),
            ("FocalLengthIn35mmFilm", ValueForm::Simple),
            ("FocalPlaneResolutionUnit", ValueForm::Simple),
            ("FocalPlaneXResolution", ValueForm::Simple),
            ("FocalPlaneYResolution", ValueForm::Simple),
            ("GPSAltitude", ValueForm::Simple),
            ("GPSAltitudeRef", ValueForm::Simple),
            ("GPSAreaInformation", ValueForm::Simple),
            ("GPSDOP", ValueForm::Simple),
            ("GPSDestBearing", ValueForm::Simple),
            ("GPSDestBearingRef", ValueForm::Simple),
            ("GPSDestDistance", ValueForm::Simple),
            ("GPSDestDistanceRef", ValueForm::Simple),
            ("GPSDestLatitude", ValueForm::Simple),
            ("GPSDestLongitude", ValueForm::Simple),
            ("GPSDifferential", ValueForm::Simple),
            ("GPSImgDirection", ValueForm::Simple),
            ("GPSImgDirectionRef", ValueForm::Simple),
            ("GPSLatitude", ValueForm::Simple),
            ("GPSLongitude", ValueForm::Simple),
            ("GPSMapDatum", ValueForm::Simple),
            ("GPSMeasureMode", ValueForm::Simple),
            ("GPSProcessingMethod", ValueForm::Simple),
            ("GPSSatellites", ValueForm::Simple),
            ("GPSSpeed", ValueForm::Simple),
            ("GPSSpeedRef", ValueForm::Simple),
            ("GPSStatus", ValueForm::Simple),
            ("GPSTimeStamp", ValueForm::Simple),
            ("GPSTrack", ValueForm::Simple),
            ("GPSTrackRef", ValueForm::Simple),
            ("GPSVersionID", ValueForm::Simple),
            ("GainControl", ValueForm::Simple),
            ("ISOSpeedRatings", ValueForm::Array),
            ("ImageUniqueID", ValueForm::Simple),
            ("LightSource", ValueForm::Simple),
            ("MaxApertureValue", ValueForm::Simple),
            ("MeteringMode", ValueForm::Simple),
            ("OECF", ValueForm::Structure),
            ("PixelXDimension", ValueForm::Simple),
            ("PixelYDimension", ValueForm::Simple),
            ("RelatedSoundFile", ValueForm::Simple),
            ("Saturation", ValueForm::Simple),
            ("SceneCaptureType", ValueForm::Simple),
            ("SceneType", ValueForm::Simple),
            ("SensingMethod", ValueForm::Simple),
            ("Sharpness", ValueForm::Simple),
            ("ShutterSpeedValue", ValueForm::Simple),
            ("SpatialFrequencyResponse", ValueForm::Structure),
            ("SpectralSensitivity", ValueForm::Simple),
            ("SubjectArea", ValueForm::Array),
            ("SubjectDistance", ValueForm::Simple),
            ("SubjectDistanceRange", ValueForm::Simple),
            ("SubjectLocation", ValueForm::Array),
            ("UserComment", ValueForm::LangAlt),
            ("WhiteBalance", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/exif/1.0/aux/",
        prefix: "aux",
        properties: &[
            ("Lens", ValueForm::Simple),
            ("SerialNumber", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/pdf/1.3/",
        prefix: "pdf",
        properties: &[
            ("Keywords", ValueForm::Simple),
            ("PDFVersion", ValueForm::Simple),
            ("Producer", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/photoshop/1.0/",
        prefix: "photoshop",
        properties: &[
            ("AuthorsPosition", ValueForm::Simple),
            ("CaptionWriter", ValueForm::Simple),
            ("Category", ValueForm::Simple),
            ("City", ValueForm::Simple),
            ("Country", ValueForm::Simple),
            ("Credit", ValueForm::Simple),
            ("DateCreated", ValueForm::Simple),
            ("Headline", ValueForm::Simple),
            ("Instructions", ValueForm::Simple),
            ("Source", ValueForm::Simple),
            ("State", ValueForm::Simple),
            ("SupplementalCategories", ValueForm::Array),
            ("TransmissionReference", ValueForm::Simple),
            ("Urgency", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/tiff/1.0/",
        prefix: "tiff",
        properties: &[
            ("Artist", ValueForm::Simple),
            ("BitsPerSample", ValueForm::Array),
            ("Compression", ValueForm::Simple),
            ("Copyright", ValueForm::LangAlt),
            ("DateTime", ValueForm::Simple),
            ("ImageDescription", ValueForm::LangAlt),
            ("ImageLength", ValueForm::Simple),
            ("ImageWidth", ValueForm::Simple),
            ("Make", ValueForm::Simple),
            ("Model", ValueForm::Simple),
            ("Orientation", ValueForm::Simple),
            ("PhotometricInterpretation", ValueForm::Simple),
            ("PlanarConfiguration", ValueForm::Simple),
            ("PrimaryChromaticities", ValueForm::Array),
            ("ReferenceBlackWhite", ValueForm::Array),
            ("ResolutionUnit", ValueForm::Simple),
            ("SamplesPerPixel", ValueForm::Simple),
            ("Software", ValueForm::Simple),
            ("TransferFunction", ValueForm::Array),
            ("WhitePoint", ValueForm::Array),
            ("XResolution", ValueForm::Simple),
            ("YCbCrCoefficients", ValueForm::Array),
            ("YCbCrPositioning", ValueForm::Simple),
            ("YCbCrSubSampling", ValueForm::Array),
            ("YResolution", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/",
        prefix: "xmp",
        properties: &[
            ("Advisory", ValueForm::Array),
            ("BaseURL", ValueForm::Simple),
            ("CreateDate", ValueForm::Simple),
            ("CreatorTool", ValueForm::Simple),
            ("Identifier", ValueForm::Array),
            ("Label", ValueForm::Simple),
            ("MetadataDate", ValueForm::Simple),
            ("ModifyDate", ValueForm::Simple),
            ("Nickname", ValueForm::Simple),
            ("Rating", ValueForm::Simple),
            ("Thumbnails", ValueForm::Array),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/bj/",
        prefix: "xmpBJ",
        properties: &[("JobRef", ValueForm::Array)],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/mm/",
        prefix: "xmpMM",
        properties: &[
            ("DerivedFrom", ValueForm::Structure),
            ("DocumentID", ValueForm::Simple),
            ("History", ValueForm::Array),
            ("InstanceID", ValueForm::Simple),
            ("LastURL", ValueForm::Simple),
            ("ManageTo", ValueForm::Simple),
            ("ManageUI", ValueForm::Simple),
            ("ManagedFrom", ValueForm::Structure),
            ("Manager", ValueForm::Simple),
            ("ManagerVariant", ValueForm::Simple),
            ("RenditionClass", ValueForm::Simple),
            ("RenditionOf", ValueForm::Structure),
            ("RenditionParams", ValueForm::Simple),
            ("SaveID", ValueForm::Simple),
            ("VersionID", ValueForm::Simple),
            ("Versions", ValueForm::Array),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/rights/",
        prefix: "xmpRights",
        properties: &[
            ("Certificate", ValueForm::Simple),
            ("Marked", ValueForm::Simple),
            ("Owner", ValueForm::Array),
            ("UsageTerms", ValueForm::LangAlt),
            ("WebStatement", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xap/1.0/t/pg/",
        prefix: "xmpTPg",
        properties: &[
            ("Colorants", ValueForm::Array),
            ("Fonts", ValueForm::Array),
            ("MaxPageSize", ValueForm::Structure),
            ("NPages", ValueForm::Simple),
            ("PlateNames", ValueForm::Array),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xmp/1.0/DynamicMedia/",
        prefix: "xmpDM",
        properties: &[
            ("absPeakAudioFilePath", ValueForm::Simple),
            ("album", ValueForm::Simple),
            ("altTapeName", ValueForm::Simple),
            ("altTimecode", ValueForm::Structure),
            ("artist", ValueForm::Simple),
            ("audioChannelType", ValueForm::Simple),
            ("audioCompressor", ValueForm::Simple),
            ("audioModDate", ValueForm::Simple),
            ("audioSampleRate", ValueForm::Simple),
            ("audioSampleType", ValueForm::Simple),
            ("beatSpliceParams", ValueForm::Structure),
            ("composer", ValueForm::Simple),
            ("contributedMedia", ValueForm::Array),
            ("copyright", ValueForm::Simple),
            ("duration", ValueForm::Structure),
            ("engineer", ValueForm::Simple),
            ("fileDataRate", ValueForm::Simple),
            ("genre", ValueForm::Simple),
            ("instrument", ValueForm::Simple),
            ("introTime", ValueForm::Structure),
            ("key", ValueForm::Simple),
            ("logComment", ValueForm::Simple),
            ("loop", ValueForm::Simple),
            ("markers", ValueForm::Array),
            ("metadataModDate", ValueForm::Simple),
            ("numberOfBeats", ValueForm::Simple),
            ("outCue", ValueForm::Structure),
            ("projectRef", ValueForm::Structure),
            ("pullDown", ValueForm::Simple),
            ("relativePeakAudioFilePath", ValueForm::Simple),
            ("relativeTimestamp", ValueForm::Structure),
            ("releaseDate", ValueForm::Simple),
            ("resampleParams", ValueForm::Structure),
            ("scaleType", ValueForm::Simple),
            ("scene", ValueForm::Simple),
            ("shotDate", ValueForm::Simple),
            ("shotLocation", ValueForm::Simple),
            ("shotName", ValueForm::Simple),
            ("speakerPlacement", ValueForm::Simple),
            ("startTimecode", ValueForm::Structure),
            ("stretchMode", ValueForm::Simple),
            ("tapeName", ValueForm::Simple),
            ("tempo", ValueForm::Simple),
            ("timeScaleParams", ValueForm::Structure),
            ("timeSignature", ValueForm::Simple),
            ("trackNumber", ValueForm::Simple),
            ("videoAlphaMode", ValueForm::Simple),
            ("videoAlphaPremultipleColor", ValueForm::Structure),
            ("videoAlphaUnityIsTransparent", ValueForm::Simple),
            ("videoColorSpace", ValueForm::Simple),
            ("videoCompressor", ValueForm::Simple),
            ("videoFieldOrder", ValueForm::Simple),
            ("videoFrameRate", ValueForm::Simple),
            ("videoFrameSize", ValueForm::Structure),
            ("videoModDate", ValueForm::Simple),
            ("videoPixelAspectRatio", ValueForm::Simple),
            ("videoPixelDepth", ValueForm::Simple),
        ],
    },
    Schema {
        uri: "http://ns.adobe.com/xmp/Identifier/qual/1.0/",
        prefix: "xmpidq",
        properties: &[("Scheme", ValueForm::Simple)],
    },
    Schema {
        uri: "http://purl.org/dc/elements/1.1/",
        prefix: "dc",
        properties: &[
            ("contributor", ValueForm::Array),
            ("coverage", ValueForm::Simple),
            ("creator", ValueForm::Array),
            ("date", ValueForm::Array),
            ("description", ValueForm::LangAlt),
            ("format", ValueForm::Simple),
            ("identifier", ValueForm::Simple),
            ("language", ValueForm::Array),
            ("publisher", ValueForm::Array),
            ("relation", ValueForm::Array),
            ("rights", ValueForm::LangAlt),
            ("source", ValueForm::Simple),
            ("subject", ValueForm::Array),
            ("title", ValueForm::LangAlt),
            ("type", ValueForm::Array),
        ],
    },
];
