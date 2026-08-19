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
