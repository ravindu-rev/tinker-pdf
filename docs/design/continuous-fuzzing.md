# Continuous fuzzing

The fuzz targets in `fuzz/fuzz_targets/` run locally, on Linux, a session at
a time ([verification.md](../verification.md)). A public continuous-fuzzing
service runs a project's targets on its own machines every day, keeps
growing their corpora and reports the crashes it finds; what it needs from
a project is a build: a container recipe and a script that writes every
target, its seeds and its options into one output directory. This design
records what the service asks of a Rust project, settles the files that
live in this repository and the few that live in the service's, and makes
the build checkable here, by a local run of the same recipe and a
first-party check of what it wrote.

## Scope

- **The service's requirements for a Rust project**, recorded from the pages
  and the base image they name: the image, the environment its build script
  runs in, what the output directory holds for each target, and which
  configurations the service builds.
- **The in-tree integration** under `fuzz/service/`: the build script the
  service runs and the container recipe a local build runs it in.
- **The build mode**: the local fuzz sessions' own, so a crash the service
  reports reproduces in a session here.
- **The local build and its check**: the recipe built from the checkout it
  is given, the service's build run in it, and `cargo xtask fuzz
  --service-out DIR` reading what it wrote.
- **The service-side files** the application submits (H-119), which clone
  this repository and call the in-tree script, and nothing more.

## Non-goals

- **Running the targets.** Fuzzing is the local sessions' and, once the
  application is accepted, the service's; the local build builds and checks,
  and runs no target.
- **Dictionaries.** The service reads `<target>.dict` beside a target; none
  is written.
- **New targets.** They are K-04's. The build reads the roster off
  `fuzz/fuzz_targets/`, so a new target is built with no edit here.
- **The application itself**, and reading what the service reports: both
  are the owner's (H-119).

## Design

### What the service requires

Recorded from the service's new-project guide
(`https://google.github.io/oss-fuzz/getting-started/new-project-guide/`), its
Rust page (`…/new-project-guide/rust-lang/`), its FAQ
(`https://google.github.io/oss-fuzz/faq/`), and the base images' recipes and
scripts its repository publishes (`infra/base-images/base-builder-rust/`,
`infra/base-images/base-builder/`: `Dockerfile`, `compile`, `cargo`,
`install_rust.sh`, and `infra/helper.py`).

**A project is three files in the service's repository**, under
`projects/<name>/`: `project.yaml`, a `Dockerfile` and a `build.sh`. For
Rust, `project.yaml` says `language: rust`, and the only sanitizer and the
only engine the Rust page admits are `address` and `libfuzzer`; `homepage`,
`primary_contact` and `main_repo` name the project and who receives its
reports.

**The image is `gcr.io/oss-fuzz-base/base-builder-rust`**, named with no tag,
so a build takes whatever image the tag `latest` names that day. It is the
general builder (`base-builder`, on `base-clang`) with a Rust toolchain:
`CARGO_HOME=/rust`, `RUSTUP_HOME=/rust/rustup`, and one nightly pinned by
`RUSTUP_TOOLCHAIN`, which also overrides any `rust-toolchain` file in the
project; `cargo-fuzz` installed from crates.io with `--locked` when the image
is made; `clang` as the x86-64 Linux linker
(`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER`); and `zip`, `git`, `jq` and
Python 3 on the path. A project's `Dockerfile` starts `FROM` it, clones the
source into `$SRC` (`/src`), sets `WORKDIR` there and copies its `build.sh` to
`$SRC/`.

**The build runs the image's `compile`**, with the configuration in the
environment — `SANITIZER`, `FUZZING_ENGINE`, `ARCHITECTURE` and
`FUZZING_LANGUAGE` from `project.yaml` — and `$OUT` (`/out`) and `$WORK`
(`/work`) mounted. `compile` first runs `sysctl -w vm.mmap_rnd_bits=28`, so
the container is privileged (the helper passes `--privileged`); for a Rust
project under AddressSanitizer it sets
`RUSTFLAGS="--cfg fuzzing -Zsanitizer=address -Cdebuginfo=1 -Cforce-frame-pointers"`
and points `libfuzzer-sys` at the image's prebuilt libFuzzer
(`CUSTOM_LIBFUZZER_PATH`); then it runs `bash -eux $SRC/build.sh`.

**The output directory holds, for each target `<name>`:**

| File | What it is |
| --- | --- |
| `$OUT/<name>` | the target's binary |
| `$OUT/<name>_seed_corpus.zip` | its seeds: each file in the archive is a starting input, and the archive is used because it sits beside the binary under that name |
| `$OUT/<name>.options` | libFuzzer's flags for the target, an INI file with a `[libfuzzer]` section of `flag = value` lines, the flag without its dash — the FAQ's example sets `rss_limit_mb = 6000` and `timeout = 30` |
| `$OUT/<name>.dict` | a dictionary, used when present |

At run time only `$OUT` reaches the service's machines, everything but
`/tmp` is read-only, and a target may assume no working directory. Without
an options file a single input may take about 25 seconds and 2.5 GB before
it is reported as a timeout or an out-of-memory bug.

**The configurations.** For a Rust project the service builds the fuzzing
build — libFuzzer with AddressSanitizer on x86-64 — and two that make
reports: a coverage build (`SANITIZER=coverage`), for which `compile` adds
`-Cinstrument-coverage` and the image's `cargo` wrapper turns `cargo fuzz
build` into a debug `cargo build --bins` inside `fuzz/` with debug
assertions off, copying the binaries to where a release build would leave
them; and an introspector build (`SANITIZER=introspector`), whose
`RUSTFLAGS` carry `-Cdebuginfo=2` for the service's analysis. **The local
build builds the first**, because it is the one that finds crashes and the
one the local sessions run. The other two produce reports only the service
reads, out of the service's own tooling, and would check nothing the address
build does not; they are left to the service, and the build script is
written so that the wrapper still works (below).

**The service's local tooling** (`infra/helper.py`) builds the project's
image (`build_image`), runs `compile` in it (`build_fuzzers`) and then runs
the targets in its runner image (`check_build`). The last runs targets and
judges them by the service's program, which ruling 13 does not let decide
anything here; `cargo xtask fuzz --service-out` is the check in its place.

### The in-tree files

```text
fuzz/service/build.sh      the build the service runs
fuzz/service/Dockerfile    the recipe a local build runs it in
```

**`fuzz/service/build.sh`** finds the checkout from its own path, two
directories up, so it runs the same whatever directory it is called from.
It reads the roster as `fuzzaudit::survey` does — every `*.rs` in
`fuzz/fuzz_targets/`, by its stem — so no list of names is written anywhere.
From the checkout's root it runs `cargo fuzz build`, exactly that: its
defaults are the local sessions' mode (below), and the image's coverage
wrapper recognises only a `cargo` whose first two arguments are `fuzz build`,
going into `fuzz/` itself and leaving the binaries in
`fuzz/target/x86_64-unknown-linux-gnu/release/`. Then, for each target, it
copies the binary from there to `$OUT/<name>`, packs every file of
`fuzz/corpus/<name>/` — dotted names included, no directory entries, no
path — into `$OUT/<name>_seed_corpus.zip`, and writes `$OUT/<name>.options`:

```ini
[libfuzzer]
timeout = 20
```

It runs under `set -eu`, so a target that does not build, a corpus that is
not there or a write that fails stops the build. It leaves `RUSTFLAGS` as
the environment gives it, never adding `-D warnings`: a lint a new nightly
adds is a warning, not a build the service reports as broken.

**The build mode is the local sessions'.** `cargo fuzz build` with no flag
builds as `cargo build --release` does, with debug assertions and overflow
checks on, under AddressSanitizer — what `docs/verification.md`'s sessions
run — where the Rust page's example passes `-O`, which turns the assertions
off. A crash the service reports is then one a session here reproduces with
the same binary flags, and `timeout = 20` is the sessions' `-timeout=20`, so
an input the service reports as a timeout is one a session times out on
too. The seeds are packed when the build runs, from the corpus of the
commit being built, and no archive is ever committed.

**`fuzz/service/Dockerfile`** is the service-side recipe with the clone
replaced by the checkout it is built from:

```dockerfile
FROM gcr.io/oss-fuzz-base/base-builder-rust
COPY . $SRC/tinker-pdf
WORKDIR $SRC/tinker-pdf
RUN printf '#!/bin/bash -eu\nexec bash "$SRC/tinker-pdf/fuzz/service/build.sh"\n' > $SRC/build.sh
```

and its context is the commit, written by `git archive`: a local build
builds the commit under test, exactly — nothing a working tree holds besides
it, no build output and no fetched corpus — through the same `build.sh` the
service-side files hand the service.

**The local build is three commands**, from the checkout's root, on a
machine with Docker running Linux containers:

```sh
git archive HEAD | docker build -f fuzz/service/Dockerfile -t tinker-pdf-fuzz-service -
docker run --rm --privileged -e FUZZING_LANGUAGE=rust -e SANITIZER=address \
  -e FUZZING_ENGINE=libfuzzer -e ARCHITECTURE=x86_64 \
  -v "$PWD/target/fuzz-service:/out" tinker-pdf-fuzz-service compile
cargo xtask fuzz --service-out target/fuzz-service
```

the second as the helper's `build_fuzzers` runs it, the configuration the
service's address build sets.

### The check

`cargo xtask fuzz --service-out DIR` is a first-party reading of what a
build wrote. It reads the roster off `fuzz/fuzz_targets/` as
`fuzzaudit::survey` does, and names every target whose binary `DIR/<name>`
is missing or is not an ELF file (its first four bytes `7F 45 4C 46`); whose
seed archive `DIR/<name>_seed_corpus.zip` is missing, or does not read back
through `tinker_pdf_zip::Archive` — every entry's CRC-32 checked — to exactly
the files of `fuzz/corpus/<name>/`, by name and byte for byte, none missing
and none more; or whose options file `DIR/<name>.options` holds no
`[libfuzzer]` section setting `timeout` to the sessions' 20 seconds. It reads
files and spawns nothing. Anything else in `DIR` is left alone: the service's
own files are not this check's.

### The service-side files

The application (H-119) submits three files under
`projects/tinker-pdf/` in the service's repository. They clone and call;
everything the build does is in this repository, where a change to it is a
commit here and not a change request there.

```yaml
# project.yaml
homepage: "https://github.com/ravindu-rev/tinker-pdf"
language: rust
main_repo: "https://github.com/ravindu-rev/tinker-pdf"
primary_contact: "<the owner's address, given in the application>"
sanitizers:
  - address
fuzzing_engines:
  - libfuzzer
```

```dockerfile
# Dockerfile
FROM gcr.io/oss-fuzz-base/base-builder-rust
RUN git clone --depth 1 --branch develop https://github.com/ravindu-rev/tinker-pdf tinker-pdf
WORKDIR $SRC/tinker-pdf
COPY build.sh $SRC/
```

```sh
#!/bin/bash -eu
# build.sh
exec bash "$SRC/tinker-pdf/fuzz/service/build.sh"
```

The service fuzzes `develop`, where every change lands, so a crash is found
before the owner's merge to `main` releases it; a fuzz crash blocks a
release (ruling 1).

## Milestones

1. **K-10.2, the check.** `cargo xtask fuzz --service-out DIR` in
   `xtask/src/fuzzservice.rs`, documented in `USAGE`; `TOOLS`' `xtask` row
   gains `tinker-pdf-zip` and `tinker-pdf-filters`, which the check and its
   tests read archives with. Exit: `xtask/src/fuzzservice.rs`'s unit tests,
   each over a two-target root staged in the temp directory —
   `a_complete_service_output_passes`, `a_target_without_its_binary_is_named`,
   `a_binary_that_is_not_elf_is_named`, `a_seed_archive_missing_a_seed_is_named`,
   `a_seed_archive_with_a_stray_entry_is_named`,
   `a_seed_archive_with_a_changed_byte_is_named` and
   `an_options_file_without_the_session_timeout_is_named` — and
   `this_repository_obeys_its_own_graph` with the new edges.
2. **K-10.3, the integration, built here.** `fuzz/service/build.sh` and
   `fuzz/service/Dockerfile`; `fuzz/README.md` says
   how to build the integration locally and `docs/verification.md`'s fuzz
   section names the local build. Exit: one local run, recorded in the
   commit: the image builds, its `compile` builds every target
   `fuzz/fuzz_targets/` holds, and `cargo xtask fuzz --service-out` passes
   over the output directory.

## Risks

| Risk | Mitigation |
| --- | --- |
| The toolchain the image carries moves under the build: the service bumps `RUSTUP_TOOLCHAIN`, and a nightly can warn on or refuse code that builds today | Warnings never fail the build (`RUSTFLAGS` is left as the image sets it); a refusal is a build failure the service reports, and a local build on the same image shows it first. Each local run's commit records the image's digest and its `rustc` and `cargo-fuzz` versions |
| The image's tag moves: `latest` is a different image from one week to the next | The local recipe names the same unpinned image the service builds on, since a pinned digest would check an image the service no longer uses; the run's commit records which image it was |
| Build time: every target in release with sanitizer instrumentation and one codegen unit | One `cargo fuzz build` builds them all over one shared dependency graph; the cost is the service's machines', and a local run's is recorded in its commit |
| The coverage build the local build does not make breaks unseen | `build.sh` calls exactly `cargo fuzz build` from the checkout's root and reads the binaries from the release directory, the two things the image's wrapper relies on; anything else would be a step the wrapper does not know |
| A seed corpus a session wrote into without minimising grows the archive past what the service keeps | The archive is what `fuzz/corpus/` holds, which `fuzz/README.md` asks to keep minimised and small enough to review; the check compares it file for file, so nothing is added or lost in packing |
| The service's own check of a build, which runs the targets, disagrees with this one | It is the service's: it decides what the service accepts, and nothing here. What this repository holds a build to is `cargo xtask fuzz --service-out` |
