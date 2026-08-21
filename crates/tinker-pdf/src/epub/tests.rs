//! The OCF layer's own arithmetic, out of an archive (gap 31, milestone 3).
//!
//! Everything here is a pure function over a path or a few bytes of markup.
//! What needs a real ZIP — the routing, §4.3.2's five clauses, the refusals a
//! caller sees and the inflation budget — is in `tests/epub_ocf.rs`, because
//! those are claims about `Document::open` and are worth making through the
//! door a host uses.

use tinker_pdf_xml::Limits as XmlLimits;
use tinker_pdf_zip::{Entry, Method};

use super::ocf::{
    classify, parse_container, parse_encryption, resolve_reference, Item, Obfuscation, PathDefect,
    Reserved, RootfileRef, ADOBE_OBFUSCATION, CONTAINER_ITEM, IDPF_OBFUSCATION, OCF_ROOT,
};
use super::package::{
    parse as parse_package, CoreMediaType, FallbackDefect, Package, PackageDefect, PackageVersion,
    Property,
};
use super::{
    BookLayout, BookOptionDefect, Limits, DEFAULT_FONT_SIZE, DEFAULT_PAGE, MAX_EPUB_FALLBACK_DEPTH,
    MAX_OCF_PATH_LEN, MAX_PAGE_SIDE,
};
use crate::cbz::ArchiveRefusal;

fn limits() -> Limits {
    Limits::DEFAULT
}

// ---- Section 4.2.5, and the base that is not the referring document ---------

/// **The off-by-one-segment, both directions.**
///
/// §4.2.6.3.1 defines `full-path` as a path from the container root, so it is
/// the one reference in the format whose base is not the file it is written in.
/// Resolving it against `META-INF/container.xml` — which is what every other
/// reference in an EPUB does, and what a first implementation therefore writes
/// — puts a `META-INF/` in front of it.
///
/// Both producers in milestone 1's committed corpus are here, and they are the
/// reason this cannot be waved through: one writes its package document under a
/// directory and the other at the archive root, so a build that got the base
/// wrong is wrong about both rather than about an exotic one.
#[test]
fn a_rootfile_resolves_against_the_container_root_and_not_against_meta_inf() {
    let l = limits();
    for full_path in ["EPUB/package.opf", "content.opf", "OEBPS/content.opf"] {
        let right = resolve_reference(OCF_ROOT, full_path, &l).expect("a container path");
        assert_eq!(
            right, full_path,
            "the root-relative answer is the path itself"
        );

        // And the tempting wrong answer, asserted rather than described: it
        // resolves, it is a legal container path, and it names a file no book
        // holds. That is why the refusal for it has its own name.
        let wrong = resolve_reference(CONTAINER_ITEM, full_path, &l).expect("also resolves");
        assert_eq!(wrong, format!("META-INF/{full_path}"));
        assert_ne!(right, wrong);
    }
}

/// And the general rule the exception is an exception to: §4.2.5 resolves
/// against the **referring document**, cutting at its last separator.
///
/// This is what milestone 4's manifest `href`s and milestone 8's
/// cross-references need, and it is proved here rather than there because the
/// function is written here — a contract exercised only by its first caller is
/// a contract that means whatever that caller happened to want.
#[test]
fn a_reference_resolves_against_the_document_that_wrote_it() {
    let l = limits();
    let base = "EPUB/text/chapter1.xhtml";
    for (reference, want) in [
        ("chapter2.xhtml", "EPUB/text/chapter2.xhtml"),
        ("./chapter2.xhtml", "EPUB/text/chapter2.xhtml"),
        ("../images/cover.png", "EPUB/images/cover.png"),
        ("../../toc.ncx", "toc.ncx"),
        ("sub/deeper/../a.css", "EPUB/text/sub/a.css"),
        // The fragment is not part of the path, and nearly every real `href`
        // carries one.
        ("chapter2.xhtml#part3", "EPUB/text/chapter2.xhtml"),
    ] {
        assert_eq!(
            resolve_reference(base, reference, &l).as_deref(),
            Ok(want),
            "{reference} against {base}"
        );
    }
}

/// Each of §4.2.3's rules refuses by its **own** name.
///
/// One assertion per rule rather than a sweep asserting "it failed", because a
/// test that only knows a reference was rejected cannot tell a working check
/// from a deleted one — and two of these map to different `ArchiveRefusal`s,
/// so the distinction is visible to a caller as well as to this file.
#[test]
fn each_of_the_path_restrictions_refuses_by_its_own_name() {
    let l = limits();
    let cases: &[(&str, PathDefect)] = &[
        ("", PathDefect::Empty),
        ("#fragment-only", PathDefect::Empty),
        ("/EPUB/package.opf", PathDefect::Absolute),
        ("//example.com/package.opf", PathDefect::Absolute),
        ("http://example.com/package.opf", PathDefect::Scheme),
        ("data:text/xml,%3Ca/%3E", PathDefect::Scheme),
        ("../package.opf", PathDefect::ClimbsOut),
        ("EPUB/../../package.opf", PathDefect::ClimbsOut),
        ("EPUB//package.opf", PathDefect::EmptySegment),
        ("EPUB/", PathDefect::EmptySegment),
        ("EPUB/pack?age.opf", PathDefect::RestrictedCharacter),
        ("EPUB/pack*age.opf", PathDefect::RestrictedCharacter),
        ("EPUB/pack|age.opf", PathDefect::RestrictedCharacter),
        ("EPUB/pack\"age.opf", PathDefect::RestrictedCharacter),
        ("EPUB/pack<age>.opf", PathDefect::RestrictedCharacter),
        ("EPUB/pack\\age.opf", PathDefect::RestrictedCharacter),
        ("EPUB/package.opf\u{7f}", PathDefect::RestrictedCharacter),
        ("EPUB/package.", PathDefect::RestrictedCharacter),
        // A colon in a later segment is not a scheme and is still forbidden.
        ("EPUB/pack:age.opf", PathDefect::RestrictedCharacter),
    ];
    for (reference, want) in cases {
        assert_eq!(
            resolve_reference(OCF_ROOT, reference, &l),
            Err(*want),
            "{reference:?}"
        );
    }

    // And the control: the same shapes with the offending byte removed resolve,
    // so what the table above measures is the rule and not the fixture.
    for reference in ["EPUB/package.opf", "EPUB/pack-age.opf", "a/b/c/d.opf"] {
        assert!(
            resolve_reference(OCF_ROOT, reference, &l).is_ok(),
            "{reference}"
        );
    }
}

/// A `..` that climbs above the root is **refused**, where RFC 3986 clamps.
///
/// The two resolvers in this repository deliberately disagree, and the
/// disagreement is asserted rather than described: `opc::resolve_reference` is
/// RFC 3986 §5.2.4 and discards the over-climbing segment, which gap 30's
/// milestone 8 recorded as the standard's own behaviour. Inside a container
/// that silently renames a reference to a **different** resource, which may
/// well exist — so §4.2.3, which forbids `..` in a container path at all, is
/// taken at its word here.
#[test]
fn a_climb_above_the_container_root_is_refused_where_rfc_3986_clamps() {
    let l = limits();
    assert_eq!(
        resolve_reference("a/b.xhtml", "../../../secret.opf", &l),
        Err(PathDefect::ClimbsOut)
    );
    // The other resolver, on the same shape, keeps going and lands on a name.
    assert_eq!(
        crate::xps::opc::resolve_reference("/a/b.xml", "../../../secret.opf").as_deref(),
        Some("/secret.opf")
    );
}

/// Percent-escapes are decoded per segment, after dot removal, and a decoded
/// separator does not become one.
///
/// The order is the load-bearing part. RFC 3986 removes dot segments **before**
/// decoding, so `%2E%2E` is a directory called `..` and not a climb; and a
/// `%2F` decodes to a `/` that belongs inside its segment rather than splitting
/// it, which is why such a segment is refused instead of quietly becoming two.
#[test]
fn a_percent_escape_is_decoded_per_segment_and_a_decoded_separator_is_refused() {
    let l = limits();
    assert_eq!(
        resolve_reference(OCF_ROOT, "EPUB/my%20book.opf", &l).as_deref(),
        Ok("EPUB/my book.opf")
    );
    assert_eq!(
        resolve_reference(OCF_ROOT, "EPUB/caf%C3%A9.opf", &l).as_deref(),
        Ok("EPUB/caf\u{e9}.opf")
    );
    // The order, proved by the one input the two orders disagree about.
    // Decoding first turns `a/%2E%2E/b.opf` into `a/../b.opf` and resolves it
    // to `b.opf`; removing dot segments first leaves a **segment** spelled
    // `..`, which §4.2.3 does not allow a file to be called. The control below
    // is the same shape written honestly, and it does resolve — so what this
    // pair measures is the order and not a blanket refusal.
    assert_eq!(
        resolve_reference(OCF_ROOT, "a/%2E%2E/b.opf", &l),
        Err(PathDefect::RestrictedCharacter)
    );
    assert_eq!(
        resolve_reference(OCF_ROOT, "a/../b.opf", &l).as_deref(),
        Ok("b.opf")
    );
    assert_eq!(
        resolve_reference(OCF_ROOT, "EPUB%2F..%2Fpackage.opf", &l),
        Err(PathDefect::RestrictedCharacter)
    );
    // Escapes that spell no UTF-8 are kept as written rather than replaced,
    // because a lossily rewritten name resolves to a file nobody wrote.
    assert_eq!(
        resolve_reference(OCF_ROOT, "EPUB/%FF%FE.opf", &l).as_deref(),
        Ok("EPUB/%FF%FE.opf")
    );
}

/// The length cap is charged against the reference **as written**, before the
/// merge and before dot removal.
///
/// A permit is what has been promised — `tinker-pdf-zip`'s posture. Charging
/// after the merge would mean building the whole path first and refusing it
/// second, which is the allocation the cap exists to prevent.
#[test]
fn a_content_path_past_the_length_cap_is_refused_before_it_is_merged() {
    let l = limits();
    let at_the_cap = format!("{}.opf", "a".repeat(MAX_OCF_PATH_LEN - 4));
    assert_eq!(at_the_cap.len(), MAX_OCF_PATH_LEN);
    assert!(resolve_reference(OCF_ROOT, &at_the_cap, &l).is_ok());

    let one_past = format!("a{at_the_cap}");
    assert_eq!(one_past.len(), MAX_OCF_PATH_LEN + 1);
    assert_eq!(
        resolve_reference(OCF_ROOT, &one_past, &l),
        Err(PathDefect::TooLong)
    );

    // **The charge is before the merge, and this is the input that says so.**
    // Written out it is nine bytes past the cap; after dot removal it is five
    // bytes long. A build that charged only the finished path would allocate
    // the whole reference, resolve it, and let it through — which is the
    // allocation the cap exists to prevent, so a cap that fires only on the
    // answer is a cap that fires after the damage.
    let climbing = format!("{}/../x.opf", "a".repeat(MAX_OCF_PATH_LEN));
    assert!(climbing.len() > MAX_OCF_PATH_LEN);
    assert_eq!(
        resolve_reference(OCF_ROOT, &climbing, &l),
        Err(PathDefect::TooLong)
    );

    // And the merged form is charged too, so a short reference under a long
    // base cannot get past it either. The two checks are independent: this one
    // is the only thing that sees a base the caller chose, and the one above is
    // the only thing that sees a reference that shrinks.
    let base = format!("{}/x.xhtml", "b".repeat(MAX_OCF_PATH_LEN - 8));
    assert_eq!(
        resolve_reference(&base, "chapter.xhtml", &l),
        Err(PathDefect::TooLong)
    );
}

// ---- META-INF's six names, and the seventh file -----------------------------

/// The six are recognised, a seventh file there is not one of them, and the
/// comparison is **case-sensitive**.
///
/// The case-sensitivity is §4.2.3's, and it is the near miss that matters:
/// `META-INF/Container.xml` in a comic archive must stay a comic, and a build
/// that folded ASCII case — which is what OPC 6.2.2.3 does one module over —
/// would route it into this layer and refuse it as a broken book.
#[test]
fn the_six_reserved_names_are_recognised_and_a_seventh_file_is_not() {
    for (item, want) in [
        ("META-INF/container.xml", Some(Reserved::Container)),
        ("META-INF/encryption.xml", Some(Reserved::Encryption)),
        ("META-INF/manifest.xml", Some(Reserved::Manifest)),
        ("META-INF/metadata.xml", Some(Reserved::Metadata)),
        ("META-INF/rights.xml", Some(Reserved::Rights)),
        ("META-INF/signatures.xml", Some(Reserved::Signatures)),
        // The file one of milestone 1's two producers writes into every book.
        ("META-INF/com.apple.ibooks.display-options.xml", None),
        ("META-INF/Container.xml", None),
        ("META-INF/CONTAINER.XML", None),
        ("META-INF/sub/container.xml", None),
        ("container.xml", None),
    ] {
        assert_eq!(Reserved::from_item(item), want, "{item}");
    }
}

/// And classification, which is a different question from recognition: an entry
/// is one of five things and the directory record is the one a walk trips over.
#[test]
fn an_entry_is_classified_by_its_name_and_a_directory_record_is_not_a_file() {
    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            method: Method::Stored,
            crc: Some(0),
            compressed_size: 0,
            uncompressed_size: 0,
            encrypted: false,
            streamed: false,
            header_offset: 0,
            index: 0,
        }
    }
    for (name, want) in [
        ("mimetype", Item::Mimetype),
        (
            "META-INF/container.xml",
            Item::Reserved(Reserved::Container),
        ),
        (
            "META-INF/com.apple.ibooks.display-options.xml",
            Item::UnreservedMetaInf,
        ),
        // Written by one of the two producers, deflated, zero bytes long. A
        // walk that took every entry under `META-INF/` for a file meets this
        // one first.
        ("META-INF/", Item::Directory),
        ("EPUB/", Item::Directory),
        ("EPUB/package.opf", Item::Resource),
        // Not the directory: `starts_with("META-INF")` without the separator
        // would have said otherwise.
        ("META-INFORMATION.txt", Item::Resource),
    ] {
        assert_eq!(classify(&entry(name)), want, "{name}");
    }
}

// ---- container.xml ----------------------------------------------------------

fn container(rootfiles: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\n\
         <container version=\"1.0\" \
         xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
         <rootfiles>{rootfiles}</rootfiles></container>"
    )
    .into_bytes()
}

/// Every `<rootfile>` comes back in document order, media type and all.
///
/// Choosing the default rendition here would throw away what §4.2.6.3.1 allows
/// a container to say — more than one rendition, of more than one media type —
/// and milestone 12's fixed-layout work is the caller that will need the rest.
#[test]
fn container_xml_yields_every_rootfile_in_document_order() {
    let bytes = container(
        "<rootfile full-path=\"EPUB/package.opf\" \
         media-type=\"application/oebps-package+xml\"/>\
         <rootfile full-path=\"other/thing.xml\" media-type=\"application/x-other\"/>",
    );
    let parsed = parse_container(&bytes, &XmlLimits::DEFAULT).expect("a container");
    assert_eq!(
        parsed,
        vec![
            RootfileRef {
                full_path: "EPUB/package.opf".to_owned(),
                media_type: "application/oebps-package+xml".to_owned(),
            },
            RootfileRef {
                full_path: "other/thing.xml".to_owned(),
                media_type: "application/x-other".to_owned(),
            },
        ]
    );
}

/// A root that is not `container` in OCF's namespace is not this file.
///
/// Both halves are asserted, because they are two rules: the right element in
/// the wrong namespace is somebody else's `<container>`, and the wrong element
/// in the right namespace is not §4.2.6.3.1's document at all. A build checking
/// only the local name would pass the first.
///
/// **The first two cases are the ones that made this test able to fail**, and
/// they were added after the injection matrix found the root check was doing
/// nothing any test could see. The obvious fixtures — a wrong root over an
/// empty `<rootfiles>`, or over one whose children are in the wrong namespace —
/// are refused by `out.is_empty()` whether the root is checked or not, so
/// deleting the check entirely left every one of them passing. What separates
/// them is a **usable `<rootfile>` under a root that is not a container**: the
/// correct build refuses it and a build without the check hands back a package
/// document, which is the shape a `META-INF/container.xml` holding somebody
/// else's XML has.
#[test]
fn a_container_whose_root_is_not_the_container_element_is_refused() {
    // The right namespace, the wrong element, and a `<rootfile>` that would
    // otherwise resolve.
    let wrong_element_usable =
        b"<manifest xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles>\
        <rootfile full-path=\"a.opf\" media-type=\"application/oebps-package+xml\"/>\
        </rootfiles></manifest>";
    assert_eq!(
        parse_container(wrong_element_usable, &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );
    // The right element, the wrong namespace, and children that *are* in OCF's
    // — which is what a document borrowing the name from another vocabulary
    // looks like.
    let wrong_namespace_usable = b"<container xmlns=\"urn:example:other\" \
        xmlns:ocf=\"urn:oasis:names:tc:opendocument:xmlns:container\"><ocf:rootfiles>\
        <ocf:rootfile full-path=\"a.opf\" media-type=\"application/oebps-package+xml\"/>\
        </ocf:rootfiles></container>";
    assert_eq!(
        parse_container(wrong_namespace_usable, &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );

    let wrong_namespace = b"<container xmlns=\"urn:example:other\"><rootfiles>\
        <rootfile full-path=\"a.opf\" media-type=\"application/oebps-package+xml\"/>\
        </rootfiles></container>";
    assert_eq!(
        parse_container(wrong_namespace, &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );
    let wrong_element = b"<manifest xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
        </manifest>";
    assert_eq!(
        parse_container(wrong_element, &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );
    assert_eq!(
        parse_container(b"<container", &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );
    // Well formed, in the right namespace, and naming nothing.
    assert_eq!(
        parse_container(&container(""), &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );

    // And the control the two new cases need: the same shape with the root put
    // right resolves, so what they measure is the root and not the payload.
    let right = parse_container(
        &container("<rootfile full-path=\"a.opf\" media-type=\"application/oebps-package+xml\"/>"),
        &XmlLimits::DEFAULT,
    )
    .expect("a container");
    assert_eq!(right.len(), 1);
}

/// An element the schema allows and this build has no opinion about is
/// **ignored**, not refused.
///
/// §4.2.6.3.1's own grammar puts a `<links>` element beside `<rootfiles>`, and
/// milestone 1 found a producer writing a file into `META-INF` that no version
/// of the specification names. A reader that refuses what it has not been
/// introduced to refuses the future; what it may not do is ignore the root,
/// which the test above pins.
#[test]
fn an_element_this_build_does_not_know_is_ignored_rather_than_refused() {
    let bytes = b"<container version=\"1.0\" \
        xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
        <rootfiles>\
        <rootfile full-path=\"EPUB/package.opf\" \
        media-type=\"application/oebps-package+xml\"/></rootfiles>\
        <links><link href=\"a\" rel=\"b\"/></links></container>";
    let parsed = parse_container(bytes, &XmlLimits::DEFAULT).expect("a container");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].full_path, "EPUB/package.opf");
}

/// A `<rootfile>` missing a required attribute is skipped and the container is
/// still the book it is.
#[test]
fn a_rootfile_missing_an_attribute_is_skipped_rather_than_fatal() {
    let bytes = container(
        "<rootfile media-type=\"application/oebps-package+xml\"/>\
         <rootfile full-path=\"EPUB/package.opf\" \
         media-type=\"application/oebps-package+xml\"/>",
    );
    let parsed = parse_container(&bytes, &XmlLimits::DEFAULT).expect("a container");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].full_path, "EPUB/package.opf");
}

/// `container.xml` is read under [`tinker_pdf_xml::Doctype::Refuse`], which is
/// milestone 2's relaxation **not** applied here.
///
/// That mode exists because 100 % of one producer's XHTML content documents
/// carry a document type declaration. Milestone 1 measured **zero** on any
/// `container.xml` in either corpus, so widening the grammar for this file
/// would buy nothing and cost the one defence that keeps the four committed
/// bombs out.
#[test]
fn a_doctype_on_container_xml_is_refused_rather_than_skipped() {
    let bytes = b"<!DOCTYPE container>\
        <container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
        <rootfiles><rootfile full-path=\"a.opf\" \
        media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    assert_eq!(
        parse_container(bytes, &XmlLimits::DEFAULT),
        Err(ArchiveRefusal::UnreadableContainer)
    );
}

// ---- encryption.xml ---------------------------------------------------------

fn encryption(body: &str) -> Vec<u8> {
    format!(
        "<encryption xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" \
         xmlns:enc=\"http://www.w3.org/2001/04/xmlenc#\">{body}</encryption>"
    )
    .into_bytes()
}

fn encrypted_data(algorithm: &str, uri: &str) -> String {
    format!(
        "<enc:EncryptedData><enc:EncryptionMethod Algorithm=\"{algorithm}\"/>\
         <enc:CipherData><enc:CipherReference URI=\"{uri}\"/></enc:CipherData>\
         </enc:EncryptedData>"
    )
}

/// Both obfuscations are read, and the **resource each covers** is read with
/// them.
///
/// Collecting the `<CipherReference>` is what makes this a parse rather than a
/// substring search for two URLs — and a substring search is exactly what would
/// survive every other test in this file.
#[test]
fn the_two_obfuscations_are_read_and_the_resources_they_cover_are_named() {
    let bytes = encryption(&format!(
        "{}{}",
        encrypted_data(IDPF_OBFUSCATION, "EPUB/fonts/serif.otf"),
        encrypted_data(ADOBE_OBFUSCATION, "EPUB/fonts/sans.otf"),
    ));
    let parsed = parse_encryption(&bytes, &limits()).expect("obfuscation only");
    let named: Vec<(&str, Obfuscation)> = parsed
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e.algorithm))
        .collect();
    assert_eq!(
        named,
        [
            ("EPUB/fonts/serif.otf", Obfuscation::Idpf),
            ("EPUB/fonts/sans.otf", Obfuscation::Adobe),
        ]
    );
}

/// An algorithm that is not one of the two is refused **by name**.
///
/// AES-256 here rather than a made-up URL, because a book encrypted with a key
/// this engine does not have is the case the refusal exists for, and it is a
/// real thing a real distributor produces.
#[test]
fn an_algorithm_that_is_not_an_obfuscation_is_refused_by_name() {
    let bytes = encryption(&encrypted_data(
        "http://www.w3.org/2001/04/xmlenc#aes256-cbc",
        "EPUB/text/chapter1.xhtml",
    ));
    assert_eq!(
        parse_encryption(&bytes, &limits()),
        Err(ArchiveRefusal::EncryptedResources)
    );
}

/// And an `<EncryptedData>` naming **no** algorithm is refused by the same
/// name, which is a different fact.
///
/// XML Encryption makes `<EncryptionMethod>` optional, on the grounds that the
/// key may imply it. This build has no key, so an algorithm it cannot see is
/// one it cannot agree to — and a reader that treated the absence as "nothing
/// to worry about" would hand ciphertext to a font parser.
#[test]
fn an_encrypted_data_that_names_no_algorithm_is_refused_too() {
    let bytes = encryption(
        "<enc:EncryptedData><enc:CipherData>\
         <enc:CipherReference URI=\"EPUB/fonts/serif.otf\"/>\
         </enc:CipherData></enc:EncryptedData>",
    );
    assert_eq!(
        parse_encryption(&bytes, &limits()),
        Err(ArchiveRefusal::EncryptedResources)
    );
}

/// A file at that name whose root is not `encryption`, or whose markup will not
/// read, is the container being wrong rather than the book being encrypted.
///
/// Two different refusals for two different facts, and the pair matters: a
/// build that collapsed them would tell a host "this book is encrypted" about a
/// book that is merely malformed.
#[test]
fn an_encryption_file_that_will_not_read_is_unreadable_and_not_encrypted() {
    assert_eq!(
        parse_encryption(b"<encryption", &limits()),
        Err(ArchiveRefusal::UnreadableContainer)
    );
    assert_eq!(
        parse_encryption(
            b"<enc:EncryptedData xmlns:enc=\"http://www.w3.org/2001/04/xmlenc#\"/>",
            &limits()
        ),
        Err(ArchiveRefusal::UnreadableContainer)
    );
    // A `CipherReference` that is not a container path: the file is wrong, and
    // believing it would mean not knowing which resource is obfuscated.
    let bytes = encryption(&encrypted_data(IDPF_OBFUSCATION, "http://elsewhere/x.otf"));
    assert_eq!(
        parse_encryption(&bytes, &limits()),
        Err(ArchiveRefusal::UnreadableContainer)
    );
}

/// An `encryption.xml` with no `<EncryptedData>` at all is an empty answer and
/// not a refusal.
#[test]
fn an_empty_encryption_file_leaves_the_book_readable() {
    let parsed = parse_encryption(&encryption(""), &limits()).expect("nothing encrypted");
    assert!(parsed.entries().is_empty());
}

// ---- Section 5: the package document ----------------------------------------

/// A package document around a body, at version 3.0.
fn opf(body: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">{body}</package>"#
    )
    .into_bytes()
}

/// §5.5.3.1's three required elements, spelled the way both producers do.
const METADATA: &str = concat!(
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:00000000-0000-4000-8000-000000000000</dc:identifier>"#,
    r#"<dc:title>A Book</dc:title>"#,
    r#"<dc:language>en</dc:language>"#,
    r#"<dc:creator>Somebody</dc:creator>"#,
    r#"</metadata>"#
);

/// One manifest item and one spine itemref naming it.
const ONE_CHAPTER: &str = concat!(
    r#"<manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest>"#,
    r#"<spine><itemref idref="c1"/></spine>"#
);

fn package(body: &str) -> Result<Package, ArchiveRefusal> {
    parse_package(&opf(body), "EPUB/content.opf", &limits())
}

/// §5.5.3.1's three, and the `unique-identifier` milestone 9's key is the SHA-1
/// of.
///
/// Four claims and not one: the three elements are independent, and the fourth
/// — which `dc:identifier` the `unique-identifier` attribute *points at* — is
/// independent of all of them. A book may carry three identifiers and name
/// none of them, which is `PackageDefect::UniqueIdentifierUnresolved` and is
/// the only one of the four that leaves milestone 9 with no key.
#[test]
fn the_three_required_dublin_core_elements_and_the_unique_identifier() {
    let read = package(&format!("{METADATA}{ONE_CHAPTER}")).expect("a book");
    assert_eq!(read.title(), Some("A Book"));
    assert_eq!(read.language(), Some("en"));
    assert_eq!(read.creator(), Some("Somebody"));
    assert_eq!(
        read.unique_identifier(),
        Some("urn:uuid:00000000-0000-4000-8000-000000000000")
    );
    assert!(read.defects().is_empty(), "{:?}", read.defects());

    // Each of the two that stand alone, missing on its own and reported on its
    // own.
    for (drop_from, want) in [
        (r#"<dc:title>A Book</dc:title>"#, PackageDefect::NoTitle),
        (
            r#"<dc:language>en</dc:language>"#,
            PackageDefect::NoLanguage,
        ),
    ] {
        let read = package(&format!("{}{ONE_CHAPTER}", METADATA.replace(drop_from, "")))
            .expect("still a book");
        assert_eq!(read.defects(), [want], "dropping {drop_from}");
    }

    // The identifier is a pair: dropping it loses *two* facts, because the
    // `unique-identifier` then points at nothing either. A build reporting one
    // of the two would pass a test that only looked for the other.
    let no_id = METADATA.replace(
        r#"<dc:identifier id="pub-id">urn:uuid:00000000-0000-4000-8000-000000000000</dc:identifier>"#,
        "",
    );
    let read = package(&format!("{no_id}{ONE_CHAPTER}")).expect("still a book");
    assert_eq!(
        read.defects(),
        [
            PackageDefect::NoIdentifier,
            PackageDefect::UniqueIdentifierUnresolved
        ]
    );
    assert_eq!(read.unique_identifier(), None);

    // And the sharper half: identifiers present, and the attribute names none
    // of them. Only the second defect fires, and there is still no key.
    let elsewhere = METADATA.replace(r#"id="pub-id""#, r#"id="somewhere-else""#);
    let read = package(&format!("{elsewhere}{ONE_CHAPTER}")).expect("still a book");
    assert_eq!(read.defects(), [PackageDefect::UniqueIdentifierUnresolved]);
    assert_eq!(read.unique_identifier(), None);
}

/// A manifest `href` resolves against the **package document**, which is the
/// general rule milestone 3 built and had no caller for.
///
/// Both real shapes are here because both are real: 213 of the 412 hrefs in the
/// two corpora are flat and 199 name a directory, and one producer puts its
/// package document at the archive root while the other puts it under `EPUB/`.
/// A build that resolved against the container root reads every flat book
/// correctly and loses every nested one, which is the direction that looks like
/// a missing file.
#[test]
fn a_manifest_href_resolves_against_the_package_document() {
    for (opf_path, href, want) in [
        ("EPUB/content.opf", "text/ch1.xhtml", "EPUB/text/ch1.xhtml"),
        ("EPUB/content.opf", "ch1.xhtml", "EPUB/ch1.xhtml"),
        ("EPUB/content.opf", "../images/a.png", "images/a.png"),
        ("content.opf", "ch1.xhtml", "ch1.xhtml"),
        ("OEBPS/content.opf", "ch1.xhtml", "OEBPS/ch1.xhtml"),
        // A fragment is not part of a path, and milestone 8's cross-references
        // carry one on nearly every href.
        ("EPUB/content.opf", "ch1.xhtml#top", "EPUB/ch1.xhtml"),
        // Percent-decoded per segment, per §4.2.5 and RFC 3986.
        ("EPUB/content.opf", "a%20b.xhtml", "EPUB/a b.xhtml"),
    ] {
        let body = format!(
            r#"{METADATA}<manifest><item id="c1" href="{href}" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine>"#
        );
        let read = parse_package(&opf(&body), opf_path, &limits()).expect("a book");
        assert_eq!(
            read.items()[0].path.as_deref(),
            Some(want),
            "{href} from {opf_path}"
        );
    }

    // And a reference that is not a container path at all leaves no path, which
    // is a different fact from a path naming no entry.
    let body = format!(
        r#"{METADATA}<manifest><item id="c1" href="http://example.invalid/a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine>"#
    );
    let read = parse_package(&opf(&body), "EPUB/content.opf", &limits()).expect("a book");
    assert_eq!(read.items()[0].path, None);
}

/// §5.4.1's version: 2.0 and 3.x are read, everything else is refused **by
/// name**, and an absent attribute lands in the same place with the same
/// argument.
#[test]
fn a_package_version_is_two_or_three_and_anything_else_is_refused_by_name() {
    for (value, want) in [
        ("2.0", Some(PackageVersion::Epub2)),
        ("2.0.1", Some(PackageVersion::Epub2)),
        ("3.0", Some(PackageVersion::Epub3)),
        ("3.1", Some(PackageVersion::Epub3)),
        ("3.3", Some(PackageVersion::Epub3)),
        ("1.0", None),
        ("4.0", None),
        ("3", None),
        ("3.0beta", None),
        ("", None),
        ("three", None),
    ] {
        assert_eq!(PackageVersion::from_attribute(value), want, "{value:?}");
    }

    // Through the parser, which is where the refusal is a caller's answer.
    for value in ["2.0", "3.0"] {
        let markup = format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="{value}" unique-identifier="pub-id">{METADATA}{ONE_CHAPTER}</package>"#
        );
        assert!(parse_package(markup.as_bytes(), "content.opf", &limits()).is_ok());
    }
    for value in ["4.0", "1.0", "0.999"] {
        let markup = format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="{value}" unique-identifier="pub-id">{METADATA}{ONE_CHAPTER}</package>"#
        );
        assert_eq!(
            parse_package(markup.as_bytes(), "content.opf", &limits()),
            Err(ArchiveRefusal::UnsupportedPackageVersion),
            "{value}"
        );
    }
    // Absent, which §5.4.1 forbids and which this build cannot guess at.
    let markup = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" unique-identifier="pub-id">{METADATA}{ONE_CHAPTER}</package>"#
    );
    assert_eq!(
        parse_package(markup.as_bytes(), "content.opf", &limits()),
        Err(ArchiveRefusal::UnsupportedPackageVersion)
    );
}

/// EPUB 2.0's OPF reads through the same parser, with the shapes only it has.
///
/// `opf:role` and `opf:file-as` on a `dc:` element, `opf:scheme` on the
/// identifier, a `<guide>` beside the spine and no `properties` anywhere:
/// exactly what milestone 1 measured both producers writing into their EPUB 2
/// output. None of it changes what a spine is, which is the whole claim behind
/// calling EPUB 2 a compatibility surface rather than a second reader.
#[test]
fn an_epub_2_package_reads_through_the_same_parser() {
    let markup = concat!(
        r#"<?xml version='1.0' encoding='utf-8'?>"#,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uuid_id">"#,
        r#"<metadata xmlns:opf="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:title>A Short Account of Containers</dc:title>"#,
        r#"<dc:language>en</dc:language>"#,
        r#"<dc:creator opf:file-as="Unknown" opf:role="aut">The tinker-pdf authors</dc:creator>"#,
        r#"<dc:identifier id="uuid_id" opf:scheme="uuid">dce0f952-1c42-416b-85ab-c87b15b1125d</dc:identifier>"#,
        r#"</metadata>"#,
        r#"<manifest><item id="html4" href="index_split_000.html" media-type="application/xhtml+xml"/>"#,
        r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#,
        r#"<spine toc="ncx"><itemref idref="html4"/></spine>"#,
        r#"<guide><reference type="toc" title="TOC" href="toc.html"/></guide>"#,
        r#"</package>"#
    );
    let read = parse_package(markup.as_bytes(), "content.opf", &limits()).expect("a book");
    assert_eq!(read.version(), PackageVersion::Epub2);
    assert_eq!(read.title(), Some("A Short Account of Containers"));
    assert_eq!(read.creator(), Some("The tinker-pdf authors"));
    assert_eq!(
        read.unique_identifier(),
        Some("dce0f952-1c42-416b-85ab-c87b15b1125d")
    );
    assert_eq!(read.toc(), Some("ncx"));
    assert_eq!(read.spine().len(), 1);
    assert!(read.defects().is_empty(), "{:?}", read.defects());
    // `<guide>` is at depth two and is not a section this build reads; its
    // `<reference>` is neither an item nor an itemref.
    assert_eq!(read.items().len(), 2);
    assert_eq!(read.items()[1].core, Some(CoreMediaType::Ncx));
}

/// §3.2's core media types, and the two of them that are content documents.
#[test]
fn the_core_media_types_are_the_closed_set_section_3_2_names() {
    for (spelled, want) in [
        ("application/xhtml+xml", Some(CoreMediaType::Xhtml)),
        ("image/svg+xml", Some(CoreMediaType::Svg)),
        ("text/css", Some(CoreMediaType::Css)),
        // Case-insensitive and parameters discarded, per RFC 2045 §5.1.
        ("TEXT/CSS", Some(CoreMediaType::Css)),
        ("text/css; charset=utf-8", Some(CoreMediaType::Css)),
        ("  text/css  ", Some(CoreMediaType::Css)),
        ("image/png", Some(CoreMediaType::Png)),
        ("image/jpeg", Some(CoreMediaType::Jpeg)),
        ("image/gif", Some(CoreMediaType::Gif)),
        ("image/webp", Some(CoreMediaType::WebP)),
        ("audio/mpeg", Some(CoreMediaType::Mp3)),
        ("audio/ogg; codecs=opus", Some(CoreMediaType::OggOpus)),
        ("font/otf", Some(CoreMediaType::OpenType)),
        // 3.0's spelling of the same face, which both obfuscated samples use.
        ("application/vnd.ms-opentype", Some(CoreMediaType::OpenType)),
        ("application/font-woff", Some(CoreMediaType::Woff)),
        ("font/woff2", Some(CoreMediaType::Woff2)),
        ("text/javascript", Some(CoreMediaType::JavaScript)),
        ("application/x-dtbncx+xml", Some(CoreMediaType::Ncx)),
        ("application/pls+xml", Some(CoreMediaType::Pls)),
        // Real, from the fetched corpus, and in nobody's core set.
        ("application/x-epub-quiz", None),
        ("application/pdf", None),
        ("", None),
    ] {
        assert_eq!(CoreMediaType::from_media_type(spelled), want, "{spelled}");
    }

    // The two that are content documents, and the ones that are not. A build
    // that answered "core" where §5.7.2 asks "content document" would put a PNG
    // in the spine and call it a chapter.
    assert!(CoreMediaType::Xhtml.content_document());
    assert!(CoreMediaType::Svg.content_document());
    for other in [
        CoreMediaType::Css,
        CoreMediaType::Png,
        CoreMediaType::Ncx,
        CoreMediaType::OpenType,
    ] {
        assert!(!other.content_document(), "{other:?}");
    }
}

/// §5.6.2's `properties`, including a token no version of the specification
/// defines.
#[test]
fn manifest_properties_are_read_and_an_unknown_token_is_kept_as_written() {
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml" properties="mathml scripted"/>
           <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
           <item id="t" href="t.xhtml" media-type="application/xhtml+xml" properties="svg calibre:title-page"/>
           <item id="cover" href="c.png" media-type="image/png" properties="cover-image"/>
           </manifest><spine><itemref idref="c1"/></spine>"#
    );
    let read = package(&body).expect("a book");
    assert!(read.items()[0].has(Property::MathMl));
    assert!(read.items()[0].has(Property::Scripted));
    assert!(!read.items()[0].has(Property::Nav));
    assert_eq!(read.nav().map(|i| i.id.as_str()), Some("nav"));
    assert_eq!(read.cover_image().map(|i| i.id.as_str()), Some("cover"));
    // An extension token is kept and matches nothing, which is the honest
    // answer rather than an eighth property nobody defined.
    assert_eq!(read.items()[2].properties, ["svg", "calibre:title-page"]);
    assert_eq!(Property::from_token("calibre:title-page"), None);

    // Counted per book with the count, and `nav` and `cover-image` are absent
    // because they are honoured rather than unimplemented.
    let mut counted = read.unimplemented();
    counted.sort_by_key(|(p, _)| format!("{p:?}"));
    assert_eq!(
        counted,
        vec![
            (Property::MathMl, 1),
            (Property::Scripted, 1),
            (Property::Svg, 1)
        ]
    );
}

/// §3.5.1's fallback chain, and the two rules that bound it.
///
/// **A depth cap and a cycle guard are not one rule.** A chain of seventeen
/// distinct ids is not a cycle, and a cycle of two is not deep; each case is
/// here with its own name, so deleting either check fails a test the other
/// cannot.
#[test]
fn a_fallback_chain_reaches_a_content_document_or_says_why_it_did_not() {
    // A foreign resource falling back to XHTML: what §3.5.1 exists for.
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="odd" href="a.pdf" media-type="application/pdf" fallback="c1"/>
           <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
           </manifest><spine><itemref idref="odd"/></spine>"#
    );
    let read = package(&body).expect("a book");
    let start = read.item_by_id("odd").expect("the foreign item");
    assert_eq!(
        read.content_document(start).map(|i| i.id.as_str()),
        Ok("c1")
    );

    // A core media type that is not a content document does not terminate a
    // chain: a PNG is core and is not a chapter.
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="odd" href="a.pdf" media-type="application/pdf" fallback="pic"/>
           <item id="pic" href="a.png" media-type="image/png"/>
           </manifest><spine><itemref idref="odd"/></spine>"#
    );
    let read = package(&body).expect("a book");
    let start = read.item_by_id("odd").expect("the foreign item");
    assert_eq!(
        read.content_document(start),
        Err(FallbackDefect::NoFallback)
    );

    // A fallback naming nothing.
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="odd" href="a.pdf" media-type="application/pdf" fallback="nobody"/>
           <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
           </manifest><spine><itemref idref="odd"/></spine>"#
    );
    let read = package(&body).expect("a book");
    let start = read.item_by_id("odd").expect("the foreign item");
    assert_eq!(
        read.content_document(start),
        Err(FallbackDefect::Unresolved)
    );

    // A cycle of two, which is not deep.
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="a" href="a.pdf" media-type="application/pdf" fallback="b"/>
           <item id="b" href="b.pdf" media-type="application/pdf" fallback="a"/>
           </manifest><spine><itemref idref="a"/></spine>"#
    );
    let read = package(&body).expect("a book");
    let start = read.item_by_id("a").expect("the foreign item");
    assert_eq!(read.content_document(start), Err(FallbackDefect::Cyclic));

    // And a chain one past the cap, with every link distinct — which no cycle
    // guard can see.
    let links: String = (0..=MAX_EPUB_FALLBACK_DEPTH)
        .map(|at| {
            format!(
                r#"<item id="f{at}" href="f{at}.pdf" media-type="application/pdf" fallback="f{}"/>"#,
                at + 1
            )
        })
        .collect();
    let body = format!(
        r#"{METADATA}<manifest>{links}<item id="f{}" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="f0"/></spine>"#,
        MAX_EPUB_FALLBACK_DEPTH + 1
    );
    let read = package(&body).expect("a book");
    let start = read.item_by_id("f0").expect("the first link");
    assert_eq!(read.content_document(start), Err(FallbackDefect::TooDeep));

    // One link shorter and it resolves, which is what says the cap is where it
    // says it is rather than somewhere near it.
    let links: String = (0..MAX_EPUB_FALLBACK_DEPTH - 1)
        .map(|at| {
            format!(
                r#"<item id="f{at}" href="f{at}.pdf" media-type="application/pdf" fallback="f{}"/>"#,
                at + 1
            )
        })
        .collect();
    let body = format!(
        r#"{METADATA}<manifest>{links}<item id="f{}" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="f0"/></spine>"#,
        MAX_EPUB_FALLBACK_DEPTH - 1
    );
    let read = package(&body).expect("a book");
    let start = read.item_by_id("f0").expect("the first link");
    assert!(read.content_document(start).is_ok());
}

/// §5.7's spine: order, `linear`, and the spine element's own attributes.
#[test]
fn the_spine_is_read_in_document_order_and_non_linear_items_stay_in_it() {
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>
           <item id="b" href="b.xhtml" media-type="application/xhtml+xml"/>
           <item id="c" href="c.xhtml" media-type="application/xhtml+xml"/>
           </manifest>
           <spine toc="ncx" page-progression-direction="rtl">
           <itemref idref="a"/><itemref idref="b" linear="no"/><itemref idref="c" linear="yes"/>
           </spine>"#
    );
    let read = package(&body).expect("a book");
    let order: Vec<&str> = read.spine().iter().map(|s| s.idref.as_str()).collect();
    assert_eq!(order, ["a", "b", "c"], "document order is reading order");
    assert_eq!(
        read.spine().iter().map(|s| s.linear).collect::<Vec<_>>(),
        [true, false, true],
        "a non-linear item is recorded and keeps its place"
    );
    assert_eq!(read.toc(), Some("ncx"));
    assert!(read.right_to_left());
}

/// A package document with no `<manifest>`, no `<spine>` or an empty spine is
/// refused, and each by the name that is true of it.
#[test]
fn a_package_document_without_a_spine_is_refused_by_its_own_name() {
    for (body, want) in [
        (
            format!("{METADATA}<manifest/><spine/>"),
            ArchiveRefusal::EmptySpine,
        ),
        (
            format!("{METADATA}<manifest/>"),
            ArchiveRefusal::UnreadablePackageDocument,
        ),
        (
            format!(r#"{METADATA}<spine><itemref idref="c1"/></spine>"#),
            ArchiveRefusal::UnreadablePackageDocument,
        ),
    ] {
        assert_eq!(package(&body), Err(want), "{body}");
    }

    // Markup that is not a package document at all.
    assert_eq!(
        parse_package(b"<package", "content.opf", &limits()),
        Err(ArchiveRefusal::UnreadablePackageDocument)
    );
    // The right element in the wrong namespace, which is the near miss a
    // local-name comparison would take for a package document.
    let wrong_ns = format!(
        r#"<package xmlns="http://example.invalid/opf" version="3.0">{METADATA}{ONE_CHAPTER}</package>"#
    );
    assert_eq!(
        parse_package(wrong_ns.as_bytes(), "content.opf", &limits()),
        Err(ArchiveRefusal::UnreadablePackageDocument)
    );
}

/// A `<manifest>` and a `<spine>` past their caps refuse rather than truncate.
///
/// Truncating would produce a book with the right shape and the wrong pages,
/// which is this gap's own failure mode wearing a smaller hat — gap 30's
/// argument for refusing a package whose work total is spent, one format over.
#[test]
fn the_manifest_and_spine_caps_refuse_rather_than_truncate() {
    let l = limits();

    let items: String = (0..=l.max_manifest_items)
        .map(|at| {
            format!(r#"<item id="i{at}" href="c{at}.xhtml" media-type="application/xhtml+xml"/>"#)
        })
        .collect();
    let body =
        format!(r#"{METADATA}<manifest>{items}</manifest><spine><itemref idref="i0"/></spine>"#);
    assert_eq!(package(&body), Err(ArchiveRefusal::TooLarge));

    let refs: String = (0..=l.max_spine_items)
        .map(|_| r#"<itemref idref="c1"/>"#.to_owned())
        .collect();
    let body = format!(
        r#"{METADATA}<manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine>{refs}</spine>"#
    );
    assert_eq!(package(&body), Err(ArchiveRefusal::TooLarge));

    // **The spine cap is not bounded by the manifest cap**, which is what this
    // fixture says out loud: four thousand and ninety-six itemrefs naming
    // *one* manifest item. Gap 30 found the same shape in `PageContent`.
    let refs: String = (0..l.max_spine_items)
        .map(|_| r#"<itemref idref="c1"/>"#.to_owned())
        .collect();
    let body = format!(
        r#"{METADATA}<manifest><item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine>{refs}</spine>"#
    );
    let read = package(&body).expect("at the cap it is a book");
    assert_eq!(read.spine().len(), l.max_spine_items);
    assert_eq!(read.items().len(), 1);
}

/// §5.6.1 makes an item's `id` unique; two items with one `id` are read as the
/// first and warned about.
#[test]
fn two_manifest_items_with_one_id_are_read_as_the_first() {
    let body = format!(
        r#"{METADATA}<manifest>
           <item id="c1" href="first.xhtml" media-type="application/xhtml+xml"/>
           <item id="c1" href="second.xhtml" media-type="application/xhtml+xml"/>
           </manifest><spine><itemref idref="c1"/></spine>"#
    );
    let read = package(&body).expect("still a book");
    assert_eq!(read.defects(), [PackageDefect::DuplicateItemId]);
    assert_eq!(
        read.item_by_id("c1").map(|i| i.href.as_str()),
        Some("first.xhtml")
    );
}

/// A package document is read under `Doctype::Refuse`, which is a measurement
/// rather than milestone 2's relaxation being forgotten.
///
/// **Zero of the twenty-six package documents in either corpus carries a
/// document type declaration**, where 100 % of one producer's XHTML content
/// documents do. Extending the relaxation to a file that has never needed it
/// would widen the attack surface for nothing.
#[test]
fn a_doctype_on_a_package_document_is_refused_rather_than_skipped() {
    let markup = format!(
        r#"<?xml version="1.0"?><!DOCTYPE package><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">{METADATA}{ONE_CHAPTER}</package>"#
    );
    assert_eq!(
        parse_package(markup.as_bytes(), "content.opf", &limits()),
        Err(ArchiveRefusal::UnreadablePackageDocument)
    );
}

// ---- The page box a caller states -------------------------------------------

/// [`BookLayout::sanitised`] replaces each unusable number **on its own**, and
/// names each on its own.
///
/// Three fields, three defects, and a caller that passed a bad width and a bad
/// font size has two bugs rather than "unusable options". A build that replaced
/// the whole struct when one field was wrong would throw away a page box the
/// caller meant.
#[test]
fn an_unusable_open_option_is_replaced_on_its_own_and_named_on_its_own() {
    let (layout, defects) = BookLayout::sanitised(DEFAULT_PAGE, DEFAULT_FONT_SIZE);
    assert_eq!(layout.page, DEFAULT_PAGE);
    assert_eq!(layout.font_size, DEFAULT_FONT_SIZE);
    assert!(defects.is_empty());

    // Each field alone, and the others survive.
    let (layout, defects) = BookLayout::sanitised((f64::NAN, 800.0), 11.0);
    assert_eq!(layout.page, (DEFAULT_PAGE.0, 800.0));
    assert_eq!(layout.font_size, 11.0);
    assert_eq!(defects, [BookOptionDefect::PageWidth]);

    let (layout, defects) = BookLayout::sanitised((600.0, -1.0), 11.0);
    assert_eq!(layout.page, (600.0, DEFAULT_PAGE.1));
    assert_eq!(defects, [BookOptionDefect::PageHeight]);

    let (layout, defects) = BookLayout::sanitised((600.0, 800.0), 0.0);
    assert_eq!(layout.page, (600.0, 800.0));
    assert_eq!(layout.font_size, DEFAULT_FONT_SIZE);
    assert_eq!(defects, [BookOptionDefect::FontSize]);

    // Annex C.2's ceiling: 14 400 is a page and 14 401 is not.
    let (layout, defects) = BookLayout::sanitised((MAX_PAGE_SIDE, MAX_PAGE_SIDE), 12.0);
    assert_eq!(layout.page, (MAX_PAGE_SIDE, MAX_PAGE_SIDE));
    assert!(defects.is_empty());
    let (_, defects) = BookLayout::sanitised((MAX_PAGE_SIDE + 1.0, MAX_PAGE_SIDE + 1.0), 12.0);
    assert_eq!(
        defects,
        [BookOptionDefect::PageWidth, BookOptionDefect::PageHeight]
    );

    // A font larger than the page it would be set on is not a book. The bound
    // is the page rather than a constant, so a tall page keeps a large font.
    let (layout, defects) = BookLayout::sanitised((432.0, 648.0), 649.0);
    assert_eq!(layout.font_size, DEFAULT_FONT_SIZE);
    assert_eq!(defects, [BookOptionDefect::FontSize]);
    let (layout, defects) = BookLayout::sanitised((432.0, 2000.0), 649.0);
    assert_eq!(layout.font_size, 649.0);
    assert!(defects.is_empty());

    // And all three at once, each named.
    let (layout, defects) = BookLayout::sanitised((f64::INFINITY, f64::NEG_INFINITY), f64::NAN);
    assert_eq!(layout, BookLayout::default());
    assert_eq!(
        defects,
        [
            BookOptionDefect::PageWidth,
            BookOptionDefect::PageHeight,
            BookOptionDefect::FontSize
        ]
    );
}

// ---- The element tree (gap 31, milestone 8) ---------------------------------

fn dom(markup: &str) -> super::xhtml::Dom {
    super::xhtml::read(markup.as_bytes(), &XmlLimits::DEFAULT).expect("markup")
}

/// **A sibling is an *element* sibling**, and the text between two of them is
/// not one.
///
/// `selectors-4` §14's `+` and `~` walk element siblings; a build that took the
/// previous *node* would find a text node for every indented producer's output
/// and would match `li + li` on nothing at all. The fixture is indented the way
/// milestone 1's real books are, which is the only way this can be seen.
#[test]
fn siblings_skip_the_whitespace_between_them() {
    let tree = dom("<ul>\n  <li>a</li>\n  <li>b</li>\n  <li>c</li>\n</ul>");
    let items: Vec<usize> = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.name == "li")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(items.len(), 3);
    assert_eq!(tree.nodes[items[0]].previous, None);
    assert_eq!(tree.nodes[items[0]].next, Some(items[1]));
    assert_eq!(tree.nodes[items[1]].previous, Some(items[0]));
    assert_eq!(tree.nodes[items[1]].next, Some(items[2]));
    assert_eq!(tree.nodes[items[2]].next, None);
}

/// Elements arrive in document order with every parent before its child.
///
/// `tinker_pdf_css::cascade::cascade` refuses a slice that is not, by name, and
/// this is the producer that has to satisfy it. The tree is deliberately deep
/// and wide so that a builder that appended a child before its parent would be
/// caught rather than happening to agree.
#[test]
fn elements_are_in_document_order() {
    let tree = dom("<a><b><c/></b><d><e><f/></e></d></a>");
    for (index, node) in tree.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            assert!(parent < index, "{} is after its parent", node.name);
        }
    }
    let names: Vec<&str> = tree.nodes.iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, ["a", "b", "c", "d", "e", "f"]);
}

/// A CDATA section is character data, and losing it loses a stylesheet.
///
/// Not hypothetical: it is where a producer puts a `<style>` element's body.
#[test]
fn a_cdata_section_is_text_and_not_markup() {
    let tree = dom("<style><![CDATA[p { color: red }]]></style>");
    let text: String = tree.nodes[0]
        .children
        .iter()
        .map(|child| match child {
            super::xhtml::Child::Text(text) => text.clone(),
            super::xhtml::Child::Element(_) => String::new(),
        })
        .collect();
    assert_eq!(text, "p { color: red }");
}

/// **A document that stops half way keeps the tree it had**, and says so.
///
/// Ruling 2: a chapter with one unescaped `&` in its last paragraph should lose
/// the paragraph and not the chapter. Two claims — the defect is named and the
/// text before it survives — because a build that returned an empty tree with
/// the defect set would pass a test for either one alone.
#[test]
fn truncated_markup_keeps_what_it_read_and_names_the_defect() {
    let tree = dom("<body><p>kept</p><p>lost");
    assert!(tree
        .defects
        .contains(&super::xhtml::MarkupDefect::Truncated));
    assert!(tree.nodes.iter().any(|node| node.name == "p"));
    let text: String = tree
        .nodes
        .iter()
        .flat_map(|node| node.children.iter())
        .filter_map(|child| match child {
            super::xhtml::Child::Text(text) => Some(text.as_str()),
            super::xhtml::Child::Element(_) => None,
        })
        .collect();
    assert!(text.contains("kept"), "{text:?}");
}

/// `class` is a token list and `id` is not.
#[test]
fn the_class_attribute_is_a_token_list() {
    let tree = dom(r#"<p id="one two" class="  a   b  " style="color: red"/>"#);
    assert_eq!(tree.nodes[0].id.as_deref(), Some("one two"));
    assert_eq!(tree.nodes[0].classes, ["a", "b"]);
    assert_eq!(tree.nodes[0].style.as_deref(), Some("color: red"));
}

/// An element outside the XHTML namespace is **kept and known to be foreign**.
///
/// Two of the six committed books wrap their cover in an SVG `<image>`. Keeping
/// it costs a handful of nodes; forgetting that it is foreign would let this
/// build's user-agent rule for HTML's `<title>` hide an SVG one, or — worse —
/// let an SVG `<a>` become a link annotation.
#[test]
fn a_foreign_element_is_kept_and_is_not_html() {
    let tree = dom(concat!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml">"#,
        r#"<body><svg xmlns="http://www.w3.org/2000/svg"><title>a</title></svg></body></html>"#
    ));
    let svg = tree
        .nodes
        .iter()
        .find(|node| node.name == "svg")
        .expect("the svg survived");
    assert!(!svg.is_html());
    let title = tree
        .nodes
        .iter()
        .find(|node| node.name == "title")
        .expect("its title survived");
    assert!(!title.is_html(), "an SVG title is not HTML's");
    let body = tree
        .nodes
        .iter()
        .find(|node| node.name == "body")
        .expect("the body");
    assert!(body.is_html());
}

/// A document with no namespace at all is treated as XHTML.
///
/// EPUB 2's XHTML 1.1 profile permits it and one committed producer writes it,
/// so a build that required the namespace would set two of the six books with
/// no user-agent rules whatever.
#[test]
fn a_document_with_no_namespace_is_still_html() {
    let tree = dom("<html><body><p>a</p></body></html>");
    assert!(tree.nodes.iter().all(super::xhtml::Node::is_html));
}

/// `Dom::contains` walks **up**, and it is what turns a positioned run back
/// into the element it came from.
#[test]
fn containment_is_the_ancestor_chain() {
    let tree = dom("<a><b><c/></b><d/></a>");
    let at = |name: &str| tree.nodes.iter().position(|n| n.name == name).unwrap();
    assert!(tree.contains(at("a"), at("c")));
    assert!(tree.contains(at("b"), at("c")));
    assert!(
        tree.contains(at("c"), at("c")),
        "an element contains itself"
    );
    assert!(!tree.contains(at("d"), at("c")));
    assert!(
        !tree.contains(at("c"), at("a")),
        "containment is not symmetric"
    );
}

// ---- The anonymous inline box -----------------------------------------------

/// **A paragraph does not pay its own margin twice.**
///
/// CSS 2.2 §9.2.2.1 wraps a text node in an anonymous inline box that inherits
/// from its parent and has no margin, no padding, no border and no background.
/// Giving the text the parent's own computed style instead is the shortcut that
/// looks identical on a `<span>` and doubles every margin on a `<p>` —
/// `tinker-pdf-layout` would see a block-level child and open a second box for
/// it.
#[test]
fn a_paragraph_does_not_pay_its_own_margin_twice() {
    use tinker_pdf_css::cascade::ComputedStyle;
    use tinker_pdf_css::property::{
        Color, Display, LengthPercentage, MarginValue, Sides, TextDecoration,
    };

    let mut parent = ComputedStyle::initial();
    parent.display = Display::Block;
    parent.margin = Sides::all(MarginValue::Length(LengthPercentage::Px(16.0)));
    parent.background_color = Color::BLACK;
    parent.text_decoration = TextDecoration::Underline;
    parent.font_size = 24.0;

    let inline = super::read::inline_box(&parent);
    assert_eq!(inline.display, Display::Inline, "the text is not a block");
    assert_eq!(
        inline.margin,
        Sides::all(MarginValue::Length(LengthPercentage::ZERO)),
        "the text carries the paragraph's margin"
    );
    assert_eq!(inline.background_color, Color::TRANSPARENT);
    // Inherited, because §7.2 says so.
    assert_eq!(inline.font_size, 24.0);
    // And §16.3.1's decoration, which is **not** inherited and propagates
    // anyway: an `<a>`'s underline has to reach its own text, and a build that
    // dropped it here would underline nothing anywhere.
    assert_eq!(inline.text_decoration, TextDecoration::Underline);
}

/// `rel` is a token list, and two of its tokens decide whether a sheet is
/// applied.
///
/// **Neither corpus contains an alternate stylesheet**, so this rule is
/// unreachable from any real book and a defect injected into it survived the
/// whole suite. It is a function and a test for that reason: `rel="alternate
/// stylesheet"` is a theme the author marked as *not the default*, and applying
/// it would set the book in it.
#[test]
fn a_rel_token_list_decides_whether_a_stylesheet_is_applied() {
    use super::read::applies_as_stylesheet;
    assert!(applies_as_stylesheet("stylesheet"));
    assert!(applies_as_stylesheet("StyleSheet"), "the tokens fold case");
    assert!(
        applies_as_stylesheet("stylesheet next"),
        "a second token is not a reason to drop the sheet"
    );
    assert!(!applies_as_stylesheet("alternate stylesheet"));
    assert!(!applies_as_stylesheet("stylesheet alternate"));
    assert!(!applies_as_stylesheet(""));
    assert!(!applies_as_stylesheet("next"));
    assert!(
        !applies_as_stylesheet("stylesheets"),
        "a token is a whole token"
    );
}

/// The reader refuses a document that ends inside an element, so a partial tree
/// arrives **only** through that refusal.
///
/// The other half of the rule whose second enforcement was deleted: the tree
/// keeps what it read, the defect is named, and there is exactly one place that
/// names it.
#[test]
fn an_unterminated_element_is_the_readers_refusal_and_not_a_second_check() {
    let tree = dom("<body><p>kept</p><p>lost");
    assert_eq!(tree.defects, [super::xhtml::MarkupDefect::Truncated]);
    assert_eq!(
        tree.nodes.iter().filter(|node| node.name == "p").count(),
        2,
        "the element that was open when the document ended is still in the tree"
    );
    // And a document that ends properly has no defect at all, which is what
    // says the assertion above is about the document rather than about every
    // document.
    assert!(dom("<body><p>kept</p></body>").defects.is_empty());
}

// ---- Faces, and the codes a character is drawn with --------------------------

/// The twelve faces are twelve, and their indices are a bijection.
///
/// The resource names a document uses are `Bk{index}`, so two faces sharing an
/// index would draw one in the other's font and neither test nor renderer would
/// see it.
#[test]
fn every_face_has_its_own_index_and_its_own_base_font() {
    use std::collections::BTreeSet;
    let faces = super::paint::Face::all();
    assert_eq!(faces.len(), 12);
    let indices: BTreeSet<usize> = faces.iter().map(|face| face.index()).collect();
    assert_eq!(indices.len(), 12);
    assert_eq!(indices.iter().copied().max(), Some(11));
    let names: BTreeSet<&[u8]> = faces.iter().map(|face| face.base_font()).collect();
    assert_eq!(names.len(), 12, "two faces share a /BaseFont");
    let resources: BTreeSet<Vec<u8>> = faces.iter().map(|face| face.resource()).collect();
    let overflow: BTreeSet<Vec<u8>> = faces.iter().map(|face| face.overflow_resource()).collect();
    assert_eq!(resources.len(), 12);
    assert_eq!(overflow.len(), 12);
    assert!(
        resources.is_disjoint(&overflow),
        "a face's two fonts share a resource name"
    );
}

/// `css-fonts-4` §5's list is walked in the author's order, and a family this
/// build has never heard of moves to the next entry rather than ending the
/// walk.
#[test]
fn a_family_this_build_does_not_have_falls_through_to_the_next() {
    use super::paint::{Face, Generic};
    use tinker_pdf_css::property::{FontFamily, FontStyle};
    use tinker_pdf_layout::metrics::FontRequest;

    let face = |families: Vec<FontFamily>, weight: u16, style: FontStyle| {
        Face::of(&FontRequest {
            families: &families,
            weight,
            style,
            size: 16.0,
        })
    };

    // `Georgia, serif` is what pandoc writes on every book it produced here.
    let georgia = face(
        vec![FontFamily::Named("Georgia".to_owned()), FontFamily::Serif],
        400,
        FontStyle::Normal,
    );
    assert_eq!(georgia.generic, Generic::Serif);

    // A name nothing recognises, followed by one that is: the walk continues.
    let unknown = face(
        vec![
            FontFamily::Named("Nonesuch Display".to_owned()),
            FontFamily::Monospace,
        ],
        400,
        FontStyle::Normal,
    );
    assert_eq!(unknown.generic, Generic::Monospace);

    // And a list of nothing but names this build has never heard of resolves to
    // the initial family rather than to no face at all.
    let none = face(
        vec![FontFamily::Named("Nonesuch Display".to_owned())],
        400,
        FontStyle::Normal,
    );
    assert_eq!(none.generic, Generic::Serif);

    // `css-fonts-4` §2.2's threshold, either side of it.
    assert!(!face(vec![FontFamily::Serif], 500, FontStyle::Normal).bold);
    assert!(face(vec![FontFamily::Serif], 600, FontStyle::Normal).bold);
    assert!(face(vec![FontFamily::Serif], 400, FontStyle::Italic).italic);
    assert!(face(vec![FontFamily::Serif], 400, FontStyle::Oblique).italic);
}

/// The `WinAnsiEncoding` map, at the three ranges it is made of and at the
/// characters that are in none of them.
#[test]
fn the_encoding_covers_what_it_covers_and_says_so() {
    use super::paint::winansi_code;
    // Below 0x80 the encoding is ASCII by construction.
    assert_eq!(winansi_code('A'), Some(0x41));
    assert_eq!(winansi_code(' '), Some(0x20));
    // 0x80..=0x9F is the table, and this is where an em dash lives — the
    // character milestone 1's books carry ten of.
    assert_eq!(winansi_code('\u{2014}'), Some(0x97));
    assert_eq!(winansi_code('\u{2026}'), Some(0x85));
    assert_eq!(winansi_code('\u{2019}'), Some(0x92));
    // At and above 0xA0 it is Latin-1 by construction.
    assert_eq!(winansi_code('\u{e9}'), Some(0xE9));
    assert_eq!(winansi_code('\u{a0}'), Some(0xA0));
    // And the ones it does not cover, including the non-breaking hyphen the
    // corpus carries three of and the Japanese line it carries in five books.
    assert_eq!(winansi_code('\u{2011}'), None);
    assert_eq!(winansi_code('\u{65e5}'), None);
    assert_eq!(winansi_code('\u{4e00}'), None);
}

/// A character outside the encoding gets an **overflow code**, and the same
/// character gets the same one twice.
///
/// A build that allocated a fresh code per occurrence would exhaust the 224 on
/// the first paragraph of Japanese and would draw the same kanji from two
/// codes, which is a page that looks right and a `/Differences` array that is
/// nonsense.
#[test]
fn an_unencodable_character_gets_one_stable_code() {
    use super::paint::{Chosen, Coded, Face, Fonts, Generic, OVERFLOW_FIRST};
    use tinker_pdf_css::property::{FontFamily, FontStyle, FontVariant, TextDecoration};
    use tinker_pdf_layout::TextRun;

    let run = |text: &str| TextRun {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        text: text.to_owned(),
        font_size: 16.0,
        families: vec![FontFamily::Serif],
        weight: 400,
        style: FontStyle::Normal,
        variant: FontVariant::Normal,
        color: tinker_pdf_css::property::Color::BLACK,
        decoration: TextDecoration::None,
        painted: true,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        generated: false,
        anchor: None,
    };

    let faces = super::typeface::FaceSet::new();
    let mut fonts = Fonts::new(&faces);
    fonts.note(&run("a\u{65e5}b\u{672c}"));
    fonts.note(&run("\u{65e5}\u{65e5}"));
    assert_eq!(fonts.unrepresented(), 0);
    assert_eq!(
        fonts.overflow_fonts(),
        1,
        "one face needed one overflow font"
    );
    // **Two codes for two characters**, over four occurrences across two runs.
    // A build that allocated a code per occurrence encodes every book
    // identically -- `encode` finds the first entry either way -- and exhausts
    // the 224 four times as fast, which no page and no extracted string can
    // show.
    assert_eq!(
        fonts.codes(Face {
            generic: Generic::Serif,
            bold: false,
            italic: false
        }),
        2
    );

    let face = Face {
        generic: Generic::Serif,
        bold: false,
        italic: false,
    };
    let chosen = Chosen::Standard(face);
    // The encodable characters stay in the primary font.
    assert_eq!(
        fonts.encode(chosen, 'a'),
        Some(Coded::Simple {
            resource: face.resource(),
            code: b'a'
        })
    );
    // The others are in the overflow font, in the order they were first met,
    // and stably.
    assert_eq!(
        fonts.encode(chosen, '\u{65e5}'),
        Some(Coded::Simple {
            resource: face.overflow_resource(),
            code: OVERFLOW_FIRST
        })
    );
    assert_eq!(
        fonts.encode(chosen, '\u{672c}'),
        Some(Coded::Simple {
            resource: face.overflow_resource(),
            code: OVERFLOW_FIRST + 1
        })
    );
    assert_eq!(
        fonts.encode(chosen, '\u{65e5}'),
        Some(Coded::Simple {
            resource: face.overflow_resource(),
            code: OVERFLOW_FIRST
        }),
        "the same character was given two codes"
    );
    // A character nothing ever noted has no code at all, which is what stops a
    // page drawing a glyph the font dictionary does not describe.
    assert_eq!(fonts.encode(chosen, '\u{4e00}'), None);
    // And a face that drew nothing has no font.
    let bold = Face {
        generic: Generic::Serif,
        bold: true,
        italic: false,
    };
    assert_eq!(fonts.encode(Chosen::Standard(bold), '\u{65e5}'), None);
}

/// Past 224 distinct unencodable characters for one face, the rest are
/// **counted** rather than silently dropped.
///
/// `/Differences` has 256 codes and this build has no font program to embed
/// until milestone 9. What it must not do is lose the characters without
/// saying so, because text that is missing from a page and missing from
/// `Page::text()` is what text conservation exists to find.
#[test]
fn characters_past_the_overflow_font_are_counted() {
    use super::paint::{Fonts, OVERFLOW_CODES};
    use tinker_pdf_css::property::{Color, FontFamily, FontStyle, FontVariant, TextDecoration};
    use tinker_pdf_layout::TextRun;

    // 300 distinct CJK ideographs, of which 224 fit.
    let text: String = (0..300u32)
        .filter_map(|at| char::from_u32(0x4E00 + at))
        .collect();
    let faces = super::typeface::FaceSet::new();
    let mut fonts = Fonts::new(&faces);
    fonts.note(&TextRun {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        text,
        font_size: 16.0,
        families: vec![FontFamily::Serif],
        weight: 400,
        style: FontStyle::Normal,
        variant: FontVariant::Normal,
        color: Color::BLACK,
        decoration: TextDecoration::None,
        painted: true,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        generated: false,
        anchor: None,
    });
    assert_eq!(fonts.unrepresented(), 300 - OVERFLOW_CODES);
}

/// An East Asian character is one em wide, and a Latin one is its face's own
/// advance.
///
/// The standard 14 have no CJK glyph at all, so `Standard14::advance` answers
/// with a Latin space's width — which would set a Japanese line at a third of
/// its measure and would put the line breaker's UAX #14 work on the wrong
/// column.
#[test]
fn an_east_asian_character_is_one_em_wide() {
    use super::paint::BookMetrics;
    use tinker_pdf_css::property::{FontFamily, FontStyle};
    use tinker_pdf_layout::metrics::{FontRequest, Metrics};

    let families = vec![FontFamily::Serif];
    let font = FontRequest {
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        size: 20.0,
    };
    let metrics = BookMetrics::STANDARD;
    assert!((metrics.advance('\u{65e5}', &font) - 20.0).abs() < 1e-9);
    // Times-Roman's own published advance for `a` is 444/1000.
    assert!((metrics.advance('a', &font) - 20.0 * 0.444).abs() < 1e-9);
    // And the vertical metrics scale with the size rather than being constants.
    let vertical = metrics.vertical(&font);
    assert!((vertical.ascent - 20.0 * 0.683).abs() < 1e-9);
    assert!((vertical.descent - 20.0 * 0.217).abs() < 1e-9);
}

// ---- The table of contents ---------------------------------------------------

/// An NCX's `navMap`, nested, with the labels and the references it names.
#[test]
fn an_ncx_navmap_becomes_nested_entries() {
    let ncx = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">"#,
        r#"<head/><docTitle><text>A Book</text></docTitle><navMap>"#,
        r#"<navPoint id="a" playOrder="1"><navLabel><text>One</text></navLabel>"#,
        // The NCX DTD gives a `navPoint` an optional `navInfo` beside its
        // `navLabel`, and both hold a `<text>`. A build that took the last one
        // it saw would title this entry with its description.
        r#"<navInfo><text>A description nobody titles a chapter with</text></navInfo>"#,
        r#"<content src="ch1.xhtml"/>"#,
        "<navPoint id=\"b\" playOrder=\"2\"><navLabel><text>One  and\n  a half</text></navLabel>",
        r#"<content src="ch1.xhtml#half"/></navPoint>"#,
        r#"</navPoint>"#,
        r#"<navPoint id="c" playOrder="3"><navLabel><text>Two</text></navLabel>"#,
        r#"<content src="ch2.xhtml"/></navPoint>"#,
        r#"</navMap></ncx>"#
    );
    let entries = super::nav::from_ncx(ncx.as_bytes(), &XmlLimits::DEFAULT);
    assert_eq!(entries.len(), 2, "two top-level navPoints");
    assert_eq!(entries[0].title, "One");
    assert_eq!(entries[0].href.as_deref(), Some("ch1.xhtml"));
    assert_eq!(entries[0].children.len(), 1, "the nested one is nested");
    // The `docTitle`'s own `<text>` is outside `navMap` and is not an entry —
    // a build that took every `<text>` in the file would open every book with a
    // phantom first chapter.
    assert!(entries.iter().all(|entry| entry.title != "A Book"));
    // And the `navInfo` beside the `navLabel` is not the title either.
    assert_eq!(entries[0].title, "One");
    // Whitespace in a label is collapsed, because an outline entry is a PDF
    // text string and not a flow.
    assert_eq!(entries[0].children[0].title, "One and a half");
    assert_eq!(
        entries[0].children[0].href.as_deref(),
        Some("ch1.xhtml#half")
    );
    assert_eq!(entries[1].title, "Two");
    assert!(entries[1].children.is_empty());
}

/// A navigation document's `<nav epub:type="toc">` is the one §5.4.1.2
/// requires, and it is preferred over any other `<nav>` in the file.
///
/// Every pandoc book here writes a `landmarks` nav beside the toc, so a build
/// that took the first `<nav>` in document order would get the right answer on
/// a book whose toc happens to come first and the wrong one otherwise.
#[test]
fn the_toc_nav_wins_over_the_landmarks_nav() {
    let markup = concat!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"#,
        r#"<body>"#,
        r#"<nav epub:type="landmarks"><ol><li><a href="cover.xhtml">Cover</a></li></ol></nav>"#,
        r#"<nav epub:type="toc"><ol>"#,
        r#"<li><a href="ch1.xhtml">One</a><ol><li><a href="ch1.xhtml#a">One A</a></li></ol></li>"#,
        r#"<li><span>A heading</span></li>"#,
        r#"</ol></nav></body></html>"#
    );
    let entries = super::nav::from_navigation_document(&dom(markup));
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].title, "One");
    assert_eq!(entries[0].href.as_deref(), Some("ch1.xhtml"));
    assert_eq!(entries[0].children.len(), 1);
    assert_eq!(entries[0].children[0].title, "One A");
    // §5.4.1.2's second content model: a `<span>` is a heading that groups and
    // points nowhere, and 12.3.3 makes `/Dest` optional for exactly that.
    assert_eq!(entries[1].title, "A heading");
    assert_eq!(entries[1].href, None);
    assert!(
        entries.iter().all(|entry| entry.title != "Cover"),
        "the landmarks nav was read as the table of contents"
    );
}

/// A navigation document with **no `epub:type` at all** still yields its list.
///
/// pandoc's EPUB 2 output writes a bare `<div>` with an `<ol>` in it, and one
/// of the six committed books is that. Refusing it would lose a real book's
/// outline over an attribute.
#[test]
fn a_navigation_document_with_no_epub_type_still_has_a_list() {
    let markup = concat!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div id="toc">"#,
        r#"<ol><li><a href="ch1.xhtml">One</a></li></ol>"#,
        r#"</div></body></html>"#
    );
    let entries = super::nav::from_navigation_document(&dom(markup));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "One");
}
