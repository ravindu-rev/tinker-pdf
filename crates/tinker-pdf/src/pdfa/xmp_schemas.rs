//! The predefined XMP schemas' property tables: what value type each property
//! declares, transcribed from a published source.
//!
//! This is the table behind the value-type half of ISO 19005-1 6.7.2 and ISO
//! 19005-2 6.6.2.3 — the staged-rules row of `docs/ROADMAP.md`, whose exit
//! criterion is that the staged count moves down and the agreement ratchet up,
//! one ledger class at a time. It joins the metadata rule group `pdfa/xmp.rs`
//! delivered at milestone 3 of `docs/design/pdfa.md`.
//!
//! # Provenance
//!
//! Every row below comes from **Adobe's own published namespace tables**, the
//! `XMPNamespaces` directory of `github.com/adobe/xmp-docs` at commit
//! `e2573ad7e7959e657b1aed704546e19319cb4f5d`, which is BSD-3-Clause and is
//! recorded under "The predefined XMP schemas' property tables" in
//! `THIRDPARTY.md`. They are a machine-readable statement by the
//! format's owner, which is what makes this a transcription rather than a
//! reconstruction — and the difference is one this repository has measured the
//! price of, in the four Annex B tables that were written from a datastream
//! and were wrong.
//!
//! # What this table is *not*
//!
//! **It is not a membership list, and no rule here treats it as one.** ISO
//! 19005-1 6.7.2 and ISO 19005-2 6.6.2.3 require every property to belong to a
//! predefined schema or be described by an extension schema, and answering
//! that needs the set of names the applicable revision defined. The applicable
//! revision is **XMP 2004**: PDF Association TechNote 0008, "Predefined XMP
//! Properties in PDF/A-1", says so in as many words and adds that this version
//! "differs significantly from earlier and later revisions".
//!
//! Adobe's currently published tables are a later revision, and the veraPDF
//! corpus shows the difference by name. Every one of `xmp:Advisory`,
//! `xmpMM:LastURL`, `xmpMM:RenditionOf`, `xmpMM:SaveID`, `exif:MakerNote`,
//! `exif:ComponentsConfiguration`, `xmpDM:videoModDate`, `xmpDM:audioModDate`,
//! `xmpDM:metadataModDate` and `xmpDM:copyright` appears in a fixture the
//! suite annotates **pass** and in none of these tables, as do the whole of
//! the `xmpidq` and Exif `aux` namespaces. A membership rule reading this
//! table would report each of those conforming files as broken, which is
//! exactly the failure [`super::STAGED`] said was worse than not checking. So
//! membership stays staged and only the value *type* is read.
//!
//! TechNote 0008 is not a way round it either. It enumerates the 2004 set for
//! every schema but one, and §2.10 sends the roughly one hundred Exif
//! properties — the largest schema here, and the one the corpus exercises
//! hardest — back to the XMP 2004 specification itself, which is not
//! obtainable. Its own tables are also unreadable by this engine: 25 pages
//! render and pages 6 and 19 extract **zero characters**, the table cells
//! being lost while the prose around them comes through.
//!
//! # Why the type half is sound where the membership half is not
//!
//! A property this table does not name is skipped, so a name the 2004 revision
//! had and Adobe has since dropped costs nothing. A property it *does* name is
//! one both revisions carry, and the type is then a claim about a row that
//! survived — a much narrower bet, and one the corpus checks from both sides:
//! 289 properties, and across all 2 346 annotated PDF/A files exactly one type
//! disagrees with the conformance suite. [`REVISION_DRIFT`] is that one.

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
    /// The namespace URI, which is what a packet is matched on. The prefix is
    /// only preferred (TechNote 0008 §1.2) and is never matched.
    pub(super) uri: &'static str,
    /// The preferred prefix, carried so a finding can name a property the way
    /// a reader would write it.
    pub(super) prefix: &'static str,
    /// The properties, **sorted by name**, so the lookup below is a binary
    /// search and the iteration order is one order on every target (ruling 4).
    pub(super) properties: &'static [(&'static str, ValueForm)],
}

/// Where the conformance suite and the vendored table disagree, and the suite
/// wins — **per part**, because the parts cite different revisions.
///
/// One row, and it is worth the space because of what finding it cost and what
/// it turned out to say. `photoshop:SupplementalCategories` is an unordered
/// array of Text in Adobe's current table. The veraPDF suite pins it from both
/// sides in *both* places it appears, and the two places disagree:
///
/// | fixture | value written | annotated |
/// | --- | --- | --- |
/// | `PDF_A-1b` `6-7-2-t13-pass-l` | text | conforming |
/// | `PDF_A-1b` `6-7-2-t13-fail-l` | `rdf:Bag` | non-conforming |
/// | `PDF_A-2b` `6-6-2-3-1-t13-pass-l` | `rdf:Bag` | conforming |
/// | `PDF_A-2b` `6-6-2-3-1-t13-fail-l` | `rdf:Seq` | non-conforming |
///
/// So the property was plain Text in the revision PDF/A-1 cites and became an
/// unordered array in the one parts 2 and 3 cite, and the suite asserts both.
/// This was **not** predicted: the override was written for part 1 from the
/// first pair, applied to every part, and the part-2 pair then reported a
/// conforming file — the corpus caught a table this build had just corrected,
/// one clause after the correction.
///
/// That two-sided, two-part pinning is the whole reason this list is allowed
/// to exist. A single fixture agreeing with a reading proves only that the
/// reading is load-bearing, which is a lesson this repository has recently
/// paid for in T.88 6.5.8.2.2; a published conformance suite asserting the
/// conforming *and* the non-conforming spelling, differently under two parts,
/// is a statement about the standards rather than about a decoder. The
/// vendored table above is left **exactly as Adobe publishes it** and every
/// override is named here, so what was transcribed and what was overridden
/// stay separable.
pub(super) const REVISION_DRIFT: &[(Part, &str, &str, ValueForm)] = &[(
    Part::One,
    "http://ns.adobe.com/photoshop/1.0/",
    "SupplementalCategories",
    ValueForm::Simple,
)];

/// The value form `local` declares in `uri`, or `None` if this table does not
/// name the property — which is not a statement that the property is unknown,
/// only that nothing here can judge it.
pub(super) fn value_form(uri: &str, local: &str, part: Part) -> Option<ValueForm> {
    for (drift_part, drift_uri, drift_local, form) in REVISION_DRIFT {
        if *drift_part == part && *drift_uri == uri && *drift_local == local {
            return Some(*form);
        }
    }
    let schema = PREDEFINED.iter().find(|schema| schema.uri == uri)?;
    let index = schema
        .properties
        .binary_search_by(|(name, _)| (*name).cmp(local))
        .ok()?;
    Some(schema.properties[index].1)
}

/// The preferred prefix for a namespace this table knows, for a finding's own
/// text.
pub(super) fn prefix_of(uri: &str) -> Option<&'static str> {
    PREDEFINED
        .iter()
        .find(|schema| schema.uri == uri)
        .map(|schema| schema.prefix)
}

/// The twelve predefined schemas, sorted by URI.
pub(super) const PREDEFINED: &[Schema] = &[
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
        uri: "http://ns.adobe.com/pdf/1.3/",
        prefix: "pdf",
        properties: &[
            ("Keywords", ValueForm::Simple),
            ("PDFVersion", ValueForm::Simple),
            ("Producer", ValueForm::Simple),
            ("Trapped", ValueForm::Simple),
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
            ("ColorMode", ValueForm::Simple),
            ("Country", ValueForm::Simple),
            ("Credit", ValueForm::Simple),
            ("DateCreated", ValueForm::Simple),
            ("DocumentAncestors", ValueForm::Array),
            ("Headline", ValueForm::Simple),
            ("History", ValueForm::Simple),
            ("ICCProfile", ValueForm::Simple),
            ("Instructions", ValueForm::Simple),
            ("Source", ValueForm::Simple),
            ("State", ValueForm::Simple),
            ("SupplementalCategories", ValueForm::Array),
            ("TextLayers", ValueForm::Array),
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
            ("Ingredients", ValueForm::Array),
            ("InstanceID", ValueForm::Simple),
            ("ManageTo", ValueForm::Simple),
            ("ManageUI", ValueForm::Simple),
            ("ManagedFrom", ValueForm::Structure),
            ("Manager", ValueForm::Simple),
            ("ManagerVariant", ValueForm::Simple),
            ("OriginalDocumentID", ValueForm::Simple),
            ("Pantry", ValueForm::Array),
            ("RenditionClass", ValueForm::Simple),
            ("RenditionParams", ValueForm::Simple),
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
            ("Tracks", ValueForm::Array),
            ("absPeakAudioFilePath", ValueForm::Simple),
            ("album", ValueForm::Simple),
            ("altTapeName", ValueForm::Simple),
            ("altTimecode", ValueForm::Structure),
            ("artist", ValueForm::Simple),
            ("audioChannelType", ValueForm::Simple),
            ("audioCompressor", ValueForm::Simple),
            ("audioSampleRate", ValueForm::Simple),
            ("audioSampleType", ValueForm::Simple),
            ("beatSpliceParams", ValueForm::Structure),
            ("cameraAngle", ValueForm::Simple),
            ("cameraLabel", ValueForm::Simple),
            ("cameraModel", ValueForm::Simple),
            ("cameraMove", ValueForm::Simple),
            ("client", ValueForm::Simple),
            ("comment", ValueForm::Simple),
            ("composer", ValueForm::Simple),
            ("contributedMedia", ValueForm::Array),
            ("director", ValueForm::Simple),
            ("directorPhotography", ValueForm::Simple),
            ("discNumber", ValueForm::Simple),
            ("duration", ValueForm::Structure),
            ("engineer", ValueForm::Simple),
            ("fileDataRate", ValueForm::Simple),
            ("genre", ValueForm::Simple),
            ("good", ValueForm::Simple),
            ("instrument", ValueForm::Simple),
            ("introTime", ValueForm::Structure),
            ("key", ValueForm::Simple),
            ("logComment", ValueForm::Simple),
            ("loop", ValueForm::Simple),
            ("lyrics", ValueForm::Simple),
            ("markers", ValueForm::Array),
            ("numberOfBeats", ValueForm::Simple),
            ("outCue", ValueForm::Structure),
            ("partOfCompilation", ValueForm::Simple),
            ("projectName", ValueForm::Simple),
            ("projectRef", ValueForm::Structure),
            ("pullDown", ValueForm::Simple),
            ("relativePeakAudioFilePath", ValueForm::Simple),
            ("relativeTimestamp", ValueForm::Structure),
            ("releaseDate", ValueForm::Simple),
            ("resampleParams", ValueForm::Structure),
            ("scaleType", ValueForm::Simple),
            ("scene", ValueForm::Simple),
            ("shotDate", ValueForm::Simple),
            ("shotDay", ValueForm::Simple),
            ("shotLocation", ValueForm::Simple),
            ("shotName", ValueForm::Simple),
            ("shotNumber", ValueForm::Simple),
            ("shotSize", ValueForm::Simple),
            ("speakerPlacement", ValueForm::Simple),
            ("startTimecode", ValueForm::Structure),
            ("stretchMode", ValueForm::Simple),
            ("takeNumber", ValueForm::Simple),
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
            ("videoPixelAspectRatio", ValueForm::Simple),
            ("videoPixelDepth", ValueForm::Simple),
        ],
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
