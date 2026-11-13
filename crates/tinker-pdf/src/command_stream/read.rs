//! Reading a command stream (`docs/design/command-stream.md`): its records
//! into the page's calls, each appearance's, the notes the interpretation
//! left where a replay reaches them, and the tables; each tiling cell's
//! stream read in turn, under the one meter.

use std::sync::Arc;

use tinker_pdf_content::commands::{
    read_stream, CommandError, CommandKind, EventReader, Meter, Nesting, Outcome, Record,
};
use tinker_pdf_content::Event;

use super::tables::{Defined, ImageKey, PageRecord, Tables};

/// One note the interpretation left in a list of calls, where a replay
/// reaches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// A font name the interpretation could not resolve.
    Missing(String),
    /// A colour space it read with a repair, and the repair.
    Repaired(String, String),
}

/// One list of calls — the page's content, or an appearance's — and the
/// notes between them, each at the number of calls before it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Layer {
    /// The scope the calls resolve names in: 0, the page's, for the content.
    pub(crate) scope: u64,
    /// Whether the appearance is a widget's (12.5.6.19); false for the
    /// content.
    pub(crate) widget: bool,
    pub(crate) events: Vec<Event>,
    pub(crate) notes: Vec<(usize, Note)>,
}

/// A stream read whole.
#[derive(Clone, Debug)]
pub(crate) struct Parsed {
    pub(crate) version: (u16, u16),
    /// The page record, for a page's stream; none for a cell's.
    pub(crate) page: Option<PageRecord>,
    pub(crate) content: Layer,
    pub(crate) annotations: Vec<Layer>,
    pub(crate) tables: Tables,
    /// Each cell's stream read, in the order of the tables' cells.
    pub(crate) cells: Vec<Arc<Parsed>>,
    /// Every inline image of the stream's calls, in order, by its dictionary
    /// and data: what an `InlineImage` entry's ordinal names.
    pub(crate) inline: Vec<(Vec<u8>, Vec<u8>)>,
    /// The records, as read.
    pub(crate) records: Vec<Record>,
}

/// Reads `bytes`, a page's stream where `nested` is false and a cell's,
/// `depth` cells deep, where it is true, charging `meter` for everything it
/// holds.
pub(crate) fn parse(
    bytes: &[u8],
    meter: &mut Meter,
    depth: u32,
    nested: bool,
) -> Result<Parsed, CommandError> {
    if depth > meter.budgets().depth {
        return Err(CommandError::OverBudget { budget: "depth" });
    }
    let stream = read_stream(bytes, meter)?;
    let mut reader = EventReader::new(stream.minor);
    let mut defined = Defined::default();
    let mut tables = Tables::default();
    let mut page = None;
    let mut content = Layer::default();
    let mut annotations: Vec<Layer> = Vec::new();
    for (at, record) in stream.records.iter().enumerate() {
        let layer = annotations.last_mut().unwrap_or(&mut content);
        let kind = match reader.read(record, &mut layer.events, meter)? {
            Outcome::Defined | Outcome::Commanded | Outcome::Skipped => continue,
            Outcome::Foreign(kind) => kind,
        };
        let fields = record.read();
        match kind {
            CommandKind::Page => {
                if nested || at != 0 {
                    return Err(fields.wrong("a page record other than a page stream's first"));
                }
                page = Some(PageRecord::read(&fields)?);
            }
            CommandKind::Annotation => {
                let scope = fields.unsigned(0)?;
                let is_appearance = usize::try_from(scope)
                    .ok()
                    .and_then(|s| s.checked_sub(1))
                    .and_then(|s| tables.scopes.get(s))
                    .is_some_and(|entry| entry.parent.is_none());
                if nested || !is_appearance {
                    return Err(fields.wrong("an appearance in no appearance's scope"));
                }
                annotations.push(Layer {
                    scope,
                    widget: fields.flag(1)?,
                    events: Vec::new(),
                    notes: Vec::new(),
                });
            }
            CommandKind::MissingFont => {
                let note = Note::Missing(fields.text(0)?);
                let layer = annotations.last_mut().unwrap_or(&mut content);
                layer.notes.push((layer.events.len(), note));
            }
            CommandKind::RepairedSpace => {
                let note = Note::Repaired(fields.text(0)?, fields.text(1)?);
                let layer = annotations.last_mut().unwrap_or(&mut content);
                layer.notes.push((layer.events.len(), note));
            }
            CommandKind::Profile => defined.read_profile(&fields)?,
            CommandKind::ColorSpace => defined.read_space(&fields, &reader, meter)?,
            CommandKind::ShadingDefinition => defined.read_shading(&fields, &reader, meter)?,
            other => tables.read(other, &fields, &defined, meter)?,
        }
    }
    if !nested && page.is_none() {
        return Err(CommandError::Malformed {
            kind: CommandKind::Page.number(),
            what: "a page's stream with no page record".to_string(),
        });
    }
    // Each list nests as a recording's.
    for layer in std::iter::once(&content).chain(&annotations) {
        let mut nesting = Nesting::new();
        for event in &layer.events {
            nesting.event(event)?;
        }
        nesting.close()?;
    }
    // The inline images the tables name by their place among the calls.
    let inline: Vec<(Vec<u8>, Vec<u8>)> = std::iter::once(&content)
        .chain(&annotations)
        .flat_map(|layer| layer.events.iter())
        .filter_map(|event| match event {
            Event::DrawImage { image, .. } if image.inline => {
                Some((image.inline_dict.clone(), image.inline_data.clone()))
            }
            _ => None,
        })
        .collect();
    let wrong = |what: &str| CommandError::Malformed {
        kind: CommandKind::ImageDecode.number(),
        what: what.to_string(),
    };
    for (index, image) in tables.images.iter().enumerate() {
        if let ImageKey::Inline(ordinal) = image.key {
            if usize::try_from(ordinal).map_or(true, |o| o >= inline.len()) {
                return Err(wrong("an inline image past the stream's calls"));
            }
        }
        // Every reduction a draw can ask for is held, once.
        let wanted: Vec<u32> = image.frame.map_or_else(
            || vec![1],
            |frame| frame.reductions().into_iter().map(|(r, _)| r).collect(),
        );
        let held: Vec<u32> = tables
            .decodes
            .iter()
            .filter(|decode| decode.image == index as u64)
            .map(|decode| decode.reduction)
            .collect();
        let once = held
            .iter()
            .enumerate()
            .all(|(at, r)| !held[..at].contains(r));
        if !once || wanted.iter().any(|r| !held.contains(r)) || held.len() != wanted.len() {
            return Err(wrong(
                "an image without a decode at each reduction a draw can ask for",
            ));
        }
    }
    let mut cells = Vec::with_capacity(tables.cells.len());
    for cell in &tables.cells {
        cells.push(Arc::new(parse(&cell.stream, meter, depth + 1, true)?));
    }
    Ok(Parsed {
        version: (stream.major, stream.minor),
        page,
        content,
        annotations,
        tables,
        cells,
        inline,
        records: stream.records,
    })
}
