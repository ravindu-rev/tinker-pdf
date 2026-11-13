# Progressive rendering

A viewer drawing a heavy page wants to show something before the page is
done, to keep its own loop answering while the page draws, and to stop
cleanly when the page is no longer wanted. Today a page renders in one call:
`Page::render` interprets and draws every operator before it returns, and a
`DisplayList::render` replays every recorded call before it returns. This
document settles how a render starts, pauses on a budget, hands back what it
has drawn so far, resumes from exactly where it stopped and finishes — with
the one property that makes the whole thing worth having: **a render paused at
every k-th event and resumed is byte-identical to an uninterrupted render**,
in pixels, size, format and warnings.

## Scope

- **The unit of progress**: a display list's events, the page content's and
  then each annotation's, counted in the order a direct render draws them.
- **The budget**: a count of events, so where a render pauses is a property
  of the page and the budget, never of the machine.
- **The partial result**: what the render has drawn so far, as a bitmap, at
  any pause, without changing what it goes on to draw.
- **The API**: start, step, progress, partial, finish.
- **How the renderer outlives one call**, what a page that is not retained
  does, cancellation, warnings, and ruling 5 for a progressive region.

## Non-goals

- **A resumable interpreter.** The interpreter runs a content stream to its
  end in one call, and forms, Type 3 glyph procedures and soft masks' groups
  run inside it on its own stack; pausing it in the middle of a form would
  mean saving that stack. A pause is a cursor over a *recording* instead, so
  only a retained page pauses: **a page whose display list is not retained —
  one past `MAX_DISPLAY_LIST_BYTES` — renders in one step**, which is the
  direct render.
- **Threads.** A progressive render draws on the thread that steps it, and
  starts none; drawing a page on several threads is P-15's pool, which a host
  may combine with this by rendering regions, each progressively.
- **Refinement by resolution.** A partial is the page at the asked scale with
  part of its content drawn, never a coarse picture of all of it.
- **A pause inside one event.** An event is drawn whole: one large image, one
  mesh or one glyph run's outline is one event, so the budget bounds how many
  calls a step makes and not how long any one of them takes.

## Design

### The unit and the budget

A retained page is a list of events (`tinker_pdf_content::Event`): every
fill, stroke, clip, glyph, image, shading, `q`, `Q`, form, group, soft mask
and marked-content scope the interpreter made, each with its state. A
progressive render walks **the content's events, then each annotation's** when
`RenderOptions::annotations` asks, in `/Annots` order — the order
`DisplayList::render` replays them in. Each annotation's events are replayed in
the resource scope its appearance resolves in, pushed before its first event
and popped after its last, as the direct replay pushes and pops it; the push
and the pop are not events.

**A budget is a number of events handed to the device.** `step(budget)` hands
over at most `budget` events and stops; an event skipped inside a bracket the
device declined, which the device is not handed, does not count against it
(below). So a step's work is a count, the pauses fall on the same events on
every target, at every scale and in every run, and **no clock enters the
engine**: a host that wants a step to take about sixteen milliseconds measures
its own clock between steps and adjusts the next budget, which decides only
when a pause comes, never what is drawn.

**Progress** is events done of the total: done counts every event the walk has
passed, handed over or skipped, and the total is the content's events plus,
when annotations are drawn, every annotation's. A step over a recording made
with the renderer's own answers — every retained page's — skips nothing, so
it advances progress by exactly its budget until the last step, which reaches
the total.

### The replay cursor (`tinker-pdf-content`, RC-15.2)

`replay` today walks a slice of events to its end, asking the device the three
questions again and skipping to the matching end of a bracket the device
declines where the recording entered it. The cursor is that walk with its
place kept: **the index of the next event, and the depth of the declined
bracket it is skipping, if any** — the two pieces of state the walk carries
from one event to the next. `Cursor::step(events, device, budget)` resumes the
walk at the index, hands the device events until it has handed `budget` or the
events end, and returns a `Replayed` for that step alone: delivered, skipped,
disagreed, cancelled. Skipped events met before the budget runs out are passed
in the same step; a step stops right after its last delivery, so skipped
events after it are the next step's. A budget of zero hands nothing and passes
nothing. `replay` becomes the cursor run to its end with a budget no list
reaches, so there is one walk and not two.

The interruptions a cursor carries across a pause are exactly the ones a walk
carries across an event: a bracket being skipped stays skipped, a question the
device answered differently is counted once, and a device that says it is
cancelled — before an event, as now — ends the walk: everything left is
counted skipped, `cancelled` is set, and every later step returns at once.
Summed, the steps' `Replayed`s are the whole replay's, and the events a device
receives are the same events in the same order whatever the budgets were.

### The partial result (`tinker-pdf-render`, RC-15.3)

`Renderer::snapshot(&self) -> Canvas` returns what the render has drawn so
far, **as `finish` would return it if the render stopped here**, built on
copies so that nothing the render goes on to draw changes:

- **The page-level canvas, copied**: the canvas a render of this rectangle
  started with, standing at its corner of the page frame.
- **The held image run, composited onto the copy and never onto the
  canvas.** A run of abutting images is held back so that it composites as one
  element (`tinker_pdf_raster::fragments`); composited early, the page would
  conflate the seams the run exists to hide. The copy takes the run's
  composite under the clip and soft mask in force, as `flush_run` would; the
  run itself goes on accumulating.
- **An open transparency group shows as if it closed now.** Its buffer is
  copied, its backdrop removed (11.4.7.2), and the copy composited onto a copy
  of what is under it at the group's alpha, blend mode and soft mask at the
  `Do`, after a knockout restore where the group under it knocks out — the
  steps `close_group` takes, on copies. Nested groups are closed so from the
  innermost outward.
- **An open soft mask's group shows nothing.** Its buffer is a mask in the
  making, never page content, and is installed only when its group ends; the
  partial shows what is under it as it stood when the `gs` opened it, and a
  group opened inside the mask's group is part of the mask and shows nothing
  either.

Each group's frame and each soft mask's group's records how many of the
other kind were open when it opened, which is the order the end of a render
closes them in and the order a snapshot walks; a group's buffer holds the
canvas under it, so a copy of the innermost holds every canvas the walk
closes onto. A snapshot taken with no bracket open is the canvas with the run
composited,
which is exactly what `finish` returns for the same calls. The cost of a
partial is a copy of the canvas and of every open group's buffer, paid only
when a host asks for one.

### The API (`tinker-pdf`, RC-15.4)

```rust
let list = page.display_list();
let mut render = list.render_progressively(&options);
while !render.is_finished() {
    let progress = render.step(256);          // at most 256 events
    show(render.partial());                   // the page so far
    let _ = (progress.done, progress.total);
}
let bitmap = render.finish();                 // Page::render's bitmap
```

- `DisplayList::render_progressively(&self, options) -> ProgressiveRender`
  **starts** a render; `Page::render_progressively(&self, options)` records the
  page's display list and starts it from that, which is the same render.
- `ProgressiveRender::step(&mut self, budget) -> Progress` draws at most
  `budget` events and returns the progress after them; a step after the last
  event draws nothing.
- `progress(&self) -> Progress`, `{ done, total }` in events, and
  `is_finished(&self)`, done equal to the total.
- `partial(&self) -> Bitmap` is the snapshot taken through the same end a
  finished render takes — its format conversion, its forced colours' tint and
  premultiplication — so a partial is the finished bitmap's shape and format,
  with less drawn on it.
- `finish(self) -> Bitmap` draws whatever is left and ends exactly as a direct
  render ends. A finished render's bitmap is `DisplayList::render`'s for the
  same options, byte for byte, and so `Page::render`'s.

**How the renderer outlives one call.** `Renderer<'g, G>` borrows the
resources it resolves names in, which ties a renderer to the call that made
it. A progressive render owns everything instead: the list's events behind an
`Arc` it shares with the list, its own resources for one render
(`PageResources::for_one_render_counted`, over the list's caches) behind an
`Arc`, the page, and a `Renderer<'static, PageResources>` made by a
constructor that takes the page scope as an `Arc` — the `Scope::Owned` the
renderer already keeps for a pushed scope. So a progressive render borrows
nothing, can be kept beside a viewer's other state between frames, and is
dropped like any value.

**One pipeline.** `Page::render_layer_with` is split at the paint: its
beginning — the scale and its clamp, the view and its transform, the canvas
format and canvas, the renderer and every option it carries — and its end —
the resources' warnings, the format conversion or forced colours' tint, and
premultiplication. A direct render is the beginning, the interpretation and
the end; a replay is the beginning, the replay and the end; a progressive
render is the beginning, the cursor's steps and the end, and a partial is the
end's conversion applied to a snapshot. Every render of a page is the one
pipeline (ruling 5).

**A page that is not retained** has no events to walk. Its progressive render
counts one unit, the page: the first step of any budget above zero draws the
page directly, as `Page::render` does, and reaches a progress of one of one;
its partial before that step is the page's background.

### Cancellation

`RenderOptions::cancel` reaches the renderer as it does for a direct render,
and the cursor asks the device before every event: a cancelled render's next
step hands nothing, counts the rest skipped and finishes the walk, and its
`finish` reports `RenderWarning::Cancelled` exactly as a direct render
cancelled at that event would. A host that stops wanting a page need not
cancel at all: it stops stepping and drops the render, which frees it.

### Warnings

A finished progressive render's warnings are the direct render's: each event
draws through the same renderer and the same resources a replay uses, the end
adds what it adds, and what the recording's interpretation could not resolve
is reported by a render that ran to its end, as `DisplayList::render` reports
it. A partial carries the warnings met so far — what the renderer and the
resources have recorded by that pause, and nothing `finish` adds only at the
end of a render that is not finished.

### Ruling 5, progressively

A progressive render of a `RenderOptions::region` is the region's render
stepped: the same canvas standing at the region's corner, every decision taken
on the page frame. Its finished bitmap is the direct render of the region,
which is the page's rectangle byte for byte, and a partial at any pause is the
page's partial at the same pause, cropped — the snapshot is the canvas's own
rectangle of the page frame and composites nothing a whole page would not.
The one decision ruling 5 names as the canvas's, whether an image joins a run,
is the same one here, and RC-21 is where it closes for partials and finished
tiles alike.

## Milestones

1. **RC-15.2, the cursor.** `crates/tinker-pdf-content/src/replay.rs`
   `a_replay_resumed_every_k_events_is_the_whole_replay`: for k from 1 to 8,
   over recordings with a declined bracket and a disagreeing one that pauses
   fall inside, and a device cancelled between two steps, the events a
   `RecordingDevice` receives and the summed `Replayed` equal one
   uninterrupted replay's, each step delivering exactly its budget until the
   last; the five replay tests before it pass unchanged.
2. **RC-15.3, the snapshot.** The render crate's
   `a_snapshot_changes_nothing_the_render_goes_on_to_draw`: calls holding an
   abutting image run, an open transparency group and a soft mask being
   drawn, snapshotted after every call, finish byte-identical to the render
   never snapshotted; each snapshot taken with no bracket open equals what a
   renderer fed the same calls finishes with.
3. **RC-15.4, the facade.** `crates/tinker-pdf/tests/determinism.rs`
   `a_render_paused_every_k_events_is_the_direct_render`: every fingerprint
   fixture, with budgets of 1, 2, 3, 7 and 64 events and a partial taken at
   every pause, finishes byte-identical to the direct render in pixels, size,
   format and warnings, with and without annotations, each step advancing its
   progress by exactly the budget until the last reaches the list's length,
   and a list that is not retained finishes in one step; entered in K-07's
   roster.
4. **RC-15.5a, the speed row, unmeasured.** In MS-04's render group, the
   150-dpi text page drawn from its display list in budgets of 256 events
   with a partial at every pause, beside the planning baseline's direct render
   of the same page; `cargo xtask baseline` holds the bench file and
   `baseline.json` to one list with it. RC-15.5b takes its figure, the first
   paired ratio, in the timing phase; no target is invented, which is the
   owner's (H-77).

## Risks

| Risk | Mitigation |
| --- | --- |
| A paused render draws differently from an uninterrupted one | The pause is a cursor over a recording: the device receives the same events in the same order whatever the budgets, and the determinism test holds every fingerprint page to it at five budgets |
| A partial changes what the render goes on to draw — an image run flushed early, a group closed early | A snapshot works on copies only, and its test snapshots after every call and holds the finished canvas to the one never snapshotted |
| A step's time is not bounded by its budget: one event can be a full-page image or a mesh | Stated: the budget bounds a step's calls, and a host measures its own clock between steps and adjusts the next budget; nothing in the engine reads a clock |
| A partial is expensive on a page of nested groups | A snapshot copies the canvas and each open group's buffer once, and is taken only when asked |
| A page too large to retain cannot pause | It renders in one step, the direct render, and its progress says one of one |
| The renderer borrows resources and cannot outlive the call that made it | A progressive render owns its resources behind an `Arc` and its renderer holds them as an owned scope |
| A region's partial differs from the page's partial cropped | The snapshot is the canvas's own rectangle of the one page frame; ruling 5's named image-run exception is the only difference, and RC-21 owns it |
