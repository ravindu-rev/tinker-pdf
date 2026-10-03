//! The byte layout of every struct a binding packs by hand.
//!
//! Go compiles against the header, so cgo lays these out itself. Ruby's Fiddle
//! and Java's FFM do not: bindings/ruby/lib/tinker_pdf.rb packs each one into
//! a string and bindings/java/.../TinkerPdf.java writes each field at an
//! offset, both transcribed from the numbers below. A field reordered, widened
//! or inserted here would make those bindings write garbage that still
//! compiles and still "works" until the engine reads the wrong field -- so the
//! numbers are pinned where the struct is defined, and moving one fails this
//! suite before it fails a caller.
//!
//! Sixty-four-bit targets only: a pointer is eight bytes in every offset
//! below, which is also what the hand-packed bindings assume (Ruby's `Q`,
//! Java's eight-byte address layout). It spawns nothing (ruling 13).
#![cfg(target_pointer_width = "64")]

use std::mem::{offset_of, size_of};

use tinker_pdf_ffi::{
    TpdfDate, TpdfDestination, TpdfDestinationRead, TpdfEmbeddedFile, TpdfEncryption, TpdfImage,
    TpdfPageLabelRange, TpdfSanitise, TpdfSourceVtable, TpdfTarget, TpdfWriteOptions,
};

#[test]
fn a_destination_is_a_kind_and_five_doubles() {
    assert_eq!(size_of::<TpdfDestination>(), 48);
    assert_eq!(offset_of!(TpdfDestination, kind), 0);
    assert_eq!(offset_of!(TpdfDestination, left), 8);
    assert_eq!(offset_of!(TpdfDestination, bottom), 16);
    assert_eq!(offset_of!(TpdfDestination, right), 24);
    assert_eq!(offset_of!(TpdfDestination, top), 32);
    assert_eq!(offset_of!(TpdfDestination, zoom), 40);
}

#[test]
fn a_target_carries_its_view_inline_and_its_uri_last() {
    assert_eq!(size_of::<TpdfTarget>(), 64);
    assert_eq!(offset_of!(TpdfTarget, kind), 0);
    assert_eq!(offset_of!(TpdfTarget, page_index), 4);
    assert_eq!(offset_of!(TpdfTarget, view), 8);
    assert_eq!(offset_of!(TpdfTarget, uri), 56);
}

#[test]
fn an_image_is_three_ints_a_pointer_and_a_length() {
    assert_eq!(size_of::<TpdfImage>(), 32);
    assert_eq!(offset_of!(TpdfImage, kind), 0);
    assert_eq!(offset_of!(TpdfImage, width), 4);
    assert_eq!(offset_of!(TpdfImage, height), 8);
    assert_eq!(offset_of!(TpdfImage, data), 16);
    assert_eq!(offset_of!(TpdfImage, data_len), 24);
}

#[test]
fn write_options_are_seven_ints_then_the_encryption_pointer() {
    assert_eq!(size_of::<TpdfWriteOptions>(), 40);
    assert_eq!(offset_of!(TpdfWriteOptions, mode), 0);
    assert_eq!(offset_of!(TpdfWriteOptions, linearize), 4);
    assert_eq!(offset_of!(TpdfWriteOptions, version_major), 8);
    assert_eq!(offset_of!(TpdfWriteOptions, version_minor), 12);
    assert_eq!(offset_of!(TpdfWriteOptions, object_streams), 16);
    assert_eq!(offset_of!(TpdfWriteOptions, compress), 20);
    assert_eq!(offset_of!(TpdfWriteOptions, garbage_collect), 24);
    assert_eq!(offset_of!(TpdfWriteOptions, encryption), 32);
}

#[test]
fn encryption_is_two_passwords_the_permissions_and_the_entropy() {
    assert_eq!(size_of::<TpdfEncryption>(), 40);
    assert_eq!(offset_of!(TpdfEncryption, user_password), 0);
    assert_eq!(offset_of!(TpdfEncryption, owner_password), 8);
    assert_eq!(offset_of!(TpdfEncryption, permissions), 16);
    assert_eq!(offset_of!(TpdfEncryption, entropy), 24);
    assert_eq!(offset_of!(TpdfEncryption, entropy_len), 32);
}

#[test]
fn a_destination_as_read_puts_its_view_after_the_page_reference() {
    assert_eq!(size_of::<TpdfDestinationRead>(), 72);
    assert_eq!(offset_of!(TpdfDestinationRead, kind), 0);
    assert_eq!(offset_of!(TpdfDestinationRead, has_page_index), 4);
    assert_eq!(offset_of!(TpdfDestinationRead, page_index), 8);
    assert_eq!(offset_of!(TpdfDestinationRead, has_page_ref), 12);
    assert_eq!(offset_of!(TpdfDestinationRead, page_object), 16);
    assert_eq!(offset_of!(TpdfDestinationRead, page_generation), 20);
    assert_eq!(offset_of!(TpdfDestinationRead, view), 24);
}

#[test]
fn a_page_label_range_pads_before_its_prefix_and_after_its_start() {
    assert_eq!(size_of::<TpdfPageLabelRange>(), 24);
    assert_eq!(offset_of!(TpdfPageLabelRange, first_page), 0);
    assert_eq!(offset_of!(TpdfPageLabelRange, style), 4);
    assert_eq!(offset_of!(TpdfPageLabelRange, prefix), 8);
    assert_eq!(offset_of!(TpdfPageLabelRange, start), 16);
}

#[test]
fn a_date_is_eight_ints_in_reading_order() {
    assert_eq!(size_of::<TpdfDate>(), 32);
    assert_eq!(offset_of!(TpdfDate, year), 0);
    assert_eq!(offset_of!(TpdfDate, month), 4);
    assert_eq!(offset_of!(TpdfDate, day), 8);
    assert_eq!(offset_of!(TpdfDate, hour), 12);
    assert_eq!(offset_of!(TpdfDate, minute), 16);
    assert_eq!(offset_of!(TpdfDate, second), 20);
    assert_eq!(offset_of!(TpdfDate, has_utc_offset), 24);
    assert_eq!(offset_of!(TpdfDate, utc_offset_minutes), 28);
}

#[test]
fn an_embedded_file_is_eight_words() {
    assert_eq!(size_of::<TpdfEmbeddedFile>(), 64);
    assert_eq!(offset_of!(TpdfEmbeddedFile, name), 0);
    assert_eq!(offset_of!(TpdfEmbeddedFile, filename), 8);
    assert_eq!(offset_of!(TpdfEmbeddedFile, description), 16);
    assert_eq!(offset_of!(TpdfEmbeddedFile, mime_type), 24);
    assert_eq!(offset_of!(TpdfEmbeddedFile, created), 32);
    assert_eq!(offset_of!(TpdfEmbeddedFile, modified), 40);
    assert_eq!(offset_of!(TpdfEmbeddedFile, data), 48);
    assert_eq!(offset_of!(TpdfEmbeddedFile, data_len), 56);
}

#[test]
fn a_sanitise_request_is_four_ints_in_the_order_it_names_them() {
    assert_eq!(size_of::<TpdfSanitise>(), 16);
    assert_eq!(offset_of!(TpdfSanitise, javascript), 0);
    assert_eq!(offset_of!(TpdfSanitise, actions), 4);
    assert_eq!(offset_of!(TpdfSanitise, embedded_files), 8);
    assert_eq!(offset_of!(TpdfSanitise, metadata), 12);
}

#[test]
fn a_source_vtable_is_three_function_pointers() {
    assert_eq!(size_of::<TpdfSourceVtable>(), 24);
    assert_eq!(offset_of!(TpdfSourceVtable, len), 0);
    assert_eq!(offset_of!(TpdfSourceVtable, read), 8);
    assert_eq!(offset_of!(TpdfSourceVtable, free), 16);
}
